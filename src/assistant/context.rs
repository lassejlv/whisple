const MAX_SELECTION: usize = 4_000;
#[cfg_attr(target_os = "macos", allow(dead_code))]
const MAX_WIDTH: u32 = 1440;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub app: Option<String>,
    pub window: Option<String>,
    pub selection: Option<String>,
}

impl Snapshot {
    pub fn is_empty(&self) -> bool {
        self.app.is_none() && self.window.is_none() && self.selection.is_none()
    }
}

pub fn focused() -> Snapshot {
    #[cfg(target_os = "macos")]
    let snapshot = macos::focused();
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    let snapshot = x11::focused();
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "freebsd")))]
    let snapshot = Snapshot::default();
    tidy(snapshot)
}

/// Fills in what can be read after the bar has focus. On X11 the selection
/// is still owned by the other app, so it is read here, off the UI thread.
pub fn complete(snapshot: &mut Snapshot) {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    if snapshot.selection.is_none() {
        snapshot.selection = x11::selection();
    }
    *snapshot = tidy(std::mem::take(snapshot));
}

pub fn capture_png() -> Result<Vec<u8>, String> {
    #[cfg(target_os = "macos")]
    {
        macos::capture_png()
    }
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        x11::capture_png()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "freebsd")))]
    {
        Err("Screen capture is not available on this system.".into())
    }
}

fn tidy(snapshot: Snapshot) -> Snapshot {
    let clean = |value: Option<String>| {
        value
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    Snapshot {
        app: clean(snapshot.app),
        window: clean(snapshot.window),
        selection: clean(snapshot.selection).map(|text| truncate(&text, MAX_SELECTION)),
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

/// Shrinks packed RGB pixels to at most `max_width` wide, averaging each
/// source block so small text stays legible.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn fit(rgb: &[u8], width: u32, height: u32, max_width: u32) -> (Vec<u8>, u32, u32) {
    if width <= max_width || width == 0 || height == 0 {
        return (rgb.to_vec(), width, height);
    }
    let out_w = max_width;
    let out_h = ((u64::from(height) * u64::from(out_w)) / u64::from(width)).max(1) as u32;
    let mut out = Vec::with_capacity((out_w * out_h * 3) as usize);
    for y in 0..out_h {
        let y0 = y * height / out_h;
        let y1 = ((y + 1) * height / out_h).max(y0 + 1);
        for x in 0..out_w {
            let x0 = x * width / out_w;
            let x1 = ((x + 1) * width / out_w).max(x0 + 1);
            let mut sum = [0u32; 3];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let at = ((sy * width + sx) * 3) as usize;
                    for channel in 0..3 {
                        sum[channel] += u32::from(rgb[at + channel]);
                    }
                }
            }
            let count = (y1 - y0) * (x1 - x0);
            out.extend(sum.map(|total| (total / count) as u8));
        }
    }
    (out, out_w, out_h)
}

