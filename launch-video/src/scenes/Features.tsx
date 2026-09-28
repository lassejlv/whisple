import {
  AbsoluteFill,
  Easing,
  Img,
  interpolate,
  OffthreadVideo,
  Sequence,
  spring,
  staticFile,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { Glow, Grain } from "../components/Atmosphere";
import { blur, useExit, useReveal } from "../components/Reveal";
import { Wallpaper } from "../components/Wallpaper";
import { BEAT, COMMAND, END, FEATURES, SAY, sec } from "../timeline";
import { color, ease, fontFamily } from "../theme";

const clamp = { extrapolateLeft: "clamp", extrapolateRight: "clamp" } as const;
const out = Easing.bezier(...ease.out);
const inOut = Easing.bezier(...ease.inOut);

type Span = { from: number; to: number };

const CARD_W = 1120;
const CARD_H = 700;
// Captures are 2x; the bar is 116 px tall and the result stills 800 x 351.
const BAR_H = 116;
const RESULT_H = 351;
// Every capture's bottom row shows the desktop taskbar through the window.
const EDGE = 1.2;

/** Fades a beat's layer in at its start and out at its end. */
const useLayer = (span: Span, last = false) => {
  const frame = useCurrentFrame();
  const inP = interpolate(frame, [span.from - 4, span.from + 10], [0, 1], { ...clamp, easing: out });
  const outP = last ? 0 : interpolate(frame, [span.to - 8, span.to + 6], [0, 1], { ...clamp, easing: inOut });
  return inP * (1 - outP);
};

/** Eases a panel up out of the bar, the way the macOS window grows. */
const usePanelReveal = (at: number, height: number, bar: number) => {
  const frame = useCurrentFrame();
  const inset = interpolate(frame, [at, at + 13], [height - bar, 0], { ...clamp, easing: out });
  return `inset(${inset}px 0 ${EDGE}px 0 round 30px 30px 0 0)`;
};

const Copy: React.FC<{ span: Span; eyebrow: string; title: string; sub: string; last?: boolean }> = ({
  span,
  eyebrow,
  title,
  sub,
  last,
}) => {
  const frame = useCurrentFrame();
  const kicker = useReveal(span.from + 2, 20, 10);
  const head = useReveal(span.from + 6, 22, 18);
  const body = useReveal(span.from + 12, 22, 12, 0);
  const exit = useExit(span.to - 10, 12);
  if (frame < span.from - 2 || (!last && frame > span.to + 4)) {
    return null;
  }
  return (
    <div
      style={{
        position: "absolute",
        left: 116,
        top: "50%",
        width: 560,
        transform: "translateY(-50%)",
        fontFamily,
        ...(last ? {} : exit),
      }}
    >
      <div
        style={{
          ...kicker,
          fontSize: 22,
          fontWeight: 600,
          letterSpacing: "0.12em",
          textTransform: "uppercase",
          color: color.amber,
        }}
      >
        {eyebrow}
      </div>
      <div
        style={{
          ...head,
          marginTop: 22,
          fontSize: 78,
          fontWeight: 600,
          letterSpacing: "-0.045em",
          lineHeight: 1.04,
          color: color.label,
        }}
      >
        {title}
      </div>
      <div
        style={{
          ...body,
          marginTop: 26,
          fontSize: 29,
          fontWeight: 500,
          letterSpacing: "-0.015em",
          lineHeight: 1.35,
          color: color.secondary,
        }}
      >
        {sub}
      </div>
    </div>
  );
};

const HudClip: React.FC<{
  src: string;
  width: number;
  height: number;
  trimBefore?: number;
}> = ({ src, width, height, trimBefore = 0 }) => (
  <OffthreadVideo
    src={staticFile(src)}
    transparent
    muted
    trimBefore={trimBefore}
    style={{ width, height, display: "block", clipPath: `inset(0 0 ${EDGE}px 0)` }}
  />
);

// "So, um, I, I think we should ship it on Friday", timed from speech-to-text.
const SPOKEN: { text: string; start: number; end: number; filler?: boolean }[] = [
  { text: "So,", start: 0.12, end: 0.28 },
  { text: "um,", start: 0.46, end: 0.66, filler: true },
  { text: "I,", start: 0.82, end: 0.88, filler: true },
  { text: "I", start: 1.02, end: 1.08 },
  { text: "think", start: 1.08, end: 1.2 },
  { text: "we", start: 1.24, end: 1.3 },
  { text: "should", start: 1.32, end: 1.46 },
  { text: "ship", start: 1.5, end: 1.64 },
  { text: "it", start: 1.66, end: 1.7 },
  { text: "on", start: 1.74, end: 1.86 },
  { text: "Friday.", start: 1.88, end: 2.26 },
];

const SpokenWord: React.FC<{ word: (typeof SPOKEN)[number] }> = ({ word }) => {
  const frame = useCurrentFrame();
  const at = SAY.clean + sec(word.start);
  const shown = interpolate(frame, [at - 2, at + 6], [0, 1], clamp);
  const strike = word.filler
    ? interpolate(frame, [SAY.clean + sec(word.end) + 4, SAY.clean + sec(word.end) + 14], [0, 1], {
        ...clamp,
        easing: out,
      })
    : 0;
  return (
    <span
      style={{
        position: "relative",
        display: "inline-block",
        marginRight: "0.28em",
        opacity: shown * (1 - strike * 0.6),
        transform: `translateY(${(1 - shown) * 8}px)`,
        color: word.filler ? color.amber : color.label,
      }}
    >
      {word.text}
      {word.filler ? (
        <span
          style={{
            position: "absolute",
            left: -2,
            top: "54%",
            height: 3,
            width: `calc(${strike * 100}% + 4px)`,
            borderRadius: 2,
            background: color.amber,
          }}
        />
      ) : null}
    </span>
  );
};

const CleanBeat: React.FC = () => {
  const frame = useCurrentFrame();
  const opacity = useLayer(BEAT.clean);
  const resultAt = SAY.clean + sec(2.26) + 6;
  const reveal = usePanelReveal(resultAt, RESULT_H, BAR_H);
  const label = useReveal(SAY.clean - 8, 16, 8, 0);
  if (opacity <= 0) {
    return null;
  }
  return (
    <AbsoluteFill style={{ opacity }}>
      <Wallpaper />
      <div
        style={{
          position: "absolute",
          top: 88,
          width: "100%",
          textAlign: "center",
          fontFamily,
        }}
      >
        <div style={{ ...label, fontSize: 20, fontWeight: 600, letterSpacing: "0.1em", color: "rgba(255,255,255,0.55)" }}>
          YOU SAY
        </div>
        <div
          style={{
            marginTop: 18,
            fontSize: 44,
            fontWeight: 500,
            letterSpacing: "-0.02em",
          }}
        >
          {SPOKEN.map((word) => (
            <SpokenWord key={word.text + word.start} word={word} />
          ))}
        </div>
      </div>
      <div style={{ position: "absolute", left: 150, bottom: 40, width: 820, height: 378 }}>
        {frame < resultAt ? (
          <Sequence from={SAY.clean - 4} layout="none">
            <HudClip src="footage/hud_record.webm" width={820} height={378} trimBefore={sec(2.6)} />
          </Sequence>
        ) : (
          <Img
            src={staticFile("footage/result_clean.png")}
            style={{ position: "absolute", left: 10, bottom: 1, width: 800, clipPath: reveal }}
          />
        )}
      </div>
    </AbsoluteFill>
  );
};

const LANGUAGES = [
  { src: "footage/result_da.png", name: "Dansk", at: SAY.danish + 4, x: 60, y: 54 },
  { src: "footage/result_es.png", name: "Español", at: SAY.danish + 22, x: 300, y: 250 },
  { src: "footage/result_de.png", name: "Deutsch", at: SAY.danish + 40, x: 540, y: 446 },
];

const LanguageCard: React.FC<(typeof LANGUAGES)[number]> = ({ src, name, at, x, y }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const p = spring({ frame: frame - at, fps, config: { damping: 18, stiffness: 150 } });
  const scale = 0.62;
  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        opacity: p,
        transform: `translateY(${(1 - p) * 26}px) scale(${0.95 + p * 0.05})`,
        transformOrigin: "left bottom",
        filter: "drop-shadow(0 24px 48px rgba(0,0,0,0.45))",
      }}
    >
      <Img src={staticFile(src)} style={{ width: 800 * scale, display: "block", clipPath: `inset(0 0 ${EDGE * scale}px 0)` }}
      />
      <div
        style={{
          position: "absolute",
          left: 18,
          top: -18,
          padding: "7px 16px",
          borderRadius: 999,
          background: "rgba(255,179,64,0.16)",
          border: "1px solid rgba(255,179,64,0.45)",
          color: color.amber,
          fontFamily,
          fontSize: 19,
          fontWeight: 600,
          backdropFilter: "blur(12px)",
        }}
      >
        {name}
      </div>
    </div>
  );
};

