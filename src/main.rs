mod app;
mod audio;
mod models;
mod motion;
mod place;
mod stt;
mod text;
mod theme;
mod ui;

use gpui::{
    px, size, App, AppContext, Application, Bounds, Focusable, WindowBackgroundAppearance,
    WindowBounds, WindowDecorations, WindowKind, WindowOptions,
};

fn main() {
    Application::new().run(|cx: &mut App| {
        app::bind_keys(cx);

        let window_size = size(px(app::WINDOW_WIDTH), px(app::COLLAPSED_HEIGHT));
        let bounds = bottom_center(window_size, cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: None,
                focus: true,
                show: true,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                display_id: None,
                window_background: WindowBackgroundAppearance::Transparent,
                app_id: Some("whisp".into()),
                window_min_size: Some(size(px(320.0), px(56.0))),
                window_decorations: Some(WindowDecorations::Client),
                tabbing_identifier: None,
            },
            |window, cx| {
                let view = cx.new(app::Whisp::new);
                window.focus(&view.focus_handle(cx));
                view
            },
        )
        .expect("open the voice window");
        cx.activate(true);
    });
}

fn bottom_center(window_size: gpui::Size<gpui::Pixels>, cx: &App) -> Bounds<gpui::Pixels> {
    let margin = px(16.0);
    let Some(display) = cx.primary_display() else {
        return Bounds {
            origin: gpui::point(px(240.0), px(120.0)),
            size: window_size,
        };
    };
    let screen = display.bounds();
    Bounds {
        origin: gpui::point(
            screen.origin.x + (screen.size.width - window_size.width) * 0.5,
            screen.origin.y + screen.size.height - window_size.height - margin,
        ),
        size: window_size,
    }
}
