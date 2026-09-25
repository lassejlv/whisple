use std::cell::RefCell;

use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use super::{Chord, Presses, Shortcut};

struct Registration {
    manager: GlobalHotKeyManager,
    /// The native hotkey for each `Shortcut`, by index.
    shortcuts: [Option<HotKey>; 2],
    registered: [bool; 2],
    paused: bool,
    held: [bool; 2],
}

// AppKit hotkeys must be registered and polled on the main thread. Settings
// and the background listener both run on GPUI's foreground executor.
thread_local! {
    static REGISTRATION: RefCell<Option<Registration>> = const { RefCell::new(None) };
}

pub fn install(slot: Shortcut, chord: Chord) -> Result<(), String> {
    let shortcut = native_shortcut(&chord)?;
    REGISTRATION.with_borrow_mut(|state| {
        if state.is_none() {
            *state = Some(Registration {
                manager: GlobalHotKeyManager::new().map_err(|err| err.to_string())?,
                shortcuts: [None, None],
                registered: [false, false],
                paused: false,
                held: [false, false],
            });
        }
        let state = state.as_mut().unwrap();
        let index = slot.index();
        if state.shortcuts[1 - index] == Some(shortcut) {
            return Err("Whisple already uses that shortcut.".into());
        }
        if state.shortcuts[index] == Some(shortcut) {
            return Ok(());
        }
        // Register first so a conflict never replaces a working preference.
        state
            .manager
            .register(shortcut)
            .map_err(|err| err.to_string())?;
        if let Some(previous) = state.shortcuts[index].filter(|_| state.registered[index]) {
            if let Err(err) = state.manager.unregister(previous) {
                let _ = state.manager.unregister(shortcut);
                return Err(err.to_string());
            }
        }
        // While paused, registering only proved the shortcut is free.
        if state.paused {
            let _ = state.manager.unregister(shortcut);
        }
        state.shortcuts[index] = Some(shortcut);
        state.registered[index] = !state.paused;
        state.held[index] = false;
        while GlobalHotKeyEvent::receiver().try_recv().is_ok() {}
        Ok(())
    })
}

pub fn set_paused(paused: bool) {
    REGISTRATION.with_borrow_mut(|state| {
        let Some(state) = state else { return };
        if state.paused == paused {
            return;
        }
        for index in 0..state.shortcuts.len() {
            let Some(shortcut) = state.shortcuts[index] else {
                continue;
            };
            let result = match (paused, state.registered[index]) {
                (true, true) => state.manager.unregister(shortcut),
                (false, false) => state.manager.register(shortcut),
                _ => Ok(()),
            };
            match result {
                Ok(()) => state.registered[index] = !paused,
                Err(err) => eprintln!("could not update the global shortcut: {err}"),
            }
        }
        state.paused = paused;
        state.held = [false, false];
        while GlobalHotKeyEvent::receiver().try_recv().is_ok() {}
    });
}

pub fn take_presses() -> Presses {
    REGISTRATION.with_borrow_mut(|state| {
        let mut presses = Presses::default();
        let Some(state) = state else { return presses };
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            let Some(index) = (0..state.shortcuts.len()).find(|&index| {
                state.registered[index]
                    && state.shortcuts[index].map(|shortcut| shortcut.id()) == Some(event.id)
            }) else {
                continue;
            };
            match event.state {
                HotKeyState::Pressed if !state.held[index] => {
                    state.held[index] = true;
                    presses.flip(Shortcut::ALL[index]);
                }
                HotKeyState::Released => state.held[index] = false,
                _ => {}
            }
        }
        presses
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
