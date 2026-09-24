mod app;
mod audio;
mod cloud;
mod hotkey;
mod models;
mod motion;
mod place;
mod settings;
mod startup;
mod stt;
mod text;
mod theme;
mod tray;
mod ui;
#[cfg(target_os = "macos")]
mod updater;

use std::borrow::Cow;

use gpui_kit::component::Root;
use gpui_kit::{
    px, size, App, AppContext, AssetSource, Bounds, Focusable, SharedString,
    WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowKind, WindowOptions,
};

struct Assets;

// Lucide icons the panels use beyond the kit's default component bundle.
gpui_kit::assets::icon_assets!(ExtraIcons, [Clipboard, Copy, Globe, Mic, Power, Sparkles]);

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        let ours: Option<Cow<'static, [u8]>> = match path {
            "icons/gear.svg" => Some(Cow::Borrowed(include_bytes!("../assets/gear.svg"))),
            "icons/chevron.svg" => Some(Cow::Borrowed(include_bytes!("../assets/chevron.svg"))),
            "icons/chevron-left.svg" => {
                Some(Cow::Borrowed(include_bytes!("../assets/chevron-left.svg")))
            }
            "icons/whisp/chevrons-up-down.svg" => Some(Cow::Borrowed(include_bytes!(
                "../assets/chevrons-up-down.svg"
            ))),
            "icons/whisp/ring-arc.svg" => {
                Some(Cow::Borrowed(include_bytes!("../assets/ring-arc.svg")))
            }
            "icons/whisp/ring-track.svg" => {
                Some(Cow::Borrowed(include_bytes!("../assets/ring-track.svg")))
            }
            "icons/whisp/check-bold.svg" => {
                Some(Cow::Borrowed(include_bytes!("../assets/check-bold.svg")))
            }
            "icons/whisp/chevron-left-bold.svg" => Some(Cow::Borrowed(include_bytes!(
                "../assets/chevron-left-bold.svg"
            ))),
            "icons/whisp/chevron-right-bold.svg" => Some(Cow::Borrowed(include_bytes!(
                "../assets/chevron-right-bold.svg"
            ))),
            "icons/whisp/keyboard.svg" => {
                Some(Cow::Borrowed(include_bytes!("../assets/keyboard.svg")))
            }
            "icons/whisp/play.svg" => Some(Cow::Borrowed(include_bytes!("../assets/play.svg"))),
            "icons/whisp/search.svg" => Some(Cow::Borrowed(include_bytes!("../assets/search.svg"))),
            "icons/whisp/trash.svg" => Some(Cow::Borrowed(include_bytes!("../assets/trash.svg"))),
            "icons/whisp/openai.svg" => Some(Cow::Borrowed(include_bytes!("../assets/openai.svg"))),
            "icons/whisp/groq.svg" => Some(Cow::Borrowed(include_bytes!("../assets/groq.svg"))),
            "icons/whisp/groq-mark.svg" => {
                Some(Cow::Borrowed(include_bytes!("../assets/groq-mark.svg")))
            }
            _ => None,
        };
        if ours.is_some() {
            return Ok(ours);
        }
        if let Ok(Some(bytes)) = ExtraIcons.load(path) {
            return Ok(Some(bytes));
        }
        // Kit icons (settings, chevrons, check) live in the component bundle.
        // A missing path is empty, not a hard error, so our own names still resolve.
        match gpui_kit::assets::Assets.load(path) {
            Ok(bytes) => Ok(bytes),
            Err(_) => Ok(None),
        }
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(Assets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            theme::install(cx);
            app::bind_keys(cx);
            let menu_bar = tray::install(cx);

            let window_size = size(px(app::WINDOW_WIDTH), px(app::COLLAPSED_HEIGHT));
            let bounds = bottom_center(window_size, cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: None,
                    focus: !menu_bar,
                    show: !menu_bar,
                    kind: WindowKind::PopUp,
                    is_movable: false,
                    // AppKit only adds the resizable style bit when a titlebar is
                    // present, and setContentSize keeps the top-left fixed. The
                    // bottom edge is pinned in `place` instead.
                    app_owns_titlebar_drag: false,
                    inactive_frame_interval: None,
                    is_resizable: false,
                    is_minimizable: false,
                    display_id: None,
                    window_background: WindowBackgroundAppearance::Transparent,
                    icon: None,
                    app_id: Some("whisple".into()),
                    window_min_size: Some(size(px(320.0), px(app::COLLAPSED_HEIGHT))),
                    window_decorations: Some(WindowDecorations::Client),
                    tabbing_identifier: None,
                },
                |window, cx| {
                    let view = cx.new(|cx| app::Whisp::new(window, !menu_bar, cx));
                    window.focus(&view.focus_handle(cx), cx);
                    // Only the rounded HUD paints. The kit root would otherwise fill
                    // the whole window, square corners included.
                    cx.new(|cx| {
                        use gpui_kit::Styled as _;
                        Root::new(view, window, cx)
                            .bordered(false)
                            .bg(gpui_kit::transparent_black())
                    })
                },
            )
            .expect("open the voice window");
            if !menu_bar {
                cx.activate(true);
            }
        });
}

fn bottom_center(
    window_size: gpui_kit::Size<gpui_kit::Pixels>,
    cx: &App,
) -> Bounds<gpui_kit::Pixels> {
    let margin = px(16.0);
    let Some(display) = cx.primary_display() else {
        return Bounds {
            origin: gpui_kit::point(px(240.0), px(120.0)),
            size: window_size,
        };
    };
    let screen = display.bounds();
    Bounds {
        origin: gpui_kit::point(
            screen.origin.x + (screen.size.width - window_size.width) * 0.5,
            screen.origin.y + screen.size.height - window_size.height - margin,
        ),
        size: window_size,
    }
}
