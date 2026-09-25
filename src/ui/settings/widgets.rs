use gpui_kit::component::select::{SearchableVec, Select, SelectState};
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::{
    div, prelude::*, px, Animation, AnimationExt, AnyElement, Context, Div, Entity, FontWeight,
    IntoElement, SharedString, Stateful, Styled,
};

use super::{Choice, SettingsWindow};
use crate::i18n::t;
use crate::platform::hotkey;
use crate::transcription::cloud::Provider;
use crate::ui::motion;
use crate::ui::theme;

pub(super) fn section(title: &'static str, rows: Vec<AnyElement>) -> Div {
    labeled_section(section_label(title), rows)
}

pub(super) fn section_with_detail(
    title: &'static str,
    detail: String,
    rows: Vec<AnyElement>,
) -> Div {
    labeled_section(
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(section_label(title))
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme::TERTIARY)
                    .child(detail),
            ),
        rows,
    )
}

fn labeled_section(label: Div, rows: Vec<AnyElement>) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(label.pl(px(4.0)))
        .child(group().children(rows))
}

pub(super) fn provider_tile(provider: Provider, size: f32) -> Div {
    let (background, mark) = match provider {
        Provider::OpenAi | Provider::Xai | Provider::Vercel => (theme::LABEL, theme::HUD),
        Provider::Groq => (gpui_kit::rgba(0xf54f35ff), theme::LABEL),
    };
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded(px(size * 0.24))
        .bg(background)
        .flex()
        .items_center()
        .justify_center()
        .child(
            Icon::empty()
                .path(provider.icon())
                .text_color(mark)
                .with_size(px(size * 0.9)),
        )
}

pub(super) fn modal_button(id: &'static str, label: &'static str, primary: bool) -> Stateful<Div> {
    div()
        .id(id)
        .role(gpui_kit::Role::Button)
        .aria_label(label)
        .h(px(32.0))
        .px(px(14.0))
        .rounded(px(8.0))
        .bg(if primary { theme::AMBER } else { theme::RAISED })
        .flex()
        .items_center()
        .text_size(px(12.0))
        .when(primary, |button| button.font_weight(FontWeight::SEMIBOLD))
        .text_color(if primary { theme::HUD } else { theme::LABEL })
        .cursor_pointer()
        .hover(|button| button.opacity(0.88))
        .child(label)
}

pub(super) fn selector_control(
    state: &Entity<SelectState<SearchableVec<Choice>>>,
    label: &'static str,
) -> Div {
    div()
        .w(px(220.0))
        .h(px(30.0))
        .flex_shrink_0()
        .rounded(px(8.0))
        .bg(theme::RAISED)
        .child(
            Select::new(state)
                .accessibility_label(label)
                .appearance(false)
                .small()
                .w_full()
                .h_full(),
        )
}

pub(super) fn selector_row(
    title: &'static str,
    subtitle: Option<&'static str>,
    trailing: AnyElement,
    divider: bool,
) -> AnyElement {
    div()
        .min_h(px(if subtitle.is_some() { 58.0 } else { 50.0 }))
        .pl(px(16.0))
        .pr(px(14.0))
        .py(px(9.0))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .when(divider, |row| {
            row.border_t_1().border_color(theme::HAIRLINE)
        })
        .child(label_stack(title, subtitle))
        .child(trailing)
        .into_any_element()
}

pub(super) fn setting_row(
    id: &'static str,
    title: &'static str,
    subtitle: Option<&'static str>,
    trailing: AnyElement,
    divider: bool,
    cx: &mut Context<SettingsWindow>,
    action: impl Fn(&mut SettingsWindow, &mut Context<SettingsWindow>) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .role(gpui_kit::Role::Button)
        .aria_label(title)
        .min_h(px(if subtitle.is_some() { 58.0 } else { 50.0 }))
        .pl(px(16.0))
        .pr(px(14.0))
        .py(px(9.0))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .when(divider, |row| {
            row.border_t_1().border_color(theme::HAIRLINE)
        })
        .cursor_pointer()
        .hover(|row| row.bg(theme::HOVER))
        .on_click(cx.listener(move |view, _, window, cx| {
            view.flipped = Some(id);
            action(view, cx);
            window.refresh();
        }))
        .child(label_stack(title, subtitle))
        .child(trailing)
        .into_any_element()
}

