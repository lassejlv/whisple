pub fn is_allowed() -> bool {
    #[cfg(target_os = "macos")]
    return super::macos::microphone::is_allowed();
    #[cfg(not(target_os = "macos"))]
    false
}

pub fn request() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return super::macos::microphone::request();
    #[cfg(not(target_os = "macos"))]
    crate::audio::Mic::start("").map(drop)
}
