//! Marks the platforms that update themselves from GitHub releases with the
//! `updates` cfg, and gives the Windows executable its icon and version
//! details. The Windows updater reads `WhispleSemanticVersion` back to check a
//! download before installing it.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(updates)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os == "macos" || os == "windows" {
        println!("cargo:rustc-cfg=updates");
    }
    if os == "windows" {
        windows_resources();
    }
}

#[cfg(windows)]
fn windows_resources() {
    use std::path::PathBuf;

    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = root.join("assets").join("whisple.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    // The numeric version fields hold only major, minor and patch; the full
    // semantic version, prerelease included, goes in the strings.
    let mut numbers = version
        .split(['.', '-', '+'])
        .map(|part| part.parse::<u16>().unwrap_or(0));
    let [major, minor, patch] = [(); 3].map(|_| numbers.next().unwrap_or(0));
    let rc = format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Whisple"
            VALUE "FileDescription", "Whisple"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "whisple"
            VALUE "LegalCopyright", "MIT License"
            VALUE "OriginalFilename", "whisple.exe"
            VALUE "ProductName", "Whisple"
            VALUE "ProductVersion", "{version}"
            VALUE "WhispleSemanticVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#,
        icon = icon.display().to_string().replace('\\', "/"),
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("whisple.rc");
    std::fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}

/// Cross builds for Windows go without the icon and version details, and
/// cannot update themselves.
#[cfg(not(windows))]
fn windows_resources() {}