pub(super) fn label_stack(title: &str, subtitle: Option<&str>) -> Div {
    div()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .text_size(px(13.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::LABEL)
                .child(title.to_string()),
        )
        .children(subtitle.map(|subtitle| {
            div()
                .text_size(px(12.0))
                .text_color(theme::SECONDARY)
                .child(subtitle.to_string())
        }))
}

impl SettingsWindow {
    /// A switch for the row `id`. It slides only right after that row was
    /// clicked, so opening a page never animates every switch at once.
    pub(super) fn switch(&self, id: &'static str, on: bool) -> AnyElement {
        const TRAVEL: f32 = 16.0;
        let fill = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .rounded_full()
            .bg(theme::AMBER);
        let knob = div()
            .absolute()
            .top(px(2.0))
            .size(px(18.0))
            .rounded_full()
            .bg(theme::KNOB);
        let track = div()
            .relative()
            .w(px(38.0))
            .h(px(22.0))
            .flex_shrink_0()
            .rounded_full()
            .bg(theme::TRACK);
        let at = move |t: f32| if on { t } else { 1.0 - t };
        if self.flipped != Some(id) {
            return track
                .child(fill.opacity(at(1.0)))
                .child(knob.left(px(2.0 + TRAVEL * at(1.0))))
                .into_any_element();
        }
        let ease = || Animation::new(super::TRANSITION).with_easing(motion::ease_out);
        track
            .child(
                fill.with_animation(format!("{id}-{on}-fill"), ease(), move |fill, t| {
                    fill.opacity(at(t))
                }),
            )
            .child(
                knob.with_animation(format!("{id}-{on}-knob"), ease(), move |knob, t| {
                    knob.left(px(2.0 + TRAVEL * at(t)))
                }),
            )
            .into_any_element()
    }
}

pub(super) fn shortcut_control(shortcut: &str, capturing: bool) -> AnyElement {
    let content = if capturing {
        div()
            .text_size(px(12.0))
            .text_color(theme::AMBER)
            .child(t("Press shortcut…"))
            .into_any_element()
    } else if shortcut.is_empty() {
        div()
            .px(px(6.0))
            .text_size(px(12.0))
            .text_color(theme::TERTIARY)
            .child(t("Not set"))
            .into_any_element()
    } else {
        div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .children(hotkey::keycaps(shortcut).into_iter().map(|cap| {
                div()
                    .h(px(22.0))
                    .min_w(px(22.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.0))
                    .bg(theme::RAISED)
                    .text_size(px(12.0))
                    .text_color(theme::LABEL)
                    .child(cap)
            }))
            .into_any_element()
    };
    div()
        .h(px(30.0))
        .px(px(4.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(6.0))
        .rounded(px(8.0))
        .bg(theme::HUD)
        .shadow(vec![theme::inner_ring(theme::HAIRLINE)])
        .child(content)
        .child(div().w(px(1.0)).h(px(16.0)).bg(theme::HAIRLINE))
        .child(
            div()
                .px(px(8.0))
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::AMBER)
                .child(t("Change")),
        )
        .into_any_element()
}

pub(super) fn section_label(label: &'static str) -> Div {
    div()
        .text_size(px(12.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::SECONDARY)
        .child(label)
}

pub(super) fn group() -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(12.0))
        .overflow_hidden()
        .bg(theme::INSET)
        .shadow(vec![theme::inner_ring(theme::HAIRLINE)])
}

pub(super) fn modal_backdrop() -> Div {
    div()
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .left_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui_kit::rgba(0x000000b3))
}

pub(super) fn link_row(
    id: &'static str,
    title: &'static str,
    detail: impl Into<SharedString>,
    url: &'static str,
    divider: bool,
    _cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    div()
        .id(id)
        .role(gpui_kit::Role::Button)
        .aria_label(title)
        .h(px(42.0))
        .px(px(16.0))
        .flex()
        .items_center()
        .justify_between()
        .when(divider, |row| {
            row.border_t_1().border_color(theme::HAIRLINE)
        })
        .cursor_pointer()
        .hover(|row| row.bg(theme::HOVER))
        .on_click(move |_, _, cx| cx.open_url(url))
        .child(
            div()
                .text_size(px(13.0))
                .text_color(theme::LABEL)
                .child(title),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(theme::SECONDARY)
                .child(detail.into()),
        )
        .into_any_element()
}