const LanguagesBeat: React.FC = () => {
  const opacity = useLayer(BEAT.languages);
  if (opacity <= 0) {
    return null;
  }
  return (
    <AbsoluteFill style={{ opacity }}>
      <Wallpaper />
      {LANGUAGES.map((card) => (
        <LanguageCard key={card.name} {...card} />
      ))}
    </AbsoluteFill>
  );
};

const CommandBeat: React.FC = () => {
  const frame = useCurrentFrame();
  const opacity = useLayer(BEAT.command);
  const push = interpolate(frame, [COMMAND.resultAt, BEAT.command.to], [1, 1.16], { ...clamp, easing: inOut });
  if (opacity <= 0) {
    return null;
  }
  // The screen recording is 1920 x 1172 and the taskbar shows through below
  // the bar (rows 1136 on) until the browser covers it, so frame above it.
  const fit = CARD_H / 1136;
  const video: React.CSSProperties = {
    position: "absolute",
    left: (CARD_W - 1920 * fit) / 2,
    top: 0,
    width: 1920 * fit,
    height: 1172 * fit,
    display: "block",
  };
  return (
    <AbsoluteFill style={{ opacity, background: "#000" }}>
      <AbsoluteFill style={{ transform: `scale(${push})`, transformOrigin: "50% 100%" }}>
        {frame < COMMAND.resultAt ? (
          <Sequence from={SAY.command - 6} layout="none">
            <OffthreadVideo src={staticFile("footage/cmd_record.mp4")} muted style={video} />
          </Sequence>
        ) : frame < COMMAND.panelAt ? (
          <Sequence from={COMMAND.resultAt} layout="none">
            <OffthreadVideo
              src={staticFile("footage/cmd_result.mp4")}
              muted
              trimBefore={COMMAND.resultClipFrom}
              style={video}
            />
          </Sequence>
        ) : (
          <Sequence from={COMMAND.panelAt} layout="none">
            <OffthreadVideo
              src={staticFile("footage/cmd_result.mp4")}
              muted
              trimBefore={COMMAND.panelClipFrom}
              style={video}
            />
          </Sequence>
        )}
      </AbsoluteFill>
    </AbsoluteFill>
  );
};

