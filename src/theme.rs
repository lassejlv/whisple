use std::time::Duration;

use gpui_kit::base::{Easing, Spring};
use gpui_kit::component::{Theme, ThemeMode, ThemeTokens};
use gpui_kit::{px, Hsla, Rgba};

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
/// Off-state switch track. Solid, so it reads on the translucent row fill.
pub const TRACK: Rgba = color(0x2c2c2eff);

pub fn tone(color: Rgba) -> Hsla {
    color.into()
}

/// Paint kit controls with the capsule palette and the same ease-out timings.
pub fn install(cx: &mut gpui_kit::App) {
    let theme = Theme::global_mut(cx);
    theme.mode = ThemeMode::Dark;
    theme.font_family = "Inter".into();
    theme.font_size = px(15.0);
    theme.radius = px(12.0);
    theme.radius_lg = px(22.0);
    theme.shadow = false;
    theme.focus_ring = false;
    theme.transparent = tone(CLEAR);

    {
        let colors = &mut theme.colors;
        colors.background = tone(MATERIAL);
        colors.foreground = tone(LABEL);
        colors.primary = tone(BLUE);
        colors.primary_foreground = tone(LABEL);
        colors.primary_hover = tone(BLUE);
        colors.primary_active = tone(BLUE);
        colors.secondary = tone(FILL);
        colors.secondary_foreground = tone(SECONDARY);
        colors.muted = tone(FILL);
        colors.muted_foreground = tone(SECONDARY);
        colors.accent = tone(BLUE_SOFT);
        colors.accent_foreground = tone(BLUE);
        colors.border = tone(HAIRLINE);
        colors.ring = tone(BLUE_SOFT);
        colors.input = tone(HAIRLINE);
        colors.success = tone(GREEN);
        colors.success_foreground = tone(LABEL);
        colors.danger = tone(RED);
        colors.danger_foreground = tone(LABEL);
        colors.link = tone(BLUE);
        colors.button = tone(FILL);
        colors.button_foreground = tone(LABEL);
        colors.button_hover = tone(FILL);
        colors.button_active = tone(BLUE_SOFT);
        colors.switch = tone(TRACK);
        colors.switch_thumb = tone(LABEL);
        colors.progress_bar = tone(BLUE);
        colors.scrollbar = tone(CLEAR);
        colors.scrollbar_thumb = tone(TERTIARY);
        colors.scrollbar_thumb_hover = tone(SECONDARY);
        colors.list = tone(CLEAR);
        colors.list_hover = tone(BLUE_SOFT);
        colors.list_active = tone(BLUE_SOFT);
        colors.popover = tone(MATERIAL);
        colors.popover_foreground = tone(LABEL);
    }

    theme.tokens = ThemeTokens::from(&theme.colors);
    let ease = Easing::cubic_bezier(0.23, 1.0, 0.32, 1.0).expect("whisp ease-out");
    theme.motion.duration_fast = Duration::from_millis(140);
    theme.motion.duration_normal = Duration::from_millis(180);
    theme.motion.duration_slow = Duration::from_millis(220);
    theme.motion.easing_enter = ease.clone();
    theme.motion.easing_move = ease;
    theme.motion.spring_control = Spring::new(Duration::from_millis(150));
    theme.motion.spring_move = Spring::new(Duration::from_millis(150));
    Theme::sync_base(cx);
}
