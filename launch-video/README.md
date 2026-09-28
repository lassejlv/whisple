# Whisple launch film

A 30-second, 1920×1080 launch video built with [Remotion](https://www.remotion.dev). Narration, the dictated lines, sound effects and music come from ElevenLabs.

All app footage is real. Whisple (release build, `GPUI_X11_SCALE_FACTOR=2`) was recorded while ElevenLabs voices played into a PulseAudio virtual microphone, and the local Turbo model transcribed them on-device. Voice bar takes were shot on a green desktop and keyed into transparent clips and stills in `public/footage/`. The voice command take is a plain screen recording of the bar opening github.com in the browser.

The feature beats (filler cleanup, auto-detected languages, a voice command and the model menu) were filmed on a build with the fixes from [#25](https://github.com/lassejlv/whisple/pull/25), which the filler cleanup and 2× voice bar depend on.

```sh
npm install
npm run studio            # preview and scrub the timeline
./scripts/master.sh       # render and master to out/whisple-launch-final.mp4
```

`master.sh` needs `ffmpeg` and `jq`. To regenerate the audio, run `ELEVENLABS_API_KEY=… ./scripts/generate-audio.sh`. New takes change the word timings used in `src/timeline.ts` and the scenes.
