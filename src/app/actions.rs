use super::*;

impl Whisp {
    pub(crate) fn choose_model(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(spec) = models::spec(id) else {
            return;
        };
        self.pending_uninstall = None;
        self.error = None;
        self.recovery = None;
        if models::is_downloaded(spec) {
            self.selected = spec.id.to_string();
            models::save_selected(spec.id);
            self.refresh_models();
            self.menu_open = false;
            self.snap_chrome();
            cx.notify();
            return;
        }
        self.start_download(spec, cx);
    }

    pub(crate) fn choose_cloud(&mut self, provider: Provider, cx: &mut Context<Self>) {
        if !self.cloud_keys[provider.index()] {
            self.open_settings_at(SettingsTarget::CloudKey(provider), cx);
            return;
        }
        self.selected = provider.id().to_string();
        self.pending_uninstall = None;
        self.menu_open = false;
        // A saved key after a rejected one: offer the kept recording again.
        if matches!(self.recovery, Some(Recovery::CloudKey(_))) && self.failed_audio.is_some() {
            self.error = Some(tf(
                "{} key saved. Retry your recording.",
                &[&provider.name()],
            ));
            self.recovery = Some(Recovery::CloudOther);
        } else {
            self.error = None;
            self.recovery = None;
        }
        self.persist();
        self.snap_chrome();
        cx.notify();
    }

    pub(crate) fn choose_from_menu(&mut self, id: &str, cx: &mut Context<Self>) {
        match Provider::from_id(id) {
            Some(provider) => self.choose_cloud(provider, cx),
            None => self.choose_model(id, cx),
        }
    }

    pub(crate) fn cancel_download(&mut self, cx: &mut Context<Self>) {
        if let Some(download) = self.download.take() {
            download.cancel.store(true, Ordering::Relaxed);
            cx.notify();
        }
    }

    pub(crate) fn uninstall_model(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(spec) = models::spec(id) else {
            return;
        };
        if !models::is_downloaded(spec) {
            self.pending_uninstall = None;
            self.refresh_models();
            cx.notify();
            return;
        }
        if self.pending_uninstall.as_deref() != Some(id) {
            self.pending_uninstall = Some(id.to_string());
            cx.notify();
            return;
        }
        self.pending_uninstall = None;
        if matches!(self.phase, Phase::Listening(_) | Phase::Transcribing)
            || self
                .download
                .as_ref()
                .is_some_and(|download| download.id == id)
        {
            self.error =
                Some(t("Finish recording or downloading before removing this model.").into());
            cx.notify();
            return;
        }
        match models::uninstall(spec) {
            Ok(()) => {
                self.refresh_models();
                if self.selected == id {
                    let replacement = self
                        .models
                        .iter()
                        .find(|model| model.ready && model.spec.recommended)
                        .or_else(|| self.models.iter().find(|model| model.ready))
                        .map(|model| model.spec.id)
                        .unwrap_or_else(models::recommended_id);
                    self.selected = replacement.to_string();
                    self.persist();
                }
                self.error = None;
            }
            Err(err) => self.error = Some(err),
        }
        cx.notify();
    }

    pub(crate) fn storage_used(&self) -> u64 {
        self.models
            .iter()
            .filter(|model| model.ready)
            .map(|model| model.spec.bytes)
            .sum()
    }

    pub(crate) fn toggle_menu(&mut self, cx: &mut Context<Self>) {
        self.stop_recording();
        if self.menu_open {
            self.menu_open = false;
        } else {
            self.show_model_choices(cx);
            return;
        }
        cx.notify();
    }

    pub(super) fn show_model_choices(&mut self, cx: &mut Context<Self>) {
        if self.menu_choices().is_empty() {
            self.menu_open = false;
            self.open_settings_at(SettingsTarget::Models, cx);
            return;
        }
        self.menu_open = true;
        cx.notify();
    }

    pub(crate) fn restart_onboarding(&mut self, cx: &mut Context<Self>) {
        if self.visibility_locked() {
            self.error = Some(t("Finish recording before setting up again.").into());
            self.snap_chrome();
            cx.notify();
            return;
        }
        self.stop_recording();
        self.cancel_download(cx);
        self.transcription_id = self.transcription_id.wrapping_add(1);
        let hud = self.hud_window;
        let settings = self.settings_window.take();
        cx.defer(move |cx| {
            crate::ui::onboarding::open(cx);
            if let Some(settings) = settings {
                settings.close(cx);
            }
            hud.update(cx, |_, window, _| window.remove_window()).ok();
        });
    }

