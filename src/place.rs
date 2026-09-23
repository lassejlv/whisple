//! Keep the voice window pinned to the bottom center, and clip it to a
//! rounded shape. Lavapipe does not composite transparent windows, so the
//! X shape extension is what makes the corners disappear.

use x11rb::connection::Connection;
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{
    AtomEnum, ConfigureWindowAux, ConnectionExt as _, Rectangle, Window,
};
use x11rb::rust_connection::RustConnection;

static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn dock(width: f32, height: f32, screen_x: f32, screen_y: f32, screen_w: f32, screen_h: f32) {
    let Ok(_guard) = LOCK.lock() else {
        return;
    };
    let radius = if height <= 88.0 { height / 2.0 } else { 22.0 };
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

pub fn set_mapped(mapped: bool) {
    let Ok(_guard) = LOCK.lock() else {
        return;
    };
    if let Err(err) = map_client(mapped) {
        eprintln!("could not change the voice window: {err}");
    }
}

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

fn window_pid(conn: &impl Connection, window: Window, pid_atom: u32) -> Option<u32> {
    let reply = conn
        .get_property(false, window, pid_atom, AtomEnum::CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    let bytes: [u8; 4] = reply.value.get(..4)?.try_into().ok()?;
    Some(u32::from_ne_bytes(bytes))
}

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

fn circle_inset(radius: u16, row: u16) -> u16 {
    let r = radius as f32;
    let y = r - row as f32 - 0.5;
    let inside = (r * r - y * y).max(0.0).sqrt();
    (r - inside).ceil().clamp(0.0, r) as u16
}
