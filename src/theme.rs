use gpui::Rgba;

const fn color(hex: u32) -> Rgba {
    let [r, g, b, a] = hex.to_be_bytes();
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a as f32 / 255.0,
    }
}

pub const INK: Rgba = color(0x141418f2);
pub const INK_RAISED: Rgba = color(0x22222af2);
pub const LINE: Rgba = color(0xffffff14);
pub const TEXT: Rgba = color(0xf4f4f5ff);
pub const MUTED: Rgba = color(0xa1a1aaff);
pub const FAINT: Rgba = color(0xffffff14);
pub const ACCENT: Rgba = color(0xff5d73ff);
pub const ACCENT_SOFT: Rgba = color(0xff5d7324);
pub const OK: Rgba = color(0x86efacff);
pub const DANGER: Rgba = color(0xfca5a5ff);
