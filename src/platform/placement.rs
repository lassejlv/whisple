/// Sizes and the screen rectangle are in logical pixels; `scale` is the
/// window's device pixel ratio.
pub fn dock(
    width: f32,
    height: f32,
    screen_x: f32,
    screen_y: f32,
    screen_w: f32,
    screen_h: f32,
    scale: f32,
) {
    #[cfg(target_os = "macos")]
    super::macos::placement::anchor(width, height);
    #[cfg(target_os = "windows")]
    super::windows::placement::anchor(width, height);
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    super::linux::placement::dock(width, height, screen_x, screen_y, screen_w, screen_h, scale);
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    let _ = (screen_x, screen_y, screen_w, screen_h, scale);
}

pub fn set_mapped(mapped: bool) {
    #[cfg(target_os = "macos")]
    super::macos::placement::set_mapped(mapped);
    #[cfg(target_os = "windows")]
    super::windows::placement::set_mapped(mapped);
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    super::linux::placement::set_mapped(mapped);
}

/// A replacement HUD has a new native window identifier.
pub(crate) fn reset() {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    super::linux::placement::reset();
}
