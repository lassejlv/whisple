//! Audio: the input device, a live level meter, the spoken language, and
//! the language notes come out in.

use gpui_kit::component::select::{SearchableVec, SelectItem};
use gpui_kit::{div, prelude::*, px, Context, Div, IntoElement, SharedString, Styled};

use crate::audio::Mic;
use crate::settings::Preferences;
use crate::settings_window::widgets::*;
use crate::settings_window::SettingsWindow;
use crate::theme;

#[derive(Clone)]
pub(in crate::settings_window) struct Choice {
    id: String,
    label: SharedString,
}

impl SelectItem for Choice {
    type Value = String;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.id
    }
}

pub(in crate::settings_window) fn microphone_choices(names: &[String]) -> SearchableVec<Choice> {
    let mut choices = vec![Choice {
        id: String::new(),
        label: "System default".into(),
    }];
    choices.extend(names.iter().map(|name| Choice {
        id: name.clone(),
        label: name.clone().into(),
    }));
    SearchableVec::new(choices)
}

pub(in crate::settings_window) fn language_choices() -> SearchableVec<Choice> {
    SearchableVec::new(
        Preferences::languages()
            .iter()
            .map(|language| Choice {
                id: language.id.into(),
                label: language.name.into(),
            })
            .collect::<Vec<_>>(),
    )
}

/// "Same as spoken" first, then every language a note can be translated to.
pub(in crate::settings_window) fn output_language_choices() -> SearchableVec<Choice> {
    let same = Choice {
        id: String::new(),
        label: "Same as spoken".into(),
    };
    SearchableVec::new(
        std::iter::once(same)
            .chain(
                Preferences::languages()
                    .iter()
                    .filter(|language| language.id != "auto")
                    .map(|language| Choice {
                        id: language.id.into(),
                        label: language.name.into(),
                    }),
            )
            .collect::<Vec<_>>(),
    )
}

impl SettingsWindow {
    pub(in crate::settings_window) fn choose_microphone(
        &mut self,
        name: &str,
        cx: &mut Context<Self>,
    ) {
        if !name.is_empty()
            && !self
                .microphone_names
                .iter()
                .any(|candidate| candidate == name)
        {
            return;
        }
        self.monitor = None;
        self.hud
            .update(cx, |hud, cx| hud.set_input_device(name, cx));
        self.monitor = Mic::monitor(name).ok();
        cx.notify();
    }

    pub(in crate::settings_window) fn choose_language(
        &mut self,
        language: &str,
        cx: &mut Context<Self>,
    ) {
        self.hud
            .update(cx, |hud, cx| hud.choose_language(language, cx));
        cx.notify();
    }

    pub(in crate::settings_window) fn choose_output_language(
        &mut self,
        language: &str,
        cx: &mut Context<Self>,
    ) {
        self.hud
            .update(cx, |hud, cx| hud.choose_output_language(language, cx));
        cx.notify();
    }

    pub(in crate::settings_window) fn audio(&self, _cx: &mut Context<Self>) -> Div {
        let bars =
            (0..18).map(|index| {
                let threshold = (index + 1) as f32 / 18.0;
                div().w(px(4.0)).h(px(12.0)).rounded(px(2.0)).bg(
                    if self.monitor_level >= threshold {
                        theme::AMBER
                    } else {
                        theme::TRACK
                    },
                )
            });
        div()
            .flex()
            .flex_col()
            .gap(px(26.0))
            .child(section(
                "Input",
                vec![
                    selector_row(
                        "Microphone",
                        None,
                        selector_control(&self.microphone_select, "Microphone").into_any_element(),
                        false,
                    ),
                    div()
                        .h(px(58.0))
                        .px(px(16.0))
                        .border_t_1()
                        .border_color(theme::HAIRLINE)
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(label_stack(
                            "Input level",
                            Some("Say something to test your microphone."),
                        ))
                        .child(div().flex().gap(px(3.0)).items_center().children(bars))
                        .into_any_element(),
                ],
            ))
            .child(section(
                "Language",
                vec![
                    selector_row(
                        "Spoken language",
                        Some("English-only models always transcribe English."),
                        selector_control(&self.language_select, "Spoken language")
                            .into_any_element(),
                        false,
                    ),
                    selector_row(
                        "Output language",
                        Some("Translates your notes with your OpenAI or Groq key."),
                        selector_control(&self.output_select, "Output language").into_any_element(),
                        true,
                    ),
                ],
            ))
    }
}
