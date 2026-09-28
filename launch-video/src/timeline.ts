export const FPS = 30;
export const DURATION = 900;

export const sec = (s: number) => Math.round(s * FPS);

// Scene windows, in frames. Neighbouring scenes overlap for cross-fades.
export const INTRO = { from: 0, to: 118 };
export const DEMO = { from: 100, to: 556 };
export const FEATURES = { from: 534, to: 772 };
export const END = { from: 752, to: DURATION };

// The voice bar footage is a real take of Whisple transcribing on-device.
// hud_record.webm starts 1.0 s into the take and hud_result.webm 0.47 s
// before the result panel opens.
export const HUD = {
  appear: 180,
  // Idle bar, shortcut press at clip 0.6 s, a beat of recording.
  seg1: { at: 180, clipFrom: 0, clipTo: sec(1.25) },
  // Resume just before speech; the stop press and "Transcribing…" follow.
  seg2: { at: 217, clipFrom: sec(2.5), clipTo: sec(7.9) },
  // X11 blanks the window for a few frames while it grows, so the result
  // starts on the first clean frame of the open panel.
  result: { at: 379, clipFrom: 17 },
  press: 198,
  stop: 344,
  panelOpen: 379,
};

export const DICTATION_AT = 218;

export const MESSAGE = {
  typeFrom: 392,
  typeTo: 422,
  send: 444,
  typingFrom: 454,
  reply: 478,
};

// Narration start frames. Word offsets come from ElevenLabs speech-to-text.
export const VO = {
  intro: 14,
  sayIt: 138,
  features: 552,
  end: 778,
};
