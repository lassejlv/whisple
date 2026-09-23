//! The shortcut that shows the bar.
//!
//! A passive X grab listens even when the window is unmapped. Recording a new
//! shortcut pauses the grab so the keys reach the settings surface.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once};
use std::thread;
use std::time::Duration;

use gpui_kit::{Keystroke, Modifiers};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt as _;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    pub key: String,
}

struct Grab {
    chord: Chord,
    paused: bool,
}

static GRAB: Mutex<Grab> = Mutex::new(Grab {
    chord: Chord {
        ctrl: true,
        alt: false,
        shift: true,
        super_key: false,
        key: String::new(),
    },
    paused: false,
});
static PRESSED: AtomicBool = AtomicBool::new(false);
static STARTED: Once = Once::new();

pub fn parse(source: &str) -> Option<Chord> {
    let stroke = Keystroke::parse(source).ok()?;
    from_keystroke(&stroke)
}

pub fn from_keystroke(stroke: &Keystroke) -> Option<Chord> {
    if is_modifier_only(&stroke.key) {
        return None;
    }
    from_parts(&stroke.modifiers, &stroke.key)
}

pub fn is_modifier_only(key: &str) -> bool {
    matches!(
        key,
        "ctrl"
            | "control"
            | "alt"
            | "option"
            | "shift"
            | "super"
            | "meta"
            | "cmd"
            | "win"
            | "fn"
            | "function"
    )
}

fn from_parts(modifiers: &Modifiers, key: &str) -> Option<Chord> {
    if key.is_empty() {
        return None;
    }
    Some(Chord {
        ctrl: modifiers.control,
        alt: modifiers.alt,
        shift: modifiers.shift,
        super_key: modifiers.platform,
        key: key.to_ascii_lowercase(),
    })
}

impl Chord {
    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.alt || self.super_key
    }

    pub fn canonical(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("ctrl");
        }
        if self.alt {
            parts.push("alt");
        }
        if self.shift {
            parts.push("shift");
        }
        if self.super_key {
            parts.push("super");
        }
        parts.push(self.key.as_str());
        parts.join("-")
    }

    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.super_key {
            parts.push("Super".to_string());
        }
        parts.push(key_label(&self.key));
        parts.join(" ")
    }
}

pub fn label(source: &str) -> String {
    parse(source)
        .map(|chord| chord.label())
        .unwrap_or_else(|| source.to_string())
}

