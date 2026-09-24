use std::time::Duration;

use gpui_kit::base::{Easing, Spring};
use gpui_kit::component::{Theme, ThemeMode, ThemeTokens};
use gpui_kit::{px, BoxShadow, FontFeatures, Hsla, Rgba};

const fn color(hex: u32) -> Rgba {
    let [r, g, b, a] = hex.to_be_bytes();
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a as f32 / 255.0,
    }
}

/// Instrument black: the HUD material, grouped inset sections, raised controls.
pub const HUD: Rgba = color(0x0c0c0eff);
pub const INSET: Rgba = color(0x17171aff);
pub const RAISED: Rgba = color(0x232327ff);
/// White 8%: the window border and every separator.
pub const HAIRLINE: Rgba = color(0xffffff14);
/// White 4%: a row or sidebar item under the pointer.
pub const HOVER: Rgba = color(0xffffff0a);
/// White 12%: the lit top edge of the window, and progress ring tracks.
pub const EDGE: Rgba = color(0xffffff1f);
/// White 6%: the lit top edge of a raised capsule.
pub const SHEEN: Rgba = color(0xffffff0f);
/// White 7%: the lit top edge of an icon tile or keycap.
pub const TILE_SHEEN: Rgba = color(0xffffff12);
/// Black 40%: the shaded bottom edge of a keycap.
pub const LABEL: Rgba = color(0xf5f5f7ff);
pub const SECONDARY: Rgba = color(0x98989fff);
pub const TERTIARY: Rgba = color(0x5e5e65ff);
/// Amber LED: the only accent.
pub const AMBER: Rgba = color(0xffb340ff);
pub const AMBER_SOFT: Rgba = color(0x2a1f0cff);
pub const AMBER_HALO_INNER: Rgba = color(0xffb3400d);
pub const AMBER_HALO_OUTER: Rgba = color(0xffb34006);
/// A saved API key.
pub const GREEN: Rgba = color(0x49c38aff);
pub const GREEN_SOFT: Rgba = color(0x49c38a22);
/// Amber 7%: the selected-row wash.
pub const AMBER_WASH: Rgba = color(0xffb34012);
/// Amber 16%: badge fill.
pub const AMBER_BADGE: Rgba = color(0xffb34029);
/// Amber 18%: the track under the transcribing arc.
pub const AMBER_TRACK: Rgba = color(0xffb3402e);
/// Amber 25%: the ring around an open control.
pub const AMBER_RING: Rgba = color(0xffb34040);
pub const RED: Rgba = color(0xff453aff);
/// Red 18% and 35%: the ring and glow around the stop button.
pub const RED_RING: Rgba = color(0xff453a2e);
pub const RED_GLOW: Rgba = color(0xff453a59);
/// Switch track when off, and its knob.
pub const TRACK: Rgba = color(0x3a3a3fff);
pub const KNOB: Rgba = color(0xffffffff);
pub const CLEAR: Rgba = color(0xffffff00);

#[cfg(target_os = "macos")]
pub const UI_FONT: &str = ".SystemUIFont";
#[cfg(not(target_os = "macos"))]
pub const UI_FONT: &str = "Inter";

pub fn tone(color: Rgba) -> Hsla {
    color.into()
}

/// Figures that keep their width, so a ticking timer does not shift.
pub fn tabular() -> FontFeatures {
    FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)]))
}

/// A 1px lit edge along the top inside of a rounded control.
pub fn top_edge(color: Rgba) -> BoxShadow {
    BoxShadow::new(px(0.0), px(1.0), tone(color)).inset()
}

/// A 1px ring drawn inside a control's bounds.
pub fn inner_ring(color: Rgba) -> BoxShadow {
    BoxShadow::new(px(0.0), px(0.0), tone(color))
        .spread_radius(px(1.0))
        .inset()
}

/// Paint kit controls with the same dark palette and ease-out timings.
pub fn install(cx: &mut gpui_kit::App) {
    let theme = Theme::global_mut(cx);
    theme.mode = ThemeMode::Dark;
    theme.font_family = UI_FONT.into();
    theme.font_size = px(14.0);
    theme.radius = px(9.0);
    theme.radius_lg = px(12.0);
    theme.shadow = false;
    theme.focus_ring = false;
    theme.transparent = tone(CLEAR);

    {
        let colors = &mut theme.colors;
        colors.background = tone(HUD);
        colors.foreground = tone(LABEL);
        colors.primary = tone(AMBER);
        colors.primary_foreground = tone(HUD);
        colors.primary_hover = tone(AMBER);
        colors.primary_active = tone(AMBER);
        colors.secondary = tone(RAISED);
        colors.secondary_foreground = tone(SECONDARY);
        colors.muted = tone(INSET);
        colors.muted_foreground = tone(TERTIARY);
        colors.accent = tone(AMBER_SOFT);
        colors.accent_foreground = tone(AMBER);
        colors.border = tone(HAIRLINE);
        colors.ring = tone(AMBER_RING);
        colors.input = tone(HAIRLINE);
        colors.success = tone(AMBER);
        colors.success_foreground = tone(HUD);
        colors.danger = tone(RED);
        colors.danger_foreground = tone(LABEL);
        colors.link = tone(AMBER);
        colors.button = tone(RAISED);
        colors.button_foreground = tone(LABEL);
        colors.button_hover = tone(RAISED);
        colors.button_active = tone(AMBER_SOFT);
        colors.switch = tone(TRACK);
        colors.switch_thumb = tone(KNOB);
        colors.progress_bar = tone(AMBER);
        colors.scrollbar = tone(CLEAR);
        colors.scrollbar_thumb = tone(TERTIARY);
        colors.scrollbar_thumb_hover = tone(SECONDARY);
        colors.list = tone(CLEAR);
        colors.list_hover = tone(AMBER_WASH);
        colors.list_active = tone(AMBER_WASH);
        colors.popover = tone(HUD);
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
