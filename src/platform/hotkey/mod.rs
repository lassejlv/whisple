use gpui_kit::{Keystroke, Modifiers};

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub use super::linux::hotkey::{install, set_paused, take_presses};
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use native::{install, set_paused, take_presses};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    Show,
    Record,
}

impl Shortcut {
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    pub const ALL: [Self; 2] = [Self::Show, Self::Record];

    pub fn index(self) -> usize {
        match self {
            Self::Show => 0,
            Self::Record => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Presses {
    pub show: bool,
    pub record: bool,
}

impl Presses {
    pub fn any(self) -> bool {
        self.show || self.record
    }

    /// Two presses before the app looks cancel out, like a quick double tap
    /// on a toggle.
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    pub(crate) fn flip(&mut self, shortcut: Shortcut) {
        match shortcut {
            Shortcut::Show => self.show = !self.show,
            Shortcut::Record => self.record = !self.record,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    pub key: String,
}

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

    /// Keycaps as the system writes them: words on Windows, symbols
    /// elsewhere.
    pub fn keycaps(&self) -> Vec<String> {
        let names = if cfg!(target_os = "windows") {
            ["Ctrl", "Alt", "Shift", "Win"]
        } else {
            ["⌃", "⌥", "⇧", "⌘"]
        };
        let mut caps = Vec::new();
        for (held, symbol) in [
            (self.ctrl, names[0]),
            (self.alt, names[1]),
            (self.shift, names[2]),
            (self.super_key, names[3]),
        ] {
            if held {
                caps.push(symbol.to_string());
            }
        }
        caps.push(key_label(&self.key));
        caps
    }
}

pub fn keycaps(source: &str) -> Vec<String> {
    parse(source)
        .map(|chord| chord.keycaps())
        .unwrap_or_else(|| vec![source.to_string()])
}

pub fn symbols(source: &str) -> String {
    let caps = keycaps(source);
    if cfg!(target_os = "windows") {
        caps.join("+")
    } else {
        caps.concat()
    }
}

fn key_label(key: &str) -> String {
    match key {
        "space" => "Space".into(),
        "escape" => "Esc".into(),
        "return" | "enter" if cfg!(target_os = "windows") => "Enter".into(),
        "return" | "enter" => "Return".into(),
        "tab" => "Tab".into(),
        "backspace" if cfg!(target_os = "windows") => "Backspace".into(),
        "backspace" => "Delete".into(),
        "delete" if cfg!(target_os = "windows") => "Del".into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shortcut_reads_as_words() {
        let chord = parse("ctrl-shift-space").unwrap();
        assert!(chord.has_modifier());
        assert_eq!(chord.canonical(), "ctrl-shift-space");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(chord.keycaps(), ["⌃", "⇧", "Space"]);
        #[cfg(target_os = "windows")]
        assert_eq!(chord.keycaps(), ["Ctrl", "Shift", "Space"]);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn shortcut_reads_as_symbols() {
        assert_eq!(symbols("ctrl-shift-space"), "⌃⇧Space");
        assert_eq!(symbols("super-alt-k"), "⌥⌘K");
        assert_eq!(
            keycaps("ctrl-alt-shift-super-f5"),
            ["⌃", "⌥", "⇧", "⌘", "F5"]
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn shortcut_reads_as_windows_key_names() {
        assert_eq!(symbols("ctrl-shift-space"), "Ctrl+Shift+Space");
        assert_eq!(symbols("super-alt-k"), "Alt+Win+K");
        assert_eq!(symbols("ctrl-backspace"), "Ctrl+Backspace");
        assert_eq!(symbols("alt-return"), "Alt+Enter");
    }

    #[test]
    fn an_unreadable_shortcut_stays_as_typed() {
        assert_eq!(symbols(""), "");
        assert_eq!(keycaps("ctrl-"), ["ctrl-"]);
    }

    #[test]
    fn a_bare_letter_is_not_a_global_shortcut() {
        let chord = parse("a").unwrap();
        assert!(!chord.has_modifier());
    }

    #[test]
    fn repeated_presses_cancel_out() {
        let mut presses = Presses::default();
        presses.flip(Shortcut::Record);
        assert!(presses.record && presses.any());
        presses.flip(Shortcut::Record);
        assert!(!presses.any());
    }

    #[test]
    fn modifier_keys_are_not_a_shortcut_by_themselves() {
        assert!(is_modifier_only("shift"));
        assert!(parse("shift").is_none());
    }
}