fn key_label(key: &str) -> String {
    match key {
        "space" => "Space".into(),
        "escape" => "Esc".into(),
        "return" | "enter" => "Return".into(),
        "tab" => "Tab".into(),
        "backspace" => "Delete".into(),
        "delete" => "Forward Delete".into(),
        "left" => "Left".into(),
        "right" => "Right".into(),
        "up" => "Up".into(),
        "down" => "Down".into(),
        "pageup" => "Page Up".into(),
        "pagedown" => "Page Down".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "insert" => "Insert".into(),
        other if other.len() == 1 => other.to_ascii_uppercase(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

pub fn install(chord: Chord) {
    if let Ok(mut grab) = GRAB.lock() {
        grab.chord = chord;
    }
    STARTED.call_once(|| {
        thread::Builder::new()
            .name("whisp-hotkey".into())
            .spawn(serve)
            .ok();
    });
}

pub fn set_paused(paused: bool) {
    if let Ok(mut grab) = GRAB.lock() {
        grab.paused = paused;
    }
}

pub fn take_press() -> bool {
    PRESSED.swap(false, Ordering::Relaxed)
}

fn serve() {
    loop {
        if serve_once().is_err() {
            thread::sleep(Duration::from_millis(500));
        }
    }
}

fn serve_once() -> Result<(), ()> {
    let (conn, screen_index) = x11rb::connect(None).map_err(|_| ())?;
    let screen = &conn.setup().roots[screen_index];
    let root = screen.root;
    let mut held = false;
    let mut grabbed: Option<(u8, Chord)> = None;

    loop {
        let (chord, paused) = {
            let grab = GRAB.lock().map_err(|_| ())?;
            (grab.chord.clone(), grab.paused)
        };
        let wanted = (!paused && chord.has_modifier()).then_some(chord);
        if grabbed.as_ref().map(|(_, chord)| chord) != wanted.as_ref() {
            if let Some((keycode, _)) = grabbed.take() {
                ungrab(&conn, root, keycode);
            }
            if let Some(chord) = wanted.clone() {
                if let Some(keycode) = keycode_for(&conn, &chord.key) {
                    grab_key(&conn, root, keycode, &chord);
                    grabbed = Some((keycode, chord));
                }
            }
        }

        if paused {
            held = false;
        }
        match conn.poll_for_event() {
            Ok(Some(event)) => {
                if accept_key(&conn, event, &mut held, !paused)? {
                    PRESSED.store(true, Ordering::Relaxed);
                }
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => return Err(()),
        }
    }
}

fn grab_key(conn: &impl x11rb::connection::Connection, root: u32, keycode: u8, chord: &Chord) {
    const LOCK: u16 = 2;
    const MOD2: u16 = 16;
    let base = mask(chord);
    for extra in [0, LOCK, MOD2, LOCK | MOD2] {
        let _ = conn.grab_key(
            false,
            root,
            (base | extra).into(),
            keycode,
            x11rb::protocol::xproto::GrabMode::ASYNC,
            x11rb::protocol::xproto::GrabMode::ASYNC,
        );
    }
    let _ = conn.flush();
}

fn ungrab(conn: &impl x11rb::connection::Connection, root: u32, keycode: u8) {
    let _ = conn.ungrab_key(keycode, root, x11rb::protocol::xproto::ModMask::ANY);
    let _ = conn.flush();
}

fn mask(chord: &Chord) -> u16 {
    const SHIFT: u16 = 1;
    const CONTROL: u16 = 4;
    const MOD1: u16 = 8;
    const MOD4: u16 = 64;
    let mut mask = 0;
    if chord.shift {
        mask |= SHIFT;
    }
    if chord.ctrl {
        mask |= CONTROL;
    }
    if chord.alt {
        mask |= MOD1;
    }
    if chord.super_key {
        mask |= MOD4;
    }
    mask
}

fn keycode_for(conn: &impl x11rb::connection::Connection, key: &str) -> Option<u8> {
    let keysym = keysym(key)?;
    let setup = conn.setup();
    let min = setup.min_keycode;
    let count = setup.max_keycode.saturating_sub(min).saturating_add(1);
    let reply = conn.get_keyboard_mapping(min, count).ok()?.reply().ok()?;
    let width = reply.keysyms_per_keycode as usize;
    if width == 0 {
        return None;
    }
    for (index, keys) in reply.keysyms.chunks(width).enumerate() {
        if keys.contains(&keysym) {
            return Some(min + index as u8);
        }
    }
    None
}

/// X sends a key release immediately before the repeated press. Those two
/// events share a timestamp; a real release does not.
fn accept_key(
    conn: &impl x11rb::connection::Connection,
    event: x11rb::protocol::Event,
    held: &mut bool,
    listen: bool,
) -> Result<bool, ()> {
    use x11rb::protocol::Event;
    match event {
        Event::KeyPress(_) if !*held && listen => {
            *held = true;
            Ok(true)
        }
        Event::KeyPress(_) => Ok(false),
        Event::KeyRelease(release) => match conn.poll_for_event() {
            Ok(Some(Event::KeyPress(press)))
                if press.detail == release.detail && press.time == release.time =>
            {
                Ok(false)
            }
            Ok(Some(next)) => {
                *held = false;
                accept_key(conn, next, held, listen)
            }
            Ok(None) => {
                *held = false;
                Ok(false)
            }
            Err(_) => Err(()),
        },
        _ => Ok(false),
    }
}

fn keysym(key: &str) -> Option<u32> {
    match key {
        "space" => Some(0x0020),
        "escape" => Some(0xff1b),
        "tab" => Some(0xff09),
        "return" | "enter" => Some(0xff0d),
        "backspace" => Some(0xff08),
        "delete" => Some(0xffff),
        "left" => Some(0xff51),
        "up" => Some(0xff52),
        "right" => Some(0xff53),
        "down" => Some(0xff54),
        "home" => Some(0xff50),
        "end" => Some(0xff57),
        "insert" => Some(0xff63),
        "pageup" => Some(0xff55),
        "pagedown" => Some(0xff56),
        other if other.len() == 1 => {
            let ch = other.chars().next()?;
            if ch.is_ascii() && !ch.is_ascii_control() {
                Some(ch as u32)
            } else {
                None
            }
        }
        other if other.starts_with('f') && other.len() <= 3 => {
            let number: u32 = other[1..].parse().ok()?;
            if (1..=12).contains(&number) {
                Some(0xffbd + number)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shortcut_reads_as_words() {
        let chord = parse("ctrl-shift-space").unwrap();
        assert!(chord.has_modifier());
        assert_eq!(chord.canonical(), "ctrl-shift-space");
        assert_eq!(chord.label(), "Ctrl Shift Space");
    }

    #[test]
    fn a_bare_letter_is_not_a_global_shortcut() {
        let chord = parse("a").unwrap();
        assert!(!chord.has_modifier());
    }

    #[test]
    fn modifier_keys_are_not_a_shortcut_by_themselves() {
        assert!(is_modifier_only("shift"));
        assert!(parse("shift").is_none());
    }

    #[test]
    fn keys_map_onto_x11_keysyms() {
        assert_eq!(keysym("f1"), Some(0xffbe));
        assert_eq!(keysym("-"), Some(u32::from(b'-')));
        assert_eq!(keysym("space"), Some(0x0020));
    }
}