    pub(crate) fn open_settings_window(&mut self, cx: &mut Context<Self>) {
        self.open_settings_at(SettingsTarget::General, cx);
    }

    #[cfg(feature = "licensing")]
    pub(crate) fn open_license_window(&mut self, cx: &mut Context<Self>) {
        self.open_settings_at(SettingsTarget::License, cx);
    }

    /// Ends the trial the moment its time is up. Nothing opens by itself: the
    /// bar shows the ended trial and Unlock leads to the License page.
    #[cfg(feature = "licensing")]
    pub(super) fn expire_trial_if_needed(&mut self, cx: &mut Context<Self>) -> bool {
        if let Access::Trial {
            confirmation_required,
            ..
        } = &self.license_access
        {
            if !self.license_access.allowed() {
                let expired = if *confirmation_required {
                    Access::Unavailable {
                        display_key: String::new(),
                        reason: t("Connect to the internet to continue your trial.").into(),
                    }
                } else {
                    Access::TrialExpired
                };
                self.set_license_access(expired, cx);
                return true;
            }
        }
        false
    }

    #[cfg(feature = "licensing")]
    pub(crate) fn locked(&self) -> bool {
        !matches!(self.license_access, Access::Checking) && !self.license_access.allowed()
    }

    #[cfg(not(feature = "licensing"))]
    pub(crate) fn locked(&self) -> bool {
        false
    }

    #[cfg(feature = "licensing")]
    pub(crate) fn trial_ending(&self) -> Option<Duration> {
        if matches!(
            self.license_access,
            Access::Trial {
                confirmation_required: true,
                ..
            }
        ) {
            return None;
        }
        self.license_access
            .trial_remaining()
            .filter(|left| !left.is_zero() && *left < Duration::from_secs(24 * 60 * 60))
    }

    pub(crate) fn open_settings_at(&mut self, target: SettingsTarget, cx: &mut Context<Self>) {
        self.stop_recording();
        self.menu_open = false;
        self.snap_chrome();
        let hud = cx.entity();
        let existing = self.settings_window.clone();
        cx.defer(move |cx| {
            let handle = crate::ui::settings::open(cx, hud.clone(), existing, target);
            hud.update(cx, |view, cx| {
                view.settings_window = Some(handle);
                cx.notify();
            });
        });
        cx.notify();
    }

    #[cfg(feature = "licensing")]
    pub(super) fn require_license(&mut self, cx: &mut Context<Self>) -> bool {
        self.expire_trial_if_needed(cx);
        if self.license_access.allowed() {
            return true;
        }
        if matches!(self.license_access, Access::Checking) {
            self.error = Some(t("Checking your license. Try again in a moment.").into());
            self.snap_chrome();
            cx.notify();
            return false;
        }
        // Pressing record on a locked bar is the one place that leads to
        // buying or fixing the license.
        self.open_license_window(cx);
        false
    }

    #[cfg(not(feature = "licensing"))]
    pub(super) fn require_license(&mut self, _cx: &mut Context<Self>) -> bool {
        true
    }

