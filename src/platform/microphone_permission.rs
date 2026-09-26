pub fn is_allowed() -> bool {
    #[cfg(target_os = "macos")]
    return super::macos::microphone::is_allowed();
    // Windows asks nothing, so a microphone is ready unless a privacy
    // setting blocks it or none is connected.
    #[cfg(target_os = "windows")]
    return !is_blocked() && crate::audio::input_names().is_ok_and(|names| !names.is_empty());
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    false
}

/// Whether a system privacy setting is known to block the microphone. macOS
/// reports that through the error from opening it instead.
pub fn is_blocked() -> bool {
    #[cfg(target_os = "windows")]
    return super::windows::microphone_blocked();
    #[cfg(not(target_os = "windows"))]
    false
}

pub fn request() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return super::macos::microphone::request();
    #[cfg(target_os = "windows")]
    if is_blocked() {
        return Err(BLOCKED.into());
    }
    #[cfg(not(target_os = "macos"))]
    crate::audio::Mic::start("").map(drop)
}

/// The error when Windows privacy settings block the microphone. It names
/// the setting so the recovery notice recognises it.
#[cfg(target_os = "windows")]
pub const BLOCKED: &str =
    "Microphone access is denied in Windows Settings › Privacy & security › Microphone.";