// menu.webm starts 1.0 s into the take; the menu is fully drawn from 1.13 s.
const MENU_SCALE = 0.9;

const ModelsBeat: React.FC = () => {
  const opacity = useLayer(BEAT.models, true);
  const reveal = usePanelReveal(BEAT.models.from + 2, 737 * MENU_SCALE, BAR_H * MENU_SCALE);
  if (opacity <= 0) {
    return null;
  }
  return (
    <AbsoluteFill style={{ opacity }}>
      <Wallpaper />
      <div
        style={{
          position: "absolute",
          left: (CARD_W - 900 * MENU_SCALE) / 2,
          bottom: 40,
          width: 900 * MENU_SCALE,
          height: 737 * MENU_SCALE,
          clipPath: reveal,
        }}
      >
        <Sequence from={BEAT.models.from} layout="none">
          <HudClip
            src="footage/menu.webm"
            width={900 * MENU_SCALE}
            height={737 * MENU_SCALE}
            trimBefore={34}
          />
        </Sequence>
      </div>
    </AbsoluteFill>
  );
};

const beats = [BEAT.clean, BEAT.languages, BEAT.command, BEAT.models];

const Progress: React.FC = () => {
  const frame = useCurrentFrame();
  return (
    <div style={{ display: "flex", gap: 10, justifyContent: "center", marginTop: 26 }}>
      {beats.map((span, i) => {
        const on = interpolate(frame, [span.from - 4, span.from + 8, span.to - 6, span.to + 6], [0, 1, 1, 0], clamp);
        const keep = i === beats.length - 1 ? interpolate(frame, [span.from - 4, span.from + 8], [0, 1], clamp) : on;
        return (
          <div
            key={i}
            style={{
              width: 8 + keep * 22,
              height: 8,
              borderRadius: 4,
              background: keep > 0.5 ? color.amber : "rgba(255,255,255,0.22)",
            }}
          />
        );
      })}
    </div>
  );
};

