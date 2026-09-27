import copy
import hashlib
from pathlib import Path
import tempfile
import unittest

from release import asset_groups, publish, stable_version, verify_files


class FakeGitHub:
    def __init__(self, files):
        self.release = {"id": 42, "tag_name": "v1.2.3", "draft": False, "prerelease": True,
                        "assets": [{"name": name, "state": "uploaded", **value} for name, value in files.items()]}
        self.latest = None
        self.promoted = False
        self.uploaded = False
        self.fail_upload = False

    def upload(self, tag, paths):
        if self.fail_upload:
            raise RuntimeError("upload failed")
        self.uploaded = True

    def api(self, endpoint, data=None, **kwargs):
        if data:
            assert self.uploaded
            assert data == {"prerelease": False, "make_latest": "true"}
            self.promoted = True
            self.release["prerelease"] = False
            self.latest = self.release
        return copy.deepcopy(self.latest if endpoint == "releases/latest" else self.release)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        for prefix, suffixes in asset_groups("1.2.3").items():
            entries = []
            for suffix in suffixes:
                name = prefix + suffix
                data = name.encode()
                (self.directory / name).write_bytes(data)
                entries.append(f"{hashlib.sha256(data).hexdigest()}  {name}\n")
            (self.directory / (prefix + ".sha256")).write_text("".join(entries))
        self.files = verify_files(self.directory, "1.2.3")

    def test_all_platforms_are_required(self):
        self.assertEqual(len(self.files), 10)
        (self.directory / "Whisple-1.2.3-windows-x86_64.msi").unlink()
        with self.assertRaisesRegex(ValueError, "missing"):
            verify_files(self.directory, "1.2.3")

    def test_corrupt_files_fail_before_publication(self):
        (self.directory / "Whisple-1.2.3-macos-arm64.zip").write_bytes(b"corrupt")
        with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
            verify_files(self.directory, "1.2.3")

    def test_promotes_only_after_verified_uploads(self):
        github = FakeGitHub(self.files)
        publish(github, 42, "1.2.3", self.directory, self.files)
        self.assertTrue(github.promoted)

    def test_upload_failure_leaves_prerelease(self):
        github = FakeGitHub(self.files)
        github.fail_upload = True
        with self.assertRaises(RuntimeError):
            publish(github, 42, "1.2.3", self.directory, self.files)
        self.assertFalse(github.promoted)

    def test_missing_or_mismatched_remote_assets_block_promotion(self):
        for change in ("missing", "digest", "state", "size"):
            with self.subTest(change=change):
                github = FakeGitHub(self.files)
                if change == "missing":
                    github.release["assets"].pop()
                else:
                    github.release["assets"][0][change] = "wrong"
                with self.assertRaises(ValueError):
                    publish(github, 42, "1.2.3", self.directory, self.files)
                self.assertFalse(github.promoted)

    def test_an_older_build_cannot_replace_newer_latest(self):
        github = FakeGitHub(self.files)
        github.latest = {"id": 50, "tag_name": "v1.3.0"}
        with self.assertRaisesRegex(ValueError, "newer release"):
            publish(github, 42, "1.2.3", self.directory, self.files)
        self.assertFalse(github.promoted)

    def test_stable_release_is_not_modified_before_prerelease_gate(self):
        github = FakeGitHub(self.files)
        github.release["prerelease"] = False
        with self.assertRaisesRegex(ValueError, "prerelease"):
            publish(github, 42, "1.2.3", self.directory, self.files)
        self.assertFalse(github.uploaded)

    def test_version_is_stable_and_compared_numerically(self):
        self.assertGreater(stable_version("1.10.0"), stable_version("1.9.0"))
        with self.assertRaises(ValueError):
            stable_version("1.2.3-rc.1")


if __name__ == "__main__":
    unittest.main()
