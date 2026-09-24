//! The shortcut that shows the bar.
//!
//! Native macOS hotkeys and passive X grabs listen while the panel is hidden.
//! Recording a new shortcut pauses the registration so settings receives it.

use gpui_kit::{Keystroke, Modifiers};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{install, set_paused, take_press};
#[cfg(not(target_os = "macos"))]
mod x11;
#[cfg(not(target_os = "macos"))]
pub use x11::{install, set_paused, take_press};

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

    /// One keycap per modifier symbol, in macOS order (⌃⌥⇧⌘), then the key.
    pub fn keycaps(&self) -> Vec<String> {
        let mut caps = Vec::new();
        for (held, symbol) in [
            (self.ctrl, "⌃"),
            (self.alt, "⌥"),
            (self.shift, "⇧"),
            (self.super_key, "⌘"),
        ] {
            if held {
                caps.push(symbol.to_string());
            }
        }
        caps.push(key_label(&self.key));
        caps
    }
}

/// The shortcut as keycaps, e.g. `["⌃", "⇧", "Space"]`.
pub fn keycaps(source: &str) -> Vec<String> {
    parse(source)
        .map(|chord| chord.keycaps())
        .unwrap_or_else(|| vec![source.to_string()])
}

/// The shortcut in compact symbol form, e.g. `⌃⇧Space`.
pub fn symbols(source: &str) -> String {
    keycaps(source).concat()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shortcut_reads_as_words() {
        let chord = parse("ctrl-shift-space").unwrap();
        assert!(chord.has_modifier());
        assert_eq!(chord.canonical(), "ctrl-shift-space");
        assert_eq!(chord.keycaps(), ["⌃", "⇧", "Space"]);
    }

    #[test]
    fn shortcut_reads_as_symbols() {
        assert_eq!(symbols("ctrl-shift-space"), "⌃⇧Space");
        assert_eq!(symbols("super-alt-k"), "⌥⌘K");
        assert_eq!(
            keycaps("ctrl-alt-shift-super-f5"),
            ["⌃", "⌥", "⇧", "⌘", "F5"]
        );
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
    fn modifier_keys_are_not_a_shortcut_by_themselves() {
        assert!(is_modifier_only("shift"));
        assert!(parse("shift").is_none());
    }
}
