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
static SESSION: std::sync::Mutex<Option<Session>> = std::sync::Mutex::new(None);

type Geometry = (i32, i32, u16, u16, u16);
struct Session {
    conn: RustConnection,
    client: Window,
    top: Window,
    geometry: Option<Geometry>,
}

pub(crate) fn reset() {
    if let Ok(mut session) = SESSION.lock() {
        *session = None;
    }
}

fn with_session<T>(operation: impl FnOnce(&mut Session) -> Result<T, String>) -> Result<T, String> {
    let mut cache = SESSION.lock().map_err(|err| err.to_string())?;
    if cache.is_none() {
        *cache = Some(locate()?);
    }
    let result = operation(cache.as_mut().expect("located window"));
    if result.is_err() {
        *cache = None;
    }
    result
}

pub fn dock(
    width: f32,
    height: f32,
    screen_x: f32,
    screen_y: f32,
    screen_w: f32,
    screen_h: f32,
    scale: f32,
) {
    // GPUI lays out in logical pixels, but X11 geometry and the shape mask
    // are in device pixels.
    let (x, y, width, height, radius) = device_geometry(
        width,
        height,
        (screen_x, screen_y, screen_w, screen_h),
        scale,
    );
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

/// Bottom-centre placement in device pixels: x, y, width, height and the
/// corner radius.
fn device_geometry(
    width: f32,
    height: f32,
    (screen_x, screen_y, screen_w, screen_h): (f32, f32, f32, f32),
    scale: f32,
) -> (f32, f32, f32, f32, f32) {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let x = screen_x + (screen_w - width) / 2.0;
    let y = screen_y + screen_h - height - 18.0;
    (
        x * scale,
        y * scale,
        width * scale,
        height * scale,
        crate::app::WINDOW_RADIUS * scale,
    )
}

pub fn set_mapped(mapped: bool) {
    if let Err(err) = map_client(mapped) {
        eprintln!("could not change the voice window: {err}");
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn map_client(mapped: bool) -> Result<(), String> {
    with_session(|session| {
        if mapped {
            session
                .conn
                .map_window(session.top)
                .map_err(|err| err.to_string())?
                .check()
                .map_err(|err| err.to_string())?;
        } else {
            session
                .conn
                .unmap_window(session.top)
                .map_err(|err| err.to_string())?
                .check()
                .map_err(|err| err.to_string())?;
        }
        session.conn.flush().map_err(|err| err.to_string())
    })
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn place(x: i32, y: i32, width: u16, height: u16, radius: u16) -> Result<(), String> {
    with_session(|session| {
        let wanted = (x, y, width, height, radius);
        let conn = &session.conn;
        if session.geometry == Some(wanted) {
            // A bounds/work-area event can mean another client moved the window.
            // Validate it only on events, using the already-open connection.
            let actual = conn
                .get_geometry(session.top)
                .map_err(|err| err.to_string())?
                .reply()
                .map_err(|err| err.to_string())?;
            if (
                i32::from(actual.x),
                i32::from(actual.y),
                actual.width,
                actual.height,
            ) == (x, y, width, height)
            {
                return Ok(());
            }
        }
        conn.configure_window(
            session.top,
            &ConfigureWindowAux::new()
                .x(x)
                .y(y)
                .width(width as u32)
                .height(height as u32),
        )
        .map_err(|err| err.to_string())?
        .check()
        .map_err(|err| err.to_string())?;
        if session.geometry.map(|(_, _, w, h, r)| (w, h, r)) != Some((width, height, radius)) {
            let rectangles = rounded_rects(width, height, radius.min(width / 2).min(height / 2));
            conn.shape_rectangles(
                SO::SET,
                SK::BOUNDING,
                x11rb::protocol::xproto::ClipOrdering::UNSORTED,
                session.client,
                0,
                0,
                &rectangles,
            )
            .map_err(|err| err.to_string())?
            .check()
            .map_err(|err| err.to_string())?;
        }
        conn.flush().map_err(|err| err.to_string())?;
        session.geometry = Some(wanted);
        Ok(())
    })
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn locate() -> Result<Session, String> {
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
    Ok(Session {
        conn,
        client,
        top,
        geometry: None,
    })
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
    if window_pid(conn, window, pid_atom) == Some(pid) && is_voice_window(conn, window) {
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
fn is_voice_window(conn: &impl Connection, window: Window) -> bool {
    conn.get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 64)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|reply| is_voice_class(&reply.value))
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn is_voice_class(value: &[u8]) -> bool {
    value.split(|&byte| byte == 0).next() == Some(b"whisple".as_slice())
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

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn circle_inset(radius: u16, row: u16) -> u16 {
    let r = radius as f32;
    let y = r - row as f32 - 0.5;
    let inside = (r * r - y * y).max(0.0).sqrt();
    (r - inside).ceil().clamp(0.0, r) as u16
}

#[cfg(all(test, any(target_os = "linux", target_os = "freebsd")))]
mod tests {
    use super::{device_geometry, is_voice_class};

    #[test]
    fn only_the_bar_is_docked() {
        assert!(is_voice_class(b"whisple\0whisple\0"));
        assert!(!is_voice_class(b"whisple-settings\0whisple-settings\0"));
        assert!(!is_voice_class(b"whisple-onboarding\0whisple-onboarding\0"));
        assert!(!is_voice_class(b""));
    }

    #[test]
    fn hidpi_placement_is_in_device_pixels() {
        let screen = (0.0, 0.0, 960.0, 600.0);
        let (x, y, width, height, radius) = device_geometry(400.0, 58.0, screen, 2.0);
        assert_eq!((x, y, width, height), (560.0, 1048.0, 800.0, 116.0));
        assert_eq!(radius, crate::app::WINDOW_RADIUS * 2.0);
    }

    #[test]
    fn unusable_scale_falls_back_to_logical_pixels() {
        let screen = (0.0, 0.0, 1920.0, 1200.0);
        let at_one = device_geometry(400.0, 58.0, screen, 1.0);
        assert_eq!(device_geometry(400.0, 58.0, screen, 0.0), at_one);
        assert_eq!(device_geometry(400.0, 58.0, screen, f32::NAN), at_one);
    }
}
