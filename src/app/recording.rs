use super::*;

impl Whisp {
    pub(super) fn record_from_shortcut(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        if self.recording_hotkey.is_some() || matches!(self.phase, Phase::Transcribing) {
            return;
        }
        if !self.bar_visible {
            self.set_visible(true, window, cx);
        }
        self.toggle_listen(cx);
    }

    pub(crate) fn toggle_listen(&mut self, cx: &mut Context<Self>) {
        if self.recording_hotkey.is_some() || self.actions_suppressed() {
            return;
        }
        if matches!(self.phase, Phase::Idle | Phase::Result(_)) && !self.require_license(cx) {
            return;
        }
        self.error = None;
        self.recovery = None;
        self.copied = false;
        self.stop_recording();
        if !self.selected_ready() {
            self.show_model_choices(cx);
            return;
        }

        match &self.phase {
            Phase::Listening(_) => {
                let Phase::Listening(mic) = std::mem::replace(&mut self.phase, Phase::Transcribing)
                else {
                    return;
                };
                let (samples, rate) = mic.take();
                self.recorded = self
                    .listen_started
                    .take()
                    .map_or(Duration::ZERO, |started| started.elapsed());
                self.levels.clear();
                self.rest_bars();
                self.transcribe(samples, rate, cx);
                self.snap_chrome();
            }
            Phase::Transcribing => {}
            Phase::Idle | Phase::Result(_) => match Mic::start(&self.input_device) {
                Ok(mic) => {
                    self.failed_audio = None;
                    #[cfg(target_os = "macos")]
                    if self.dictation_target.is_none() {
                        dictation::request_access();
                        self.dictation_target = dictation::Target::focused();
                    }
                    self.levels.clear();
                    self.rest_bars();
                    self.phase = Phase::Listening(mic);
                    self.listen_started = Some(Instant::now());
                    self.menu_open = false;
                    self.snap_chrome();
                }
                Err(err) => {
                    self.recovery = Some(
                        if err.to_ascii_lowercase().contains("permission")
                            || err.to_ascii_lowercase().contains("denied")
                        {
                            Recovery::MicrophonePermission
                        } else if !self.input_device.is_empty()
                            && audio::input_names().is_ok_and(|names| {
                                audio::known_input(&self.input_device, &names).is_err()
                            })
                        {
                            Recovery::MicrophoneDisconnected
                        } else {
                            Recovery::Microphone
                        },
                    );
                    self.error = Some(err);
                    self.snap_chrome();
                }
            },
        }
        cx.notify();
    }
    pub(crate) fn note_level(&mut self) {
        let Phase::Listening(mic) = &self.phase else {
            return;
        };
        let level = mic.level();
        self.levels.push_back(level);
        while self.levels.len() > BARS {
            self.levels.pop_front();
        }
        for (index, bar) in self.bars.iter_mut().enumerate() {
            let from_end = BARS - 1 - index;
            bar.target = self
                .levels
                .iter()
                .rev()
                .nth(from_end)
                .copied()
                .unwrap_or(0.08);
        }
    }

    pub(super) fn transcribe(&mut self, samples: Vec<f32>, rate: u32, cx: &mut Context<Self>) {
        self.transcribe_audio(Arc::new(samples), rate, None, false, cx);
    }