export const Features: React.FC = () => {
  const frame = useCurrentFrame();
  const enter = interpolate(frame, [FEATURES.from, FEATURES.from + 22], [0, 1], clamp);
  const settle = interpolate(frame, [FEATURES.from, FEATURES.from + 40], [0, 1], { ...clamp, easing: out });
  const drift = interpolate(frame, [FEATURES.from, END.from + 20], [0, 1]);

  return (
    <AbsoluteFill style={{ background: "#0a0a0c", opacity: enter }}>
      <Glow x="68%" y="50%" size={1600} opacity={0.12} />
      <Glow x="10%" y="92%" size={900} color="196,84,120" opacity={0.07} />

      <Copy
        span={BEAT.clean}
        eyebrow="Clean up"
        title="Talk like you talk."
        sub="The ums and stutters are gone before you paste."
      />
      <Copy
        span={BEAT.languages}
        eyebrow="Languages"
        title="Speaks your language."
        sub="Twenty languages, detected as you speak."
      />
      <Copy
        span={BEAT.command}
        eyebrow="Voice commands"
        title="Just ask."
        sub="Say “Go to github.com” and it opens."
      />
      <Copy
        span={BEAT.models}
        eyebrow="Private by default"
        title="Your model, your call."
        sub="Pick an on-device model, or bring your own cloud key."
        last
      />

      <div
        style={{
          position: "absolute",
          right: 80,
          top: "50%",
          transform: `translateY(calc(-50% + ${(1 - settle) * 40}px)) translateY(${Math.sin(drift * Math.PI * 2) * 4}px)`,
          opacity: settle,
          filter: blur((1 - settle) * 14),
        }}
      >
        <div
          style={{
            position: "relative",
            width: CARD_W,
            height: CARD_H,
            borderRadius: 30,
            overflow: "hidden",
            background: "#120e14",
            boxShadow:
              "0 0 0 1px rgba(255,255,255,0.1), 0 50px 120px rgba(0,0,0,0.55), 0 14px 40px rgba(0,0,0,0.35)",
          }}
        >
          <CleanBeat />
          <LanguagesBeat />
          <CommandBeat />
          <ModelsBeat />
        </div>
        <Progress />
      </div>
      <Grain opacity={0.04} />
    </AbsoluteFill>
  );
};
