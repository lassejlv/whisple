# Whisple launch film

A 30-second, 1920×1080 launch video built with [Remotion](https://www.remotion.dev). Narration, the dictated line, sound effects and music come from ElevenLabs.

The voice bar in the demo is real footage. Whisple (release build, `GPUI_X11_SCALE_FACTOR=2`) was recorded on a green desktop while an ElevenLabs voice played into a PulseAudio virtual microphone. Turbo transcribed it on-device, and the green was keyed out into `public/footage/*.webm`. The onboarding screenshots in `public/shots/` are captures of the same build.

```sh
npm install
npm run studio            # preview and scrub the timeline
./scripts/master.sh       # render and master to out/whisple-launch-final.mp4
```

`master.sh` needs `ffmpeg` and `jq`. To regenerate the audio, run `ELEVENLABS_API_KEY=… ./scripts/generate-audio.sh`. New takes change the word timings used in `src/timeline.ts` and the scenes.