    pub(super) fn transcribe_audio(
        &mut self,
        samples: Arc<Vec<f32>>,
        rate: u32,
        local_override: Option<&'static ModelSpec>,
        retry: bool,
        cx: &mut Context<Self>,
    ) {
        #[cfg(feature = "licensing")]
        if self.expire_trial_if_needed(cx) {
            return;
        }
        self.transcription_id = self.transcription_id.wrapping_add(1);
        let transcription_id = self.transcription_id;
        let provider = local_override
            .is_none()
            .then(|| Provider::from_id(&self.selected))
            .flatten();
        self.transcribing_provider = provider;
        let local = provider.is_none().then(|| {
            let spec = local_override.unwrap_or_else(|| self.selected_spec());
            (spec.id.to_string(), models::model_path(spec))
        });
        let language = settings::whisper_language(&self.language).map(str::to_string);
        let clean = self.clean_fillers;
        let copy = self.copy_notes;
        #[cfg(target_os = "macos")]
        let target = self.dictation_target.take();
        #[cfg(not(target_os = "macos"))]
        let target = ();
        let task_samples = Arc::clone(&samples);
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    if let Some(provider) = provider {
                        cloud::transcribe(provider, &task_samples, rate, language.as_deref(), clean)
                            .map_err(|err| (err.message().to_string(), Some(err)))
                    } else {
                        let (model_id, path) = local.expect("local model was selected");
                        stt::transcribe(
                            &model_id,
                            &path,
                            &task_samples,
                            rate,
                            language.as_deref(),
                            clean,
                        )
                        .map_err(|err| (err, None))
                    }
                })
                .await;
            this.update(cx, |view, cx| {
                if view.transcription_id != transcription_id {
                    return;
                }
                #[cfg(feature = "licensing")]
                if !view.license_access.allowed() {
                    view.expire_trial_if_needed(cx);
                    return;
                }
                view.transcribing_provider = None;
                match outcome {
                    Ok(text) => {
                        view.failed_audio = None;
                        view.recovery = None;
                        match assistant::route(&text, view.voice_commands) {
                            Route::Dictation if view.translation_target().is_none() => {
                                view.finish_dictation(text, target, copy, cx)
                            }
                            route => view.act(route, text, target, copy, cx),
                        }
                    }
                    Err((err, cloud_error)) => {
                        view.phase = Phase::Idle;
                        view.recovery = Some(match cloud_error {
                            Some(cloud::TranscriptionError::Offline(_)) => Recovery::CloudOffline,
                            Some(cloud::TranscriptionError::Unauthorized(_)) => {
                                Recovery::CloudKey(provider.expect("cloud request"))
                            }
                            Some(cloud::TranscriptionError::RateLimited(_)) => {
                                Recovery::CloudRateLimited
                            }
                            Some(cloud::TranscriptionError::NoSpeech(_)) => Recovery::NoSpeech,
                            Some(cloud::TranscriptionError::Other(_)) => Recovery::CloudOther,
                            None if err == "No speech came through."
                                || err == "That clip was too short to transcribe." =>
                            {
                                Recovery::NoSpeech
                            }
                            None if retry && local_override.is_some() => Recovery::LocalFallback,
                            None => Recovery::Model,
                        });
                        view.failed_audio = ((provider.is_some() || retry)
                            && !matches!(view.recovery, Some(Recovery::NoSpeech)))
                        .then_some((samples, rate));
                        view.error = Some(err);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn finish_dictation(
        &mut self,
        text: String,
        target: TypingTarget,
        copy: bool,
        cx: &mut Context<Self>,
    ) {
        let insert_error = match type_into(target, &text) {
            Typing::Failed(err) => Some(err),
            Typing::Inserted | Typing::NoTarget => None,
        };
        if copy {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
            self.copied = true;
        } else {
            self.copied = false;
        }
        self.show_result(text, ResultKind::Dictation);
        self.error = insert_error;
    }

    pub(super) fn show_result(&mut self, text: String, kind: ResultKind) {
        self.working = None;
        self.result_kind = kind;
        self.last_text.clone_from(&text);
        self.phase = Phase::Result(text);
        self.error = None;
    }

    /// Runs a voice command, asks the assistant, or translates a note,
    /// keeping the bar busy until it is done.
    pub(super) fn act(
        &mut self,
        route: Route,
        transcript: String,
        target: TypingTarget,
        copy: bool,
        cx: &mut Context<Self>,
    ) {
        let transcription_id = self.transcription_id;
        let asking = matches!(route, Route::Ask(_));
        let translate_to = self.translation_target();
        let provider = (asking || translate_to.is_some())
            .then(|| self.assistant_provider())
            .flatten();
        self.phase = Phase::Transcribing;
        self.transcribing_provider = provider;
        self.working = Some(match route {
            Route::Ask(_) => t("Thinking…"),
            Route::Command(_) => t("Opening…"),
            Route::Dictation => t("Translating…"),
        });
        let request = assistant::Request {
            transcript,
            route,
            provider,
            screen: self.screen_context.then(|| self.screen.clone()),
            translate_to,
        };
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { assistant::perform(request) })
                .await;
            this.update(cx, |view, cx| {
                if view.transcription_id != transcription_id {
                    return;
                }
                view.transcribing_provider = None;
                view.working = None;
                match outcome {
                    Ok(Outcome::Dictation {
                        text,
                        translated,
                        warning,
                    }) => {
                        view.finish_dictation(text, target, copy, cx);
                        if let Some(language) = translated {
                            view.result_kind = ResultKind::Translated(language);
                        }
                        if warning.is_some() {
                            view.error = warning;
                        }
                    }
                    Ok(Outcome::Opened(message)) => {
                        view.copied = false;
                        view.show_result(message, ResultKind::Command);
                    }
                    Ok(Outcome::Answer { text, provider }) => {
                        view.copied = copy;
                        if copy {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        }
                        view.show_result(text, ResultKind::Answer(provider));
                    }
                    Ok(Outcome::Typed { text, provider }) => {
                        let typing = type_into(target, &text);
                        // Text the user asked for must land somewhere: when
                        // it could not be typed, it waits on the clipboard.
                        let copied = copy || !matches!(typing, Typing::Inserted);
                        if copied {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        }
                        view.copied = copied;
                        view.show_result(text, ResultKind::Typed(provider));
                        if let Typing::Failed(err) = typing {
                            view.error = Some(err);
                        }
                    }
                    Err(err) => {
                        view.phase = Phase::Idle;
                        view.recovery = Some(match err.kind {
                            ErrorKind::Launch => Recovery::Command,
                            ErrorKind::NoKey => Recovery::AssistantKey,
                            ErrorKind::Key(provider) => Recovery::CloudKey(provider),
                            ErrorKind::Offline => Recovery::CloudOffline,
                            ErrorKind::RateLimited => Recovery::CloudRateLimited,
                            ErrorKind::Other => Recovery::Assistant,
                        });
                        view.error = Some(err.message);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub(crate) fn failed_audio_available(&self) -> bool {
        self.failed_audio.is_some()
    }

    pub(crate) fn retry_audio(&mut self, local: bool, cx: &mut Context<Self>) {
        if !self.require_license(cx) || !matches!(self.phase, Phase::Idle) {
            return;
        }
        let Some((samples, rate)) = self.failed_audio.take() else {
            return;
        };
        let local_override = if local {
            let Some(spec) = self
                .models
                .iter()
                .find(|model| model.ready && model.spec.id.starts_with("turbo"))
                .map(|model| model.spec)
            else {
                self.failed_audio = Some((samples, rate));
                return;
            };
            Some(spec)
        } else {
            None
        };
        self.error = None;
        self.recovery = None;
        self.phase = Phase::Transcribing;
        self.transcribe_audio(samples, rate, local_override, true, cx);
        self.snap_chrome();
        cx.notify();
    }

    pub(super) fn stop_recording(&mut self) {
        if self.recording_hotkey.is_some() {
            self.recording_hotkey = None;
            hotkey::set_paused(false);
        }
    }
}
