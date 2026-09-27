#!/usr/bin/env python3
"""Coordinate the complete release; only verified uploads can become latest."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.request import Request, urlopen


def stable_version(version):
    if not re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", version):
        raise ValueError("Use a stable Cargo version X.Y.Z and mark the GitHub release as prerelease while it builds.")
    return tuple(map(int, version.split(".")))


def asset_groups(version):
    return {
        f"Whisple-{version}-macos-arm64": (".dmg", ".zip"),
        f"Whisple-{version}-macos-x86_64": (".dmg", ".zip"),
        f"Whisple-{version}-windows-x86_64": ("-setup.exe", ".msi", ".zip"),
    }


def verify_files(directory, version):
    """Require all ten files and independently verify every manifest entry."""
    expected = {
        prefix + suffix
        for prefix, suffixes in asset_groups(version).items()
        for suffix in (*suffixes, ".sha256")
    }
    actual = {path.name for path in directory.iterdir()}
    if actual != expected:
        raise ValueError(f"Release files differ: missing={expected - actual}, unexpected={actual - expected}")
    files = {}
    for name in sorted(expected):
        path = directory / name
        if not path.is_file() or path.is_symlink() or path.stat().st_size == 0:
            raise ValueError(f"Invalid or empty release file: {name}")
        with path.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        files[name] = {"size": path.stat().st_size, "digest": f"sha256:{digest}"}
    for prefix, suffixes in asset_groups(version).items():
        entries = {}
        for line in (directory / (prefix + ".sha256")).read_text().splitlines():
            match = re.fullmatch(r"([0-9a-f]{64})  (.+)", line)
            if not match or match[2] in entries:
                raise ValueError(f"Invalid checksum manifest: {prefix}")
            entries[match[2]] = "sha256:" + match[1]
        if set(entries) != {prefix + suffix for suffix in suffixes}:
            raise ValueError(f"Incomplete checksum manifest: {prefix}")
        for name, digest in entries.items():
            if digest != files[name]["digest"]:
                raise ValueError(f"Checksum mismatch: {name}")
    return files


def verify_uploaded(release, files):
    assets = release["assets"]
    for name, expected in files.items():
        matches = [asset for asset in assets if asset["name"] == name]
        if len(matches) != 1:
            raise ValueError(f"Missing or duplicate uploaded asset: {name}")
        asset = matches[0]
        if asset["state"] != "uploaded" or any(asset.get(key) != value for key, value in expected.items()):
            raise ValueError(f"Uploaded asset does not match the verified build: {name}")


class GitHub:
    def __init__(self, repo):
        self.repo = repo

    def api(self, endpoint, data=None, allow_missing=False):
        request = Request(
            f"https://api.github.com/repos/{self.repo}/{endpoint}",
            data=json.dumps(data).encode() if data is not None else None,
            method="PATCH" if data is not None else "GET",
            headers={
                "Authorization": f"Bearer {os.environ['GH_TOKEN']}",
                "Accept": "application/vnd.github+json",
                "Content-Type": "application/json",
                "X-GitHub-Api-Version": "2022-11-28",
                "User-Agent": "Whisple-release",
            },
        )
        try:
            with urlopen(request, timeout=60) as response:
                return json.load(response)
        except HTTPError as error:
            if allow_missing and error.code == 404:
                return None
            raise RuntimeError(f"GitHub {request.method} {endpoint} failed with HTTP {error.code}") from error

    def upload(self, tag, paths):
        subprocess.run(["gh", "release", "upload", tag, *map(str, paths), "--repo", self.repo, "--clobber"], check=True)


def require_prerelease(release, tag):
    if release["tag_name"] != tag or release["draft"] or not release["prerelease"]:
        raise ValueError("The release must still be a published prerelease before assets are replaced.")
    if release.get("immutable"):
        raise ValueError("An immutable release cannot accept build assets.")


def publish(github, release_id, version, directory, files):
    tag = "v" + version
    endpoint = f"releases/{release_id}"
    require_prerelease(github.api(endpoint), tag)
    github.upload(tag, [directory / name for name in files])
    release = github.api(endpoint)
    require_prerelease(release, tag)
    verify_uploaded(release, files)
    latest = github.api("releases/latest", allow_missing=True)
    if latest and stable_version(latest["tag_name"].removeprefix("v")) > stable_version(version):
        raise ValueError("A newer release is already latest; refusing to replace it with this older build.")
    promoted = github.api(endpoint, {"prerelease": False, "make_latest": "true"})
    if promoted["prerelease"] or promoted["draft"]:
        raise ValueError("GitHub did not promote the release.")
    if github.api("releases/latest")["id"] != release_id:
        raise ValueError("GitHub latest does not point to the completed release.")
    print(f"Published {tag}: all {len(files)} assets verified, prerelease disabled, latest confirmed.")


def main():
    publishing = os.environ.get("PUBLISH_RELEASE") == "true"
    github = GitHub(os.environ["GITHUB_REPOSITORY"])
    if sys.argv[1] == "prepare":
        version = tomllib.loads(Path("Cargo.toml").read_text())["package"]["version"]
        stable_version(version)
        source = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
        release_id = ""
        if publishing:
            tag = os.environ["RELEASE_TAG"]
            if tag != "v" + version:
                raise ValueError(f"Release tag {tag} does not match Cargo version {version}.")
            release = github.api(f"releases/tags/{quote(tag, safe='')}")
            if release["draft"] or release.get("immutable"):
                raise ValueError("Publish an editable prerelease before starting the build.")
            release_id = release["id"]
            # Also handle someone publishing a regular release by mistake.
            release = github.api(f"releases/{release_id}", {"prerelease": True, "make_latest": "false"})
            require_prerelease(release, tag)
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            output.write(f"source={source}\nversion={version}\nrelease_id={release_id}\n")
        print(f"Building Whisple {version} from {source}; publishing={publishing}.")
    elif sys.argv[1] == "finalize":
        version = os.environ["RELEASE_VERSION"]
        stable_version(version)
        directory = Path(sys.argv[2])
        files = verify_files(directory, version)
        print(f"Verified the complete set of {len(files)} release files and all checksums.")
        if publishing:
            publish(github, int(os.environ["RELEASE_ID"]), version, directory, files)
        else:
            print("Verification complete. No release was changed.")
    else:
        raise ValueError("Expected prepare or finalize.")


if __name__ == "__main__":
    main()
