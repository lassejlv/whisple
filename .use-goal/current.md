---
schema_version: 1
kind: "goal"
id: "b293e40c-a577-44f6-a00e-748e586f0748"
previous_goal_id: null
title: "Make Whisple reliable for new users"
status: "active"
started: "2026-09-25T11:42:46.162524Z"
updated: "2026-09-25T11:53:41.477568Z"
finished: null
revision: 2
workspace: "/Users/lassevestergaard/Dev/whispapp"
agent: "codex"
run_id: null
stop_reason: null
criteria_done: 0
criteria_total: 4
objective: "Verify and improve first launch, onboarding, everyday dictation, recovery, and relaunch in the running macOS app."
criteria: [{"text": "A clean isolated first launch completes onboarding, including microphone and transcription setup, and persists through relaunch.", "done": false, "evidence": []}, {"text": "Everyday recording, transcription, dictation, settings controls, and expected error/recovery paths work in the running macOS app without a crash or visible stall.", "done": false, "evidence": []}, {"text": "Reproducible first-run, lifecycle, and performance defects found in source or live testing are fixed and checked at the affected interaction paths.", "done": false, "evidence": []}, {"text": "Formatting, default and licensed Rust tests, Clippy, and macOS packaging pass after changes.", "done": false, "evidence": []}]
constraints: ["Preserve the user's existing app settings, credentials, and unrelated worktree changes.", "Treat clean first-run testing as isolated from the installed app.", "Report unverified hardware, service, and platform paths explicitly."]
progress: ["Isolated debug first launch used target/first-run-qa; onboarding reached the voice bar, downloaded the 31 MB Quick preview model, and saved onboarding_complete=true. Source fixes now make settings saves atomic and visible on failure, move credential-store reads/writes off the UI thread, and clarify the final onboarding step."]
next_action: "Rebuild the debug app, rerun isolated onboarding to verify accessible controls and the corrected final step, then exercise recording and relaunch."
blocker: null
extensions: {"native_goal_status": "active"}
archived: null
archive_reason: null
---

# Make Whisple reliable for new users

## Objective
Verify and improve first launch, onboarding, everyday dictation, recovery, and relaunch in the running macOS app.

## Definition of done
- [ ] A clean isolated first launch completes onboarding, including microphone and transcription setup, and persists through relaunch.
- [ ] Everyday recording, transcription, dictation, settings controls, and expected error/recovery paths work in the running macOS app without a crash or visible stall.
- [ ] Reproducible first-run, lifecycle, and performance defects found in source or live testing are fixed and checked at the affected interaction paths.
- [ ] Formatting, default and licensed Rust tests, Clippy, and macOS packaging pass after changes.

## Constraints
- Preserve the user's existing app settings, credentials, and unrelated worktree changes.
- Treat clean first-run testing as isolated from the installed app.
- Report unverified hardware, service, and platform paths explicitly.

## Progress
- Isolated debug first launch used target/first-run-qa; onboarding reached the voice bar, downloaded the 31 MB Quick preview model, and saved onboarding_complete=true. Source fixes now make settings saves atomic and visible on failure, move credential-store reads/writes off the UI thread, and clarify the final onboarding step.

## Next action
Rebuild the debug app, rerun isolated onboarding to verify accessible controls and the corrected final step, then exercise recording and relaunch.

## Blocker
None.