#[cfg(any(target_os = "linux", target_os = "freebsd", test))]
fn encode_png(rgb: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder
            .write_header()
            .map_err(|err| format!("Could not encode the screenshot: {err}"))?;
        writer
            .write_image_data(rgb)
            .map_err(|err| format!("Could not encode the screenshot: {err}"))?;
    }
    Ok(bytes)
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use std::process::Command;

    use cocoa::base::{id, nil};
    use objc::{class, msg_send, sel, sel_impl};

    use super::Snapshot;
    use crate::platform::macos::dictation;

    pub fn focused() -> Snapshot {
        let (pid, app) = frontmost();
        if pid == 0 || pid == std::process::id() as i32 {
            return Snapshot::default();
        }
        let mut snapshot = Snapshot {
            app,
            ..Snapshot::default()
        };
        if !dictation::is_trusted() {
            return snapshot;
        }
        let Some(application) = dictation::application(pid) else {
            return snapshot;
        };
        snapshot.window = dictation::attribute(application.0, "AXFocusedWindow")
            .and_then(|window| dictation::attribute(window.0, "AXTitle"))
            .and_then(|title| dictation::string_value(&title));
        snapshot.selection = dictation::attribute(application.0, "AXFocusedUIElement")
            .and_then(|element| dictation::attribute(element.0, "AXSelectedText"))
            .and_then(|text| dictation::string_value(&text));
        snapshot
    }

    fn frontmost() -> (i32, Option<String>) {
        unsafe {
            let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
            let app: id = msg_send![workspace, frontmostApplication];
            if app == nil {
                return (0, None);
            }
            let pid: i32 = msg_send![app, processIdentifier];
            let name: id = msg_send![app, localizedName];
            if name == nil {
                return (pid, None);
            }
            let bytes: *const c_char = msg_send![name, UTF8String];
            let name =
                (!bytes.is_null()).then(|| CStr::from_ptr(bytes).to_string_lossy().into_owned());
            (pid, name)
        }
    }

    pub fn capture_png() -> Result<Vec<u8>, String> {
        let dir = tempfile::tempdir()
            .map_err(|err| format!("Could not prepare the screenshot: {err}"))?;
        let path = dir.path().join("screen.png");
        // -x: no sound, -m: the main display only.
        let status = Command::new("/usr/sbin/screencapture")
            .args(["-x", "-m", "-t", "png"])
            .arg(&path)
            .status()
            .map_err(|err| format!("Could not capture the screen: {err}"))?;
        if !status.success() || !path.exists() {
            return Err(
                "Could not capture the screen. Allow Whisple under Privacy & Security › Screen Recording."
                    .into(),
            );
        }
        let _ = Command::new("/usr/bin/sips")
            .args(["-Z", &super::MAX_WIDTH.to_string()])
            .arg(&path)
            .output();
        std::fs::read(&path).map_err(|err| format!("Could not read the screenshot: {err}"))
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod x11 {
    use std::time::{Duration, Instant};

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, ImageFormat, ImageOrder,
        WindowClass,
    };
    use x11rb::protocol::Event;
    use x11rb::rust_connection::RustConnection;

    use super::{encode_png, fit, Snapshot, MAX_WIDTH};

    fn atom(conn: &RustConnection, name: &[u8]) -> Option<u32> {
        Some(conn.intern_atom(false, name).ok()?.reply().ok()?.atom)
    }

    fn property(conn: &RustConnection, window: u32, name: u32, kind: u32) -> Option<Vec<u8>> {
        let reply = conn
            .get_property(false, window, name, kind, 0, 1 << 16)
            .ok()?
            .reply()
            .ok()?;
        (reply.format != 0 && !reply.value.is_empty()).then_some(reply.value)
    }

    fn cardinal(conn: &RustConnection, window: u32, name: u32, kind: u32) -> Option<u32> {
        let value = property(conn, window, name, kind)?;
        Some(u32::from_ne_bytes(value.get(..4)?.try_into().ok()?))
    }

    pub fn focused() -> Snapshot {
        let Ok((conn, screen)) = x11rb::connect(None) else {
            return Snapshot::default();
        };
        let root = conn.setup().roots[screen].root;
        let active = atom(&conn, b"_NET_ACTIVE_WINDOW")
            .and_then(|active| cardinal(&conn, root, active, AtomEnum::WINDOW.into()))
            .filter(|window| *window != 0)
            .or_else(|| {
                // Without a window manager there is no _NET_ACTIVE_WINDOW;
                // fall back to the window holding keyboard focus, or the one
                // under the pointer when focus follows it (PointerRoot).
                let focus = conn.get_input_focus().ok()?.reply().ok()?.focus;
                if focus > 1 {
                    return Some(focus);
                }
                let child = conn.query_pointer(root).ok()?.reply().ok()?.child;
                (child != 0).then_some(child)
            });
        let Some(window) = active else {
            return Snapshot::default();
        };
        let Some(window) = titled(&conn, window, root) else {
            return Snapshot::default();
        };
        let pid = atom(&conn, b"_NET_WM_PID")
            .and_then(|pid| cardinal(&conn, window, pid, AtomEnum::CARDINAL.into()));
        if pid == Some(std::process::id()) {
            return Snapshot::default();
        }
        Snapshot {
            app: class(&conn, window).or_else(|| pid.and_then(process_name)),
            window: title(&conn, window),
            selection: None,
        }
    }

    fn titled(conn: &RustConnection, mut window: u32, root: u32) -> Option<u32> {
        for _ in 0..8 {
            if title(conn, window).is_some() || class(conn, window).is_some() {
                return Some(window);
            }
            let parent = conn.query_tree(window).ok()?.reply().ok()?.parent;
            if parent == root || parent == 0 {
                return None;
            }
            window = parent;
        }
        None
    }

    fn title(conn: &RustConnection, window: u32) -> Option<String> {
        let utf8 = atom(conn, b"UTF8_STRING");
        let net_name = atom(conn, b"_NET_WM_NAME");
        if let (Some(utf8), Some(net_name)) = (utf8, net_name) {
            if let Some(value) = property(conn, window, net_name, utf8) {
                return Some(String::from_utf8_lossy(&value).into_owned());
            }
        }
        property(
            conn,
            window,
            AtomEnum::WM_NAME.into(),
            AtomEnum::STRING.into(),
        )
        .map(|value| value.iter().map(|&byte| char::from(byte)).collect())
    }

    fn class(conn: &RustConnection, window: u32) -> Option<String> {
        let value = property(
            conn,
            window,
            AtomEnum::WM_CLASS.into(),
            AtomEnum::STRING.into(),
        )?;
        let mut parts = value
            .split(|&byte| byte == 0)
            .filter(|part| !part.is_empty());
        let instance = parts.next();
        let class = parts.next().or(instance)?;
        Some(String::from_utf8_lossy(class).into_owned())
    }

    fn process_name(pid: u32) -> Option<String> {
        std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .ok()
            .map(|name| name.trim().to_string())
    }

    pub fn selection() -> Option<String> {
        let (conn, screen) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots[screen].root;
        let owner = conn
            .get_selection_owner(AtomEnum::PRIMARY.into())
            .ok()?
            .reply()
            .ok()?
            .owner;
        if owner == 0 {
            return None;
        }
        let utf8 = atom(&conn, b"UTF8_STRING")?;
        let target = atom(&conn, b"WHISPLE_SELECTION")?;
        let window = conn.generate_id().ok()?;
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .ok()?;
        let text = request_selection(&conn, window, utf8, target);
        let _ = conn.destroy_window(window);
        let _ = conn.flush();
        text
    }

    fn request_selection(
        conn: &RustConnection,
        window: u32,
        utf8: u32,
        target: u32,
    ) -> Option<String> {
        conn.convert_selection(
            window,
            AtomEnum::PRIMARY.into(),
            utf8,
            target,
            x11rb::CURRENT_TIME,
        )
        .ok()?;
        conn.flush().ok()?;
        let deadline = Instant::now() + Duration::from_millis(400);
        while Instant::now() < deadline {
            match conn.poll_for_event().ok()? {
                Some(Event::SelectionNotify(event)) if event.requestor == window => {
                    if event.property == 0 {
                        return None;
                    }
                    let reply = conn
                        .get_property(true, window, target, AtomEnum::ANY, 0, 1 << 20)
                        .ok()?
                        .reply()
                        .ok()?;
                    // A large selection arrives in INCR chunks; the screenshot
                    // covers it instead.
                    if reply.type_ != utf8 && reply.type_ != u32::from(AtomEnum::STRING) {
                        return None;
                    }
                    return Some(String::from_utf8_lossy(&reply.value).into_owned());
                }
                Some(_) => {}
                None => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        None
    }

    pub fn capture_png() -> Result<Vec<u8>, String> {
        let (conn, screen) =
            x11rb::connect(None).map_err(|_| "Screen capture needs an X11 session.".to_string())?;
        let setup = conn.setup();
        let root = &setup.roots[screen];
        let (width, height) = (root.width_in_pixels, root.height_in_pixels);
        let image = conn
            .get_image(ImageFormat::Z_PIXMAP, root.root, 0, 0, width, height, !0)
            .map_err(|err| format!("Could not capture the screen: {err}"))?
            .reply()
            .map_err(|err| format!("Could not capture the screen: {err}"))?;
        let format = setup
            .pixmap_formats
            .iter()
            .find(|format| format.depth == image.depth)
            .ok_or("The screen uses an unknown pixel format.")?;
        if format.bits_per_pixel != 32 || image.depth < 24 {
            return Err(format!(
                "Screens with {}-bit pixels cannot be captured yet.",
                format.bits_per_pixel
            ));
        }
        let pad = u32::from(format.scanline_pad.max(8));
        let stride = (u32::from(width) * 32).div_ceil(pad) * pad / 8;
        let lsb = setup.image_byte_order == ImageOrder::LSB_FIRST;
        let rgb = to_rgb(
            &image.data,
            u32::from(width),
            u32::from(height),
            stride,
            lsb,
        )?;
        let (rgb, width, height) = fit(&rgb, u32::from(width), u32::from(height), MAX_WIDTH);
        encode_png(&rgb, width, height)
    }

    pub(super) fn to_rgb(
        data: &[u8],
        width: u32,
        height: u32,
        stride: u32,
        lsb: bool,
    ) -> Result<Vec<u8>, String> {
        if data.len() < (stride * height) as usize {
            return Err("The screenshot came back incomplete.".into());
        }
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            let row = &data[(y * stride) as usize..];
            for x in 0..width {
                let pixel = &row[(x * 4) as usize..(x * 4 + 4) as usize];
                if lsb {
                    rgb.extend([pixel[2], pixel[1], pixel[0]]);
                } else {
                    rgb.extend([pixel[1], pixel[2], pixel[3]]);
                }
            }
        }
        Ok(rgb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_fields_and_long_selections_are_tidied() {
        let snapshot = tidy(Snapshot {
            app: Some("  Safari ".into()),
            window: Some("   ".into()),
            selection: Some("é".repeat(MAX_SELECTION + 10)),
        });
        assert_eq!(snapshot.app.as_deref(), Some("Safari"));
        assert_eq!(snapshot.window, None);
        let selection = snapshot.selection.unwrap();
        assert_eq!(selection.chars().count(), MAX_SELECTION + 1);
        assert!(selection.ends_with('…'));
        assert!(Snapshot::default().is_empty());
    }

    #[test]
    fn wide_screens_are_averaged_down() {
        let rgb = [0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 255];
        let (out, width, height) = fit(&rgb, 4, 1, 2);
        assert_eq!((width, height), (2, 1));
        assert_eq!(out, vec![127, 127, 127, 127, 0, 127]);
        let (same, width, _) = fit(&rgb, 4, 1, 8);
        assert_eq!(width, 4);
        assert_eq!(same, rgb);
    }

    #[test]
    fn screenshots_encode_as_png() {
        let rgb = vec![200u8; 3 * 3 * 2];
        let bytes = encode_png(&rgb, 3, 2).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let reader = decoder.read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), (3, 2));
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    #[test]
    fn x_pixels_become_rgb() {
        let data = [1, 2, 3, 0, 4, 5, 6, 0, 9, 9, 9, 9];
        assert_eq!(
            x11::to_rgb(&data, 2, 1, 12, true).unwrap(),
            vec![3, 2, 1, 6, 5, 4]
        );
        assert!(x11::to_rgb(&data, 2, 2, 12, true).is_err());
    }
}
