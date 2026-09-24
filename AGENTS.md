# Repository Guidelines

## Project Structure & Module Organization

Whisple is a single Rust desktop binary. `src/main.rs` starts the GPUI app; `src/app.rs` coordinates recording and window state, and `src/ui.rs` renders the voice bar. The Settings window lives in `src/settings_window/`: `mod.rs` owns the window and sidebar, `pages/` has one file per page with its actions, and `widgets.rs` holds the shared rows and switches. Audio capture and transcription live in `src/audio.rs`, `src/stt.rs`, and `src/cloud.rs`. After transcription, `src/assistant.rs` routes a transcript to dictation, a voice command, or the assistant: `src/commands.rs` parses "Open Spotify" and "Hey Whisple, …", `src/apps.rs` finds and launches installed apps, and `src/screen.rs` reads the front app, window title, selection, and a screenshot. Platform hotkeys are under `src/hotkey/`; settings, startup behavior, tray controls, and updates have separate modules. Keep icons and bundled media in `assets/`, release tooling in `scripts/`, and release instructions in `docs/releasing.md`. Unit tests sit beside their implementation in `src/`.

## Build, Test, and Development Commands

- `cargo run` builds and launches the development app.
- `cargo test --locked` runs the Rust unit tests using the committed lockfile; the macOS release workflow runs this command.
- `cargo fmt --check` checks Rust formatting. Run `cargo fmt` before committing formatting changes.
- `cargo clippy --all-targets` checks common Rust issues.
- `./scripts/package-macos.sh --debug` builds a local debug `.app`; omit `--debug` for a release bundle. Packaging requires macOS tools such as `sips` and `iconutil`.

## Coding Style & Naming Conventions

Use Rust 2021 conventions and `rustfmt` defaults (four-space indentation). Name modules and functions in `snake_case`, types in `PascalCase`, and constants in `SCREAMING_SNAKE_CASE`. Keep platform-specific code behind `cfg(target_os = ...)` and preserve the separation between macOS and X11 hotkey implementations. Add new assets through the loader in `src/main.rs` when the UI needs them.

## Testing Guidelines

Add focused `#[test]` cases in a nearby `#[cfg(test)] mod tests`; use names that describe the behavior being checked. There is no stated coverage threshold. Run `cargo test --locked` for logic changes. For UI changes, also launch the app and exercise the affected controls, including both the setting row and its switch where applicable. Test platform-specific behavior on the relevant OS.

## Commit & Pull Request Guidelines

Recent commits use short, action-oriented subjects such as `Add cloud transcription...` or `Build Apple silicon releases...`; an occasional `feat:` prefix is also used. Keep each commit scoped. In pull requests, describe the behavior changed, include test commands and results, link related issues when applicable, and attach screenshots for visible UI changes. Follow `docs/releasing.md` for version, tag, and asset publication steps.

## Configuration & Secrets

Cloud provider keys are stored through the system credential store (`src/cloud.rs`). Do not commit API keys, recordings, generated bundles, or files from `target/`.
