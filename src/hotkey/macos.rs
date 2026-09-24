use std::cell::RefCell;

use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use super::Chord;

struct Registration {
    manager: GlobalHotKeyManager,
    shortcut: Option<HotKey>,
    active: bool,
    held: bool,
}

// AppKit hotkeys must be registered and polled on the main thread. Settings
// and the background listener both run on GPUI's foreground executor.
thread_local! {
    static REGISTRATION: RefCell<Option<Registration>> = const { RefCell::new(None) };
}

pub fn install(chord: Chord) -> Result<(), String> {
    let shortcut = native_shortcut(&chord)?;
    REGISTRATION.with_borrow_mut(|state| {
        if state.is_none() {
            *state = Some(Registration {
                manager: GlobalHotKeyManager::new().map_err(|err| err.to_string())?,
                shortcut: None,
                active: false,
                held: false,
            });
        }
        let state = state.as_mut().unwrap();
        if state.shortcut == Some(shortcut) && state.active {
            return Ok(());
        }
        // Register first so a conflict never replaces a working preference.
        state
            .manager
            .register(shortcut)
            .map_err(|err| err.to_string())?;
        if state.active {
            if let Some(previous) = state.shortcut {
                if let Err(err) = state.manager.unregister(previous) {
                    let _ = state.manager.unregister(shortcut);
                    return Err(err.to_string());
                }
            }
        }
        state.shortcut = Some(shortcut);
        state.active = true;
        state.held = false;
        while GlobalHotKeyEvent::receiver().try_recv().is_ok() {}
        Ok(())
    })
}

pub fn set_paused(paused: bool) {
    REGISTRATION.with_borrow_mut(|state| {
        let Some(state) = state else { return };
        let Some(shortcut) = state.shortcut else {
            return;
        };
        if state.active == !paused {
            return;
        }
        let result = if paused {
            state.manager.unregister(shortcut)
        } else {
            state.manager.register(shortcut)
        };
        match result {
            Ok(()) => {
                state.active = !paused;
                state.held = false;
                while GlobalHotKeyEvent::receiver().try_recv().is_ok() {}
            }
            Err(err) => eprintln!("could not update the global shortcut: {err}"),
        }
    });
}

pub fn take_press() -> bool {
    REGISTRATION.with_borrow_mut(|state| {
        let Some(state) = state else { return false };
        let mut pressed = false;
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if !state.active || state.shortcut.map(|shortcut| shortcut.id()) != Some(event.id) {
                continue;
            }
            match event.state {
                HotKeyState::Pressed if !state.held => {
                    state.held = true;
                    pressed = !pressed;
                }
                HotKeyState::Released => state.held = false,
                _ => {}
            }
        }
        pressed
    })
}

fn native_shortcut(chord: &Chord) -> Result<HotKey, String> {
    if !chord.has_modifier() {
        return Err("Use Ctrl, Option, or Command as well.".into());
    }
    let mut keys = Vec::new();
    for (held, modifier) in [
        (chord.ctrl, "Control"),
        (chord.alt, "Alt"),
        (chord.shift, "Shift"),
        (chord.super_key, "Super"),
    ] {
        if held {
            keys.push(modifier);
        }
    }
    keys.push(match chord.key.as_str() {
        "return" => "Enter",
        "+" => "Equal",
        key => key,
    });
    keys.join("+")
        .parse::<HotKey>()
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use global_hotkey::hotkey::{Code, Modifiers};

    #[test]
    fn stored_shortcuts_map_to_native_keys_and_modifiers() {
        for (source, code, modifiers) in [
            (
                "ctrl-shift-space",
                Code::Space,
                Modifiers::CONTROL | Modifiers::SHIFT,
            ),
            ("super-alt-k", Code::KeyK, Modifiers::SUPER | Modifiers::ALT),
            ("ctrl-return", Code::Enter, Modifiers::CONTROL),
            ("super-f5", Code::F5, Modifiers::SUPER),
            ("ctrl--", Code::Minus, Modifiers::CONTROL),
        ] {
            let chord = crate::hotkey::parse(source).unwrap();
            assert_eq!(
                native_shortcut(&chord).unwrap(),
                HotKey::new(Some(modifiers), code)
            );
        }
    }

    #[test]
    fn invalid_shortcuts_cannot_replace_a_registration() {
        assert!(native_shortcut(&crate::hotkey::parse("k").unwrap()).is_err());
        let mut chord = crate::hotkey::parse("ctrl-space").unwrap();
        chord.key = "unsupported".into();
        assert!(native_shortcut(&chord).is_err());
    }
}
