//! Keep the voice window pinned to the bottom center, and clip it to a
//! rounded shape. Lavapipe does not composite transparent windows, so the
//! X shape extension is what makes the corners disappear.

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use x11rb::connection::Connection;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use x11rb::protocol::xproto::{
    AtomEnum, ConfigureWindowAux, ConnectionExt as _, Rectangle, Window,
};
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use x11rb::rust_connection::RustConnection;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn dock(width: f32, height: f32, screen_x: f32, screen_y: f32, screen_w: f32, screen_h: f32) {
    // AppKit's setContentSize and Win32's SWP_NOMOVE keep the top-left fixed,
    // so a taller panel grows off the bottom of the screen. Hold the bottom
    // edge ourselves. Linux already does that with the X configure below.
    #[cfg(target_os = "macos")]
    macos::anchor(width, height);
    #[cfg(target_os = "windows")]
    windows::anchor(width, height);
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    let _ = (screen_x, screen_y, screen_w, screen_h);

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        let Ok(_guard) = LOCK.lock() else {
            return;
        };
        let radius = crate::app::WINDOW_RADIUS;
        let x = screen_x + (screen_w - width) / 2.0;
        let y = screen_y + screen_h - height - 18.0;
        if let Err(err) = place(
            x.round() as i32,
            y.round() as i32,
            width.round() as u16,
            height.round() as u16,
            radius.round() as u16,
        ) {
            eprintln!("could not place the voice window: {err}");
        }
    }
}