    #[cfg(feature = "licensing")]
    pub(crate) fn set_license_access(&mut self, access: Access, cx: &mut Context<Self>) {
        if !access.allowed() && matches!(self.phase, Phase::Listening(_) | Phase::Transcribing) {
            self.transcription_id = self.transcription_id.wrapping_add(1);
            self.phase = Phase::Idle;
            self.failed_audio = None;
            self.transcribing_provider = None;
            self.working = None;
            self.listen_started = None;
            self.levels.clear();
            self.rest_bars();
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                self.dictation_target = None;
            }
            self.snap_chrome();
        }
        let previous_license_error = match &self.license_access {
            Access::Blocked { reason, .. } | Access::Unavailable { reason, .. } => {
                self.error.as_deref() == Some(reason.as_str())
            }
            _ => self.error.as_deref() == Some(t("Checking your license. Try again in a moment.")),
        };
        match &access {
            Access::Blocked { reason, .. } | Access::Unavailable { reason, .. } => {
                self.error = Some(reason.clone());
            }
            _ if previous_license_error => self.error = None,
            _ => {}
        }
        self.license_access = access;
        self.license_checking = false;
        self.license_generation = self.license_generation.wrapping_add(1);
        self.last_license_check = Instant::now();
        self.snap_chrome();
        cx.notify();
    }

    #[cfg(feature = "licensing")]
    pub(crate) fn refresh_license(&mut self, cx: &mut Context<Self>) {
        if self.license_checking {
            return;
        }
        self.license_checking = true;
        self.last_license_check = Instant::now();
        let generation = self.license_generation;
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let access = cx
                .background_executor()
                .spawn(async { license::check_saved() })
                .await;
            this.update(cx, |view, cx| {
                if view.license_generation == generation {
                    view.set_license_access(access, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn set_input_device(&mut self, name: &str, cx: &mut Context<Self>) {
        self.input_device = name.to_string();
        self.persist();
        cx.notify();
    }

    pub(crate) fn use_saved_cloud_key(&mut self, provider: Provider, cx: &mut Context<Self>) {
        self.cloud_keys[provider.index()] = true;
        self.choose_cloud(provider, cx);
    }

    pub(crate) fn forget_cloud_key(&mut self, provider: Provider, cx: &mut Context<Self>) {
        self.cloud_keys[provider.index()] = false;
        if self.selected == provider.id() {
            self.selected = models::recommended_id().to_string();
            self.persist();
        }
        cx.notify();
    }

    pub(crate) fn check_for_updates_now(&mut self, cx: &mut Context<Self>) {
        #[cfg(not(feature = "licensing"))]
        cx.open_url("https://github.com/lassejlv/whisple/releases");
        #[cfg(all(updates, feature = "licensing"))]
        self.check_for_updates(cx);
        #[cfg(all(not(updates), feature = "licensing"))]
        let _ = cx;
    }

    pub(crate) fn update_summary(&self) -> String {
        #[cfg(updates)]
        {
            if self.update_checking {
                return t("Checking for updates…").into();
            }
            if let Some(update) = &self.update {
                return tf("Whisple v{} is ready to install", &[&update.version]);
            }
        }
        tf("Version {}", &[&env!("CARGO_PKG_VERSION")])
    }

    pub(crate) fn ready_update(&self) -> Option<String> {
        #[cfg(updates)]
        {
            self.update
                .as_ref()
                .map(|update| update.version.to_string())
        }
        #[cfg(not(updates))]
        {
            None
        }
    }

    pub(crate) fn dismiss_update(&mut self, cx: &mut Context<Self>) {
        #[cfg(updates)]
        {
            self.update_prompt = None;
        }
        self.snap_chrome();
        cx.notify();
    }

    /// Installs and restarts. Never while a recording is running.
    pub(crate) fn install_update(&mut self, cx: &mut Context<Self>) {
        #[cfg(updates)]
        if !self.visibility_locked() {
            if let Some(update) = self.update.as_mut() {
                match update.install() {
                    Ok(()) => cx.quit(),
                    Err(err) => {
                        self.error = Some(tf("Could not install the update: {}", &[&err]));
                        self.snap_chrome();
                    }
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn toggle_open_on_startup(&mut self, cx: &mut Context<Self>) {
        self.set_open_on_startup(!self.open_on_startup, cx);
    }

    pub(crate) fn set_open_on_startup(&mut self, on: bool, cx: &mut Context<Self>) {
        if on == self.open_on_startup {
            return;
        }
        if let Err(err) = startup::apply(on) {
            self.error = Some(err);
            cx.notify();
            return;
        }
        self.open_on_startup = on;
        self.error = None;
        self.persist();
        cx.notify();
    }

    pub(crate) fn choose_language(&mut self, id: &str, cx: &mut Context<Self>) {
        if !Preferences::languages()
            .iter()
            .any(|language| language.id == id)
        {
            return;
        }
        self.language = id.to_string();
        self.persist();
        cx.notify();
    }

    pub(crate) fn choose_output_language(&mut self, id: &str, cx: &mut Context<Self>) {
        if !id.is_empty() && settings::language_name(id).is_none() {
            return;
        }
        self.output_language = id.to_string();
        self.persist();
        cx.notify();
    }

    pub(super) fn translation_target(&self) -> Option<&'static str> {
        (self.output_language != self.language)
            .then(|| settings::language_name(&self.output_language))
            .flatten()
    }

    pub(crate) fn toggle_voice_commands(&mut self, cx: &mut Context<Self>) {
        self.voice_commands = !self.voice_commands;
        self.persist();
        cx.notify();
    }

    pub(crate) fn toggle_screen_context(&mut self, cx: &mut Context<Self>) {
        self.screen_context = !self.screen_context;
        if !self.screen_context {
            self.screen = Snapshot::default();
        }
        self.persist();
        cx.notify();
    }

    /// The cloud provider the assistant asks: the selected one when it is a
    /// cloud model, otherwise any with a saved key.
    pub(super) fn assistant_provider(&self) -> Option<Provider> {
        Provider::from_id(&self.selected)
            .filter(|provider| self.cloud_keys[provider.index()])
            .or_else(|| {
                Provider::ALL
                    .into_iter()
                    .find(|provider| self.cloud_keys[provider.index()])
            })
    }

    pub(crate) fn begin_hotkey_capture(&mut self, slot: Shortcut, cx: &mut Context<Self>) {
        if self.recording_hotkey == Some(slot) {
            self.stop_recording();
        } else {
            self.recording_hotkey = Some(slot);
            self.error = None;
            hotkey::set_paused(true);
        }
        cx.notify();
    }

    pub(crate) fn toggle_copy_notes(&mut self, cx: &mut Context<Self>) {
        self.set_copy_notes(!self.copy_notes, cx);
    }

    pub(crate) fn set_copy_notes(&mut self, on: bool, cx: &mut Context<Self>) {
        if on == self.copy_notes {
            return;
        }
        self.copy_notes = on;
        self.persist();
        cx.notify();
    }

    pub(crate) fn toggle_clean_fillers(&mut self, cx: &mut Context<Self>) {
        self.set_clean_fillers(!self.clean_fillers, cx);
    }

    pub(crate) fn set_clean_fillers(&mut self, on: bool, cx: &mut Context<Self>) {
        if on == self.clean_fillers {
            return;
        }
        self.clean_fillers = on;
        self.persist();
        cx.notify();
    }

    pub(crate) fn capture_hotkey(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        if hotkey::is_modifier_only(&keystroke.key) {
            return;
        }
        let Some(slot) = self.recording_hotkey else {
            return;
        };
        if keystroke.key == "escape" {
            self.recording_hotkey = None;
            hotkey::set_paused(false);
            self.error = None;
            self.suppress_actions_until = Some(Instant::now() + Duration::from_millis(280));
            cx.notify();
            return;
        }
        let Some(chord) = hotkey::from_keystroke(keystroke) else {
            return;
        };
        if !chord.has_modifier() {
            self.error = Some(t("Use Ctrl, Alt, or Super as well.").into());
            cx.notify();
            return;
        }
        if let Err(err) = hotkey::install(slot, chord.clone()) {
            self.error = Some(tf("Could not use that shortcut: {}", &[&err]));
            cx.notify();
            return;
        }
        match slot {
            Shortcut::Show => self.show_hotkey = chord.canonical(),
            Shortcut::Record => self.record_hotkey = chord.canonical(),
        }
        self.recording_hotkey = None;
        self.error = None;
        self.suppress_actions_until = Some(Instant::now() + Duration::from_millis(280));
        self.persist();
        hotkey::set_paused(false);
        cx.notify();
    }

    pub(crate) fn close_overlay(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        if self.recording_hotkey.is_some() {
            self.recording_hotkey = None;
            hotkey::set_paused(false);
            self.error = None;
            self.suppress_actions_until = Some(Instant::now() + Duration::from_millis(280));
            cx.notify();
            return;
        }
        if self.actions_suppressed() {
            return;
        }
        if self.menu_open {
            self.menu_open = false;
        } else if matches!(self.phase, Phase::Result(_)) {
            self.phase = Phase::Idle;
            self.copied = false;
        } else if matches!(self.phase, Phase::Idle) {
            self.set_visible(false, window, cx);
        }
        self.error = None;
        self.recovery = None;
        self.failed_audio = None;
        self.snap_chrome();
        cx.notify();
    }

    pub(crate) fn copy_result(&mut self, cx: &mut Context<Self>) {
        let Phase::Result(text) = &self.phase else {
            return;
        };
        if self.result_kind == ResultKind::Command {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        self.copied = true;
        cx.notify();
    }
    pub(crate) fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.recovery = None;
        self.snap_chrome();
        cx.notify();
    }
    pub(crate) fn cancel_hotkey_capture(&mut self, cx: &mut Context<Self>) {
        if self.recording_hotkey.is_some() {
            self.stop_recording();
            self.error = None;
            cx.notify();
        }
    }

    pub(super) fn actions_suppressed(&self) -> bool {
        self.suppress_actions_until
            .is_some_and(|until| Instant::now() < until)
    }

    pub(super) fn persist(&self) {
        settings::save(&Preferences {
            onboarding_complete: settings::load().onboarding_complete,
            app_language: settings::load().app_language,
            selected: self.selected.clone(),
            language: self.language.clone(),
            output_language: self.output_language.clone(),
            show_hotkey: self.show_hotkey.clone(),
            record_hotkey: self.record_hotkey.clone(),
            copy_notes: self.copy_notes,
            clean_fillers: self.clean_fillers,
            input_device: self.input_device.clone(),
            open_on_startup: self.open_on_startup,
            show_in_menu_bar: settings::load().show_in_menu_bar,
            voice_commands: self.voice_commands,
            screen_context: self.screen_context,
            gateway_model: cloud::gateway_model().id().into(),
        });
    }

    pub(super) fn start_download(&mut self, spec: &'static ModelSpec, cx: &mut Context<Self>) {
        if self
            .download
            .as_ref()
            .is_some_and(|download| download.id == spec.id)
        {
            return;
        }
        if let Some(current) = &self.download {
            current.cancel.store(true, Ordering::Relaxed);
        }

        let received = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let id = spec.id.to_string();
        self.download = Some(Download {
            id: id.clone(),
            received: Arc::clone(&received),
            total: spec.bytes,
            cancel: Arc::clone(&cancel),
        });
        cx.notify();

        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { models::download(spec, &received, &cancel) })
                .await;
            this.update(cx, |view, cx| {
                let current = view.download.as_ref().map(|download| download.id.clone());
                if current.as_deref() == Some(spec.id) {
                    view.download = None;
                }
                match outcome {
                    Ok(_) => {
                        view.refresh_models();
                        view.selected = spec.id.to_string();
                        models::save_selected(spec.id);
                        view.error = None;
                    }
                    Err(err) if err == "Download cancelled" => {}
                    Err(err) => view.error = Some(err),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    #[cfg(updates)]
    pub(super) fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        if !cfg!(feature = "licensing") {
            return;
        }
        self.last_update_check = Instant::now();
        if !updater::is_packaged() || self.update_checking {
            return;
        }
        let prepared_version = self.update.as_ref().map(|update| update.version.clone());
        self.update_checking = true;
        tray::set_update(tray::UpdateStatus::Checking, cx);
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { updater::check_and_prepare(prepared_version) })
                .await;
            this.update(cx, |view, cx| {
                view.update_checking = false;
                let failed = match outcome {
                    Ok(Some(update)) => {
                        view.update = Some(update);
                        view.update_prompt = Some(UpdatePrompt::Ready);
                        false
                    }
                    Ok(None) => false,
                    // The menu bar and About page offer a retry; a failed
                    // background check never interrupts the bar.
                    Err(err) => {
                        eprintln!("Whisple update check failed: {err}");
                        true
                    }
                };
                let version = view
                    .update
                    .as_ref()
                    .map(|update| update.version.to_string());
                let menu_status = match (version.as_deref(), failed) {
                    (Some(version), _) => tray::UpdateStatus::Available(version),
                    (None, true) => tray::UpdateStatus::Error,
                    (None, false) => tray::UpdateStatus::UpToDate,
                };
                tray::set_update(menu_status, cx);
                view.snap_chrome();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    #[cfg(updates)]
    pub(super) fn show_or_check_for_updates(&mut self, cx: &mut Context<Self>) {
        if !cfg!(feature = "licensing") {
            cx.open_url("https://github.com/lassejlv/whisple/releases");
            return;
        }
        if self.update.is_none() {
            self.check_for_updates(cx);
            return;
        }
        self.update_prompt = Some(UpdatePrompt::Ready);
        self.snap_chrome();
        cx.notify();
    }
}
