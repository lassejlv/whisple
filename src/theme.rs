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

/// Dark-mode system materials. Blue is the system accent.
pub const MATERIAL: Rgba = color(0x1c1c1ef5);
pub const FILL: Rgba = color(0xffffff1f);
pub const HAIRLINE: Rgba = color(0xffffff2e);
pub const SEPARATOR: Rgba = color(0xffffff1f);
pub const LABEL: Rgba = color(0xf5f5f7ff);
pub const SECONDARY: Rgba = color(0xebebf599);
pub const TERTIARY: Rgba = color(0xebebf55c);
pub const BLUE: Rgba = color(0x0a84ffff);
pub const BLUE_SOFT: Rgba = color(0x0a84ff38);
pub const GREEN: Rgba = color(0x30d158ff);
pub const RED: Rgba = color(0xff453aff);
pub const SHEEN: Rgba = color(0xffffff22);
pub const CLEAR: Rgba = color(0xffffff00);