pub fn set_mapped(mapped: bool) {
    #[cfg(target_os = "macos")]
    macos::set_mapped(mapped);
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    let _ = mapped;
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        let Ok(_guard) = LOCK.lock() else {
            return;
        };
        if let Err(err) = map_client(mapped) {
            eprintln!("could not change the voice window: {err}");
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn map_client(mapped: bool) -> Result<(), String> {
    let (conn, _client, top) = locate()?;
    if mapped {
        conn.map_window(top).map_err(|err| err.to_string())?;
    } else {
        conn.unmap_window(top).map_err(|err| err.to_string())?;
    }
    conn.flush().map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn place(x: i32, y: i32, width: u16, height: u16, radius: u16) -> Result<(), String> {
    let (conn, client, top) = locate()?;

    conn.configure_window(
        top,
        &ConfigureWindowAux::new()
            .x(x)
            .y(y)
            .width(width as u32)
            .height(height as u32),
    )
    .map_err(|err| err.to_string())?;

    let rectangles = rounded_rects(width, height, radius.min(width / 2).min(height / 2));
    conn.shape_rectangles(
        SO::SET,
        SK::BOUNDING,
        x11rb::protocol::xproto::ClipOrdering::UNSORTED,
        client,
        0,
        0,
        &rectangles,
    )
    .map_err(|err| err.to_string())?;
    conn.flush().map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn locate() -> Result<(RustConnection, Window, Window), String> {
    let (conn, screen) = x11rb::connect(None).map_err(|err| err.to_string())?;
    let root = conn.setup().roots[screen].root;
    let pid_atom = conn
        .intern_atom(false, b"_NET_WM_PID")
        .map_err(|err| err.to_string())?
        .reply()
        .map_err(|err| err.to_string())?
        .atom;
    let client =
        find_pid(&conn, root, pid_atom, std::process::id(), 0).ok_or("voice window not found")?;
    let top = top_level(&conn, client, root).unwrap_or(client);
    Ok((conn, client, top))
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn find_pid(
    conn: &impl Connection,
    window: Window,
    pid_atom: u32,
    pid: u32,
    depth: usize,
) -> Option<Window> {
    if depth > 8 {
        return None;
    }
    if window_pid(conn, window, pid_atom) == Some(pid) {
        return Some(window);
    }
    let tree = conn.query_tree(window).ok()?.reply().ok()?;
    for child in tree.children {
        if let Some(found) = find_pid(conn, child, pid_atom, pid, depth + 1) {
            return Some(found);
        }
    }
    None
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn window_pid(conn: &impl Connection, window: Window, pid_atom: u32) -> Option<u32> {
    let reply = conn
        .get_property(false, window, pid_atom, AtomEnum::CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    let bytes: [u8; 4] = reply.value.get(..4)?.try_into().ok()?;
    Some(u32::from_ne_bytes(bytes))
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn top_level(conn: &impl Connection, mut window: Window, root: Window) -> Option<Window> {
    for _ in 0..8 {
        let tree = conn.query_tree(window).ok()?.reply().ok()?;
        if tree.parent == root || tree.parent == 0 {
            return Some(window);
        }
        window = tree.parent;
    }
    Some(window)
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn rounded_rects(width: u16, height: u16, radius: u16) -> Vec<Rectangle> {
    if radius == 0 {
        return vec![Rectangle {
            x: 0,
            y: 0,
            width,
            height,
        }];
    }
    let mut rects = Vec::with_capacity(radius as usize * 2 + 2);
    rects.push(Rectangle {
        x: radius as i16,
        y: 0,
        width: width.saturating_sub(radius * 2),
        height,
    });
    rects.push(Rectangle {
        x: 0,
        y: radius as i16,
        width,
        height: height.saturating_sub(radius * 2),
    });
    for row in 0..radius {
        let inset = circle_inset(radius, row);
        let span = width.saturating_sub(inset * 2);
        rects.push(Rectangle {
            x: inset as i16,
            y: row as i16,
            width: span,
            height: 1,
        });
        rects.push(Rectangle {
            x: inset as i16,
            y: height as i16 - 1 - row as i16,
            width: span,
            height: 1,
        });
    }
    rects
}

#[cfg(target_os = "macos")]
mod macos {
    use cocoa::appkit::{
        NSApplication, NSColor, NSView, NSViewLayerContentsPlacement, NSWindow, NSWindowStyleMask,
    };
    use cocoa::base::{id, nil, NO};
    use cocoa::foundation::{NSPoint, NSRect, NSSize};
    use objc::{class, msg_send, sel, sel_impl};

    const MARGIN: f64 = 18.0;
    /// `NSWindowAnimationBehaviorNone`. Popups default to utility-window
    /// animation, which eases the frame and leaves the panel clipped.
    const ANIMATION_NONE: isize = 2;

    pub fn set_mapped(mapped: bool) {
        unsafe {
            let window = hud_window();
            if window.is_null() {
                return;
            }
            if mapped {
                let _: () = msg_send![window, orderFront: nil];
            } else {
                let _: () = msg_send![window, orderOut: nil];
            }
        }
    }

    pub fn anchor(width: f32, height: f32) {
        unsafe {
            let window = hud_window();
            if window.is_null() || window.isVisible() == NO {
                return;
            }
            let view = metal_view(window);
            // GPUI creates a titled window even without a titlebar, with a
            // faint background to support AppKit's shadow. That native frame
            // outlines the empty space while our HUD closes inside it. Whisp
            // paints its own rounded border, so the host must be truly clear
            // and borderless. Apply this once, before anchoring the content.
            let style = window.styleMask();
            if style.contains(NSWindowStyleMask::NSTitledWindowMask) {
                let was_key = window.isKeyWindow() != NO;
                let responder = window.firstResponder();
                window.setStyleMask_(style & !NSWindowStyleMask::NSTitledWindowMask);
                window.setHasShadow_(NO);
                window.setBackgroundColor_(NSColor::clearColor(nil));
                // AppKit resets keyboard focus when its style changes.
                if was_key {
                    window.makeKeyWindow();
                }
                if !responder.is_null() {
                    window.makeFirstResponder_(responder);
                }
            }
            let _: () = msg_send![window, setAnimationBehavior: ANIMATION_NONE];
            anchor_drawable(window, view);

            let screen: id = msg_send![window, screen];
            if screen.is_null() {
                return;
            }
            // `visibleFrame` sits above the Dock and below the menu bar.
            let visible: NSRect = msg_send![screen, visibleFrame];
            let content = NSRect {
                origin: NSPoint {
                    x: visible.origin.x + (visible.size.width - f64::from(width)) * 0.5,
                    y: visible.origin.y + MARGIN,
                },
                size: NSSize {
                    width: f64::from(width),
                    height: f64::from(height),
                },
            };
            let frame: NSRect = msg_send![window, frameRectForContentRect: content];
            let current: NSRect = msg_send![window, frame];
            if close(current.origin.x, frame.origin.x)
                && close(current.origin.y, frame.origin.y)
                && close(current.size.width, frame.size.width)
                && close(current.size.height, frame.size.height)
            {
                return;
            }
            // `display: NO` avoids a synchronous AppKit redraw while GPUI is
            // already drawing this frame. The view's resize callback and
            // `bounds_changed` pick up the new content size.
            let _: () = msg_send![window, setFrame: frame display: NO animate: NO];
        }
    }

    unsafe fn hud_window() -> id {
        let app = NSApplication::sharedApplication(nil);
        let windows: id = msg_send![app, windows];
        let count: usize = msg_send![windows, count];
        for index in 0..count {
            let window: id = msg_send![windows, objectAtIndex: index];
            let view = metal_view(window);
            if !view.is_null() && is_hud_width(NSView::frame(window.contentView()).size.width) {
                return window;
            }
        }
        nil
    }

    unsafe fn metal_view(window: id) -> id {
        let views: id = msg_send![window.contentView(), subviews];
        let count: usize = msg_send![views, count];
        for index in 0..count {
            let view: id = msg_send![views, objectAtIndex: index];
            let layer = view.layer();
            if layer.is_null() {
                continue;
            }
            let is_metal: bool = msg_send![layer, isKindOfClass: class!(CAMetalLayer)];
            if is_metal {
                return view;
            }
        }
        nil
    }

    unsafe fn anchor_drawable(window: id, view: id) {
        // Keep the last presented frame at its original size and pinned
        // to the bar while Metal prepares a drawable for the new bounds.
        // AppKit's default stretches it, producing a one-frame flash.
        let placement = NSViewLayerContentsPlacement::NSViewLayerContentsPlacementBottom;
        if view.layerContentsPlacement() != placement {
            view.setLayerContentsPlacement(placement);
        }
        let layer = view.layer();
        let scale = window.backingScaleFactor();
        let current_scale: f64 = msg_send![layer, contentsScale];
        if (current_scale - scale).abs() > f64::EPSILON {
            let _: () = msg_send![layer, setContentsScale: scale];
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.5
    }

    fn is_hud_width(width: f64) -> bool {
        close(width, f64::from(crate::app::WINDOW_WIDTH))
    }

    #[cfg(test)]
    mod tests {
        use super::is_hud_width;

        #[test]
        fn settings_window_is_not_mistaken_for_voice_bar() {
            assert!(is_hud_width(f64::from(crate::app::WINDOW_WIDTH)));
            assert!(!is_hud_width(880.0));
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
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
            if SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut Rect as *mut c_void, 0)
                == 0
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
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn circle_inset(radius: u16, row: u16) -> u16 {
    let r = radius as f32;
    let y = r - row as f32 - 0.5;
    let inside = (r * r - y * y).max(0.0).sqrt();
    (r - inside).ceil().clamp(0.0, r) as u16
}
