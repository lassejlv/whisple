use std::ffi::c_void;

const MARGIN: f32 = 18.0;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const SPI_GETWORKAREA: u32 = 0x0030;

#[repr(C)]
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
    fn IsWindowVisible(hwnd: isize) -> i32;
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
}

pub fn anchor(width: f32, height: f32) {
    let mut hwnd = 0isize;
    unsafe {
        EnumWindows(find_ours, &mut hwnd as *mut isize as isize);
    }
    if hwnd == 0 {
        return;
    }
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

extern "system" fn find_ours(hwnd: isize, lparam: isize) -> i32 {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == std::process::id() && IsWindowVisible(hwnd) != 0 {
            *(lparam as *mut isize) = hwnd;
            return 0;
        }
    }
    1
}
