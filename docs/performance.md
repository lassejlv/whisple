# Performance notes

## Changes

- Local Whisper contexts unload when switching models, selecting cloud, removing a
  model, restarting setup, or after five minutes without local inference. A
  replacement frees the previous context before loading. Generation checks keep
  delayed cleanup from evicting a newer context.
- Dictation focus and paste delays use async timers on the foreground executor,
  retaining native thread affinity. Insertion is cancelled with its owning view
  or a revoked license. Windows rechecks the destination after clipboard waits.
- macOS builds enable Whisper's Metal backend, with CPU fallback if GPU context
  creation fails.
- Hotkeys, tray commands, outside clicks, and display changes wake UI listeners.
  License/update checks wait for their deadlines. Idle Settings and onboarding
  have no periodic timer; the visible Audio meter still refreshes at 30 Hz.
  Linux waits on X11 and a registration wake socket, and reuses its placement
  connection and window lookup.
- Audio callbacks downmix and measure levels directly from the device format.
  Recording uses a streaming low-pass resampler and reserves a 60-second, 16 kHz
  mono buffer. Monitoring stores no recording. Transcription borrows existing
  16 kHz PCM.
- Cloud transcription and assistant requests share an HTTP client. Multipart
  retries share WAV storage rather than copying the whole recording.
- Model download progress refreshes at most every 100 ms when bytes change;
  downloads alone no longer request an animation frame continuously.
- macOS screen context captures and scales in memory through ScreenCaptureKit,
  with CoreGraphics fallback on older systems. The app no longer launches
  `screencapture`/`sips` or writes temporary screenshots for this path.

## Measurements

Measured on an Apple M4 on 2026-09-28 using optimized builds. These are local
probes, not a representative workload or a battery-life measurement.

| Probe | Result |
| --- | --- |
| Turbo Q5, CPU, four threads | 2916, 3236, 2900 ms per inference |
| Turbo Q5, Metal, four CPU threads | 1520, 1188, 1191 ms per inference |
| Audio monitor, 6,000 simulated callbacks | 0 callback allocations; 8.2 ms total |
| Audio recording, 6,000 simulated callbacks | 0 callback allocations; 53.4 ms total |
| Recording sample capacity | 3,840,000 bytes for 60 seconds |
| Native screenshot | 390.5 ms; 1440 × 936 PNG, 518,517 bytes |

The inference probe used the same 4.98-second English speech WAV, model,
four-thread greedy decoding, and parameters for both backends. Each backend
loaded once, then transcribed three times. Both returned the same text. Warm
Metal inference was approximately 2.5 times faster. Model load times are omitted
because filesystem cache state differed between runs.

The audio probe supplied 60 seconds of 48 kHz integer stereo in 480-frame blocks,
using an allocation-counting allocator after setup. The callback used the actual
capture code; the measurement excludes device I/O and startup. The reported
capacity covers sample storage, not the entire process or resampler coefficients.

## Verification and limits

- Default and licensed Rust test suites pass, including resampling duration/tail,
  callback boundaries, passband and alias rejection, bounded recording storage,
  event broadcasts, clipboard restoration, and HTTP connection reuse.
- A real-model cache probe exercised reuse, invalidation, stale-cleanup
  protection, idle expiry, and a failed replacement. Idle expiry was simulated
  by aging its timestamp.
- The native screenshot probe produced a correctly oriented image. Isolated live
  macOS QA exercised recording/stopping, Audio/Models navigation, and a completed
  Quick Preview download. UI automation sometimes returned stale window images.
- Arbitrary-editor paste was not verified end to end in this QA build: its
  Accessibility permission was unavailable. No new permission was granted.
- Changed Linux and Windows native modules compile in minimal cross-target
  harnesses. Full applications and native interactions on those OSes still need
  platform QA. Older-macOS screenshot fallback and Intel Mac performance were
  not measured.
