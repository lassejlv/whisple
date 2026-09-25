use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, Once};
use std::thread;
use std::time::Duration;

use crate::platform::hotkey::{Chord, Presses, Shortcut};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt as _;

struct Grab {
    chords: [Option<Chord>; 2],
    paused: bool,
}

static GRAB: Mutex<Grab> = Mutex::new(Grab {
    chords: [None, None],
    paused: false,
});
static PRESSED: AtomicU8 = AtomicU8::new(0);
static STARTED: Once = Once::new();

const SHIFT: u16 = 1;
const LOCK: u16 = 2;
const CONTROL: u16 = 4;
const MOD1: u16 = 8;
const MOD2: u16 = 16;
const MOD4: u16 = 64;
const RELEVANT: u16 = SHIFT | CONTROL | MOD1 | MOD4;

type Grabbed = [Option<(u8, Chord)>; 2];

pub fn install(slot: Shortcut, chord: Chord) -> Result<(), String> {
    if let Ok(mut grab) = GRAB.lock() {
        let index = slot.index();
        if grab.chords[1 - index].as_ref() == Some(&chord) {
            return Err("Whisple already uses that shortcut.".into());
        }
        grab.chords[index] = Some(chord);
    }
    STARTED.call_once(|| {
        thread::Builder::new()
            .name("whisp-hotkey".into())
            .spawn(serve)
            .ok();
    });
    Ok(())
}

pub fn set_paused(paused: bool) {
    if let Ok(mut grab) = GRAB.lock() {
        grab.paused = paused;
    }
}

pub fn take_presses() -> Presses {
    let bits = PRESSED.swap(0, Ordering::Relaxed);
    Presses {
        show: bits & (1 << Shortcut::Show.index()) != 0,
        record: bits & (1 << Shortcut::Record.index()) != 0,
    }
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
    let mut held = [false; 2];
    let mut grabbed: Grabbed = [None, None];

    loop {
        let (chords, paused) = {
            let grab = GRAB.lock().map_err(|_| ())?;
            (grab.chords.clone(), grab.paused)
        };
        for (index, chord) in chords.into_iter().enumerate() {
            let wanted = chord.filter(|chord| !paused && chord.has_modifier());
            if grabbed[index].as_ref().map(|(_, chord)| chord) == wanted.as_ref() {
                continue;
            }
            if let Some((keycode, chord)) = grabbed[index].take() {
                ungrab(&conn, root, keycode, &chord);
            }
            if let Some(chord) = wanted {
                if let Some(keycode) = keycode_for(&conn, &chord.key) {
                    grab_key(&conn, root, keycode, &chord);
                    grabbed[index] = Some((keycode, chord));
                }
            }
        }

        if paused {
            held = [false; 2];
        }
        match conn.poll_for_event() {
            Ok(Some(event)) => {
                if let Some(index) = accept_key(&conn, event, &grabbed, &mut held, !paused)? {
                    PRESSED.fetch_or(1 << index, Ordering::Relaxed);
                }
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => return Err(()),
        }
    }
}

fn grab_key(conn: &impl x11rb::connection::Connection, root: u32, keycode: u8, chord: &Chord) {
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

/// Releases exactly the grabs `grab_key` made, so another shortcut on the
/// same key keeps working.
fn ungrab(conn: &impl x11rb::connection::Connection, root: u32, keycode: u8, chord: &Chord) {
    let base = mask(chord);
    for extra in [0, LOCK, MOD2, LOCK | MOD2] {
        let _ = conn.ungrab_key(keycode, root, (base | extra).into());
    }
    let _ = conn.flush();
}

fn mask(chord: &Chord) -> u16 {
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

fn slot_for(grabbed: &Grabbed, keycode: u8, state: u16) -> Option<usize> {
    grabbed.iter().position(|grab| {
        grab.as_ref()
            .is_some_and(|(code, chord)| *code == keycode && mask(chord) == state & RELEVANT)
    })
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
    grabbed: &Grabbed,
    held: &mut [bool; 2],
    listen: bool,
) -> Result<Option<usize>, ()> {
    use x11rb::protocol::Event;
    match event {
        Event::KeyPress(press) => {
            let Some(index) = slot_for(grabbed, press.detail, u16::from(press.state)) else {
                return Ok(None);
            };
            if held[index] || !listen {
                return Ok(None);
            }
            held[index] = true;
            Ok(Some(index))
        }
        Event::KeyRelease(release) => match conn.poll_for_event() {
            Ok(Some(Event::KeyPress(press)))
                if press.detail == release.detail && press.time == release.time =>
            {
                Ok(None)
            }
            Ok(next) => {
                // The modifiers may already be up, so release by key alone.
                for (index, grab) in grabbed.iter().enumerate() {
                    if grab
                        .as_ref()
                        .is_some_and(|(code, _)| *code == release.detail)
                    {
                        held[index] = false;
                    }
                }
                match next {
                    Some(next) => accept_key(conn, next, grabbed, held, listen),
                    None => Ok(None),
                }
            }
            Err(_) => Err(()),
        },
        _ => Ok(None),
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
    fn two_shortcuts_on_one_key_are_told_apart() {
        let show = crate::platform::hotkey::parse("ctrl-shift-space").unwrap();
        let record = crate::platform::hotkey::parse("ctrl-alt-space").unwrap();
        let grabbed: Grabbed = [Some((65, show)), Some((65, record))];
        assert_eq!(slot_for(&grabbed, 65, CONTROL | SHIFT), Some(0));
        assert_eq!(slot_for(&grabbed, 65, CONTROL | MOD1), Some(1));
        assert_eq!(
            slot_for(&grabbed, 65, CONTROL | MOD1 | LOCK | MOD2),
            Some(1)
        );
        assert_eq!(slot_for(&grabbed, 65, CONTROL), None);
        assert_eq!(slot_for(&grabbed, 66, CONTROL | SHIFT), None);
    }

    #[test]
    fn keys_map_onto_x11_keysyms() {
        assert_eq!(keysym("f1"), Some(0xffbe));
        assert_eq!(keysym("-"), Some(u32::from(b'-')));
        assert_eq!(keysym("space"), Some(0x0020));
    }
}
