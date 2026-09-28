export const FPS = 30;
export const DURATION = 900;

export const sec = (s: number) => Math.round(s * FPS);

// Scene windows, in frames. Neighbouring scenes overlap for cross-fades.
export const INTRO = { from: 0, to: 118 };
export const DEMO = { from: 96, to: 456 };
export const FEATURES = { from: 434, to: 818 };
export const END = { from: 800, to: DURATION };

// The voice bar footage is a real take of Whisple transcribing on-device.
// hud_record.webm starts 1.0 s into the take and hud_result.webm 0.47 s
// before the result panel opens.
export const HUD = {
  appear: 150,
  // Idle bar, shortcut press at clip 0.6 s, a beat of recording.
  seg1: { at: 150, clipFrom: 0 },
  // Resume just before speech; the stop press and "Transcribing…" follow.
  seg2: { at: 187, clipFrom: sec(2.5) },
  // X11 blanks the window for a few frames while it grows, so the result
  // starts on the first clean frame of the open panel.
  result: { at: 336, clipFrom: 17 },
  press: 168,
  stop: 314,
  panelOpen: 336,
};

export const DICTATION_AT = 188;

export const MESSAGE = {
  typeFrom: 348,
  typeTo: 372,
  send: 390,
  typingFrom: 398,
  reply: 412,
};

// Feature beats, each shown with real footage of the fixed release build.
export const BEAT = {
  clean: { from: 434, to: 544 },
  languages: { from: 544, to: 650 },
  command: { from: 650, to: 752 },
  models: { from: 752, to: 818 },
};

// Spoken lines that Whisple transcribes in the feature beats.
export const SAY = {
  clean: 446,
  danish: 552,
  command: 658,
};

// cmd_record.mp4 starts 3.0 s into the command take (speech at 0.37 s).
// cmd_result.mp4 starts 46.9 s in: result panel at 0.6 s, browser at 0.97 s.
// X11 tears the panel for its first five frames (18-22), so the take cuts
// from the last "Transcribing…" frame straight to the first clean one.
export const COMMAND = {
  resultAt: 708,
  resultClipFrom: 14,
  panelAt: 712,
  panelClipFrom: 23,
  browserOpens: 712 + sec(0.97) - 23,
};

// Narration start frames. Word offsets come from ElevenLabs speech-to-text.
export const VO = {
  intro: 14,
  sayIt: 110,
  end: 806,
};
