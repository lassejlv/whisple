use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};

const MARGIN: f32 = 18.0;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const SPI_GETWORKAREA: u32 = 0x0030;
const SW_HIDE: i32 = 0;
const SW_SHOWNOACTIVATE: i32 = 4;
const GWL_EXSTYLE: i32 = -20;
const WS_EX_TOOLWINDOW: i32 = 0x0080;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWA_BORDER_COLOR: u32 = 34;
const DWMWCP_DONOTROUND: u32 = 1;
const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;

/// The voice window whose system frame was last removed.
static UNFRAMED: AtomicIsize = AtomicIsize::new(0);

#[repr(C)]
#[derive(Clone, Copy)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[link(name = "user32")]
extern "system" {
    fn EnumWindows(callback: extern "system" fn(isize, isize) -> i32, lparam: isize) -> i32;
    fn GetWindowThreadProcessId(hwnd: isize, process_id: *mut u32) -> u32;
    fn GetClassNameW(hwnd: isize, name: *mut u16, capacity: i32) -> i32;
    fn ShowWindow(hwnd: isize, command: i32) -> i32;
    fn GetWindowRect(hwnd: isize, rect: *mut Rect) -> i32;
    fn GetClientRect(hwnd: isize, rect: *mut Rect) -> i32;
    fn GetDpiForWindow(hwnd: isize) -> u32;
    fn SetWindowPos(
        hwnd: isize,
        insert_after: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
    fn SystemParametersInfoW(action: u32, param: u32, pv: *mut c_void, winini: u32) -> i32;
    fn GetWindowLongW(hwnd: isize, index: i32) -> i32;
}

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(hwnd: isize, attribute: u32, value: *const c_void, size: u32) -> i32;
}

pub fn anchor(width: f32, height: f32) {
    let Some(hwnd) = voice_window() else {
        return;
    };
    unframe(hwnd);
    unsafe {
        let mut window = Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut client = window;
        if GetWindowRect(hwnd, &mut window) == 0 || GetClientRect(hwnd, &mut client) == 0 {
            return;
        }
        let mut work = window;
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut Rect as *mut c_void, 0) == 0
        {
            return;
        }
        let dpi = GetDpiForWindow(hwnd).max(96) as f32;
        let scale = dpi / 96.0;
        let extra_w = (window.right - window.left) - (client.right - client.left);
        let extra_h = (window.bottom - window.top) - (client.bottom - client.top);
        let outer_w = (width * scale).round() as i32 + extra_w;
        let outer_h = (height * scale).round() as i32 + extra_h;
        let margin = (MARGIN * scale).round() as i32;
        let left = work.left + (work.right - work.left - outer_w) / 2;
        let top = work.bottom - margin - outer_h;
        if (window.left - left).abs() < 2
            && (window.top - top).abs() < 2
            && (window.right - window.left - outer_w).abs() < 2
            && (window.bottom - window.top - outer_h).abs() < 2
        {
            return;
        }
        SetWindowPos(
            hwnd,
            0,
            left,
            top,
            outer_w,
            outer_h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Shows or hides the voice window. GPUI can only activate a window, so
/// hiding the bar needs the system call.
pub fn set_mapped(mapped: bool) {
    if let Some(hwnd) = voice_window() {
        unsafe {
            ShowWindow(hwnd, if mapped { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }
}

/// Windows 11 rounds and outlines every top-level window, even a borderless
/// popup, which shows as a grey frame around the transparent voice window.
fn unframe(hwnd: isize) {
    if UNFRAMED.swap(hwnd, Ordering::Relaxed) == hwnd {
        return;
    }
    unsafe {
        // Older Windows versions reject these attributes, which is fine:
        // they draw neither the rounded corners nor the outline.
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &DWMWCP_DONOTROUND as *const u32 as *const c_void,
            size_of::<u32>() as u32,
        );
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &DWMWA_COLOR_NONE as *const u32 as *const c_void,
            size_of::<u32>() as u32,
        );
    }
}

/// The voice bar: the GPUI window opened as a popup, which Windows makes a
/// tool window. Settings and onboarding are normal windows, and the tray and
/// shortcut helpers are not GPUI windows. The bar may be hidden.
fn voice_window() -> Option<isize> {
    let mut hwnd = 0isize;
    unsafe {
        EnumWindows(find_voice_window, &mut hwnd as *mut isize as isize);
    }
    (hwnd != 0).then_some(hwnd)
}

extern "system" fn find_voice_window(hwnd: isize, lparam: isize) -> i32 {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid != std::process::id() || GetWindowLongW(hwnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW == 0 {
            return 1;
        }
        let mut class = [0u16; 32];
        let len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        if String::from_utf16_lossy(&class[..len.max(0) as usize]) == "Zed::Window" {
            *(lparam as *mut isize) = hwnd;
            return 0;
        }
    }
    1
}
