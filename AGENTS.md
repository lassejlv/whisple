# Repository Guidelines

## Project Structure & Module Organization

Whisple is a single Rust desktop binary. `src/main.rs` starts the GPUI app. `src/app/` owns application state, recording, and actions; `src/audio/` captures audio; and `src/transcription/` contains local and cloud transcription, models, and text cleanup. `src/assistant/` routes transcripts to commands or the assistant and gathers app and screen context. `src/ui/` contains the voice bar, onboarding, theme, motion, and the Settings window in `src/ui/settings/`. `src/platform/` holds hotkeys and platform integrations, with native implementations under `macos/`, `linux/`, and `windows/`. The opt-in `licensing` feature builds `src/licensing/`, including Polar keys and the free trial confirmed by cloud.whisple.app. Settings, startup behavior, and updates remain separate root modules. Keep icons and bundled media in `assets/`, release tooling in `scripts/`, and release instructions in `docs/releasing.md`. Unit tests sit beside their implementation in `src/`.

## Build, Test, and Development Commands

- `cargo run` builds and launches the development app.
- `cargo test --locked` runs the default, license-free unit tests using the committed lockfile. Run `cargo test --locked --features licensing` for the licensed build; the macOS and Windows release workflows use this mode.
- `cargo fmt --check` checks Rust formatting. Run `cargo fmt` before committing formatting changes.
- `cargo clippy --all-targets` checks common Rust issues.
- `./scripts/package-macos.sh --debug` builds a local debug `.app`; omit `--debug` for a release bundle. Packaging requires macOS tools such as `sips` and `iconutil`.
- `scripts/windows/package-windows.ps1` builds the Windows installers, MSI, and update archive. It needs Inno Setup 6 and the WiX v5 .NET tool.

## Coding Style & Naming Conventions

Use Rust 2021 conventions and `rustfmt` defaults (four-space indentation). Name modules and functions in `snake_case`, types in `PascalCase`, and constants in `SCREAMING_SNAKE_CASE`. Keep platform-specific code behind `cfg(target_os = ...)` and preserve the separation between macOS and X11 hotkey implementations. Wrap every user-facing interface string in `t("…")` or `tf("… {} …", &[…])` from `src/i18n/mod.rs`, and add its translation to each table in `src/i18n/`; `cargo test` fails when a language is missing a string. Dictation, translation, and assistant output are not interface text. Add new assets through the loader in `src/main.rs` when the UI needs them.

## Testing Guidelines

Add focused `#[test]` cases in a nearby `#[cfg(test)] mod tests`; use names that describe the behavior being checked. There is no stated coverage threshold. Run `cargo test --locked` for logic changes. For UI changes, also launch the app and exercise the affected controls, including both the setting row and its switch where applicable. Test platform-specific behavior on the relevant OS.

## Commit & Pull Request Guidelines

Recent commits use short, action-oriented subjects such as `Add cloud transcription...` or `Build Apple silicon releases...`; an occasional `feat:` prefix is also used. Keep each commit scoped. In pull requests, describe the behavior changed, include test commands and results, link related issues when applicable, and attach screenshots for visible UI changes. Follow `docs/releasing.md` for version, tag, and asset publication steps.

## Configuration & Secrets

Cloud provider keys are stored through the system credential store (`src/transcription/cloud.rs`). Do not commit API keys, recordings, generated bundles, or files from `target/`.
