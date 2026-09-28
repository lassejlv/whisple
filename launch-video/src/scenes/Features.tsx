import { AbsoluteFill, Easing, interpolate, useCurrentFrame } from "remotion";
import { Glow, Grain } from "../components/Atmosphere";
import { blur, useExit, useReveal } from "../components/Reveal";
import { ScreenshotWindow } from "../components/WindowFrame";
import { FEATURES, VO } from "../timeline";
import { color, ease, fontFamily } from "../theme";

// Word offsets from speech-to-text on vo_3:
// "Whisple turns your voice into clean text, in any app, private, right on your computer."
const at = (s: number) => VO.features + Math.round(s * 30);
const BEAT_B = 652;

const clamp = { extrapolateLeft: "clamp", extrapolateRight: "clamp" } as const;
const out = Easing.bezier(...ease.out);
const inOut = Easing.bezier(...ease.inOut);

const headline: React.CSSProperties = {
  fontFamily,
  fontSize: 84,
  fontWeight: 600,
  letterSpacing: "-0.045em",
  lineHeight: 1.04,
  color: color.label,
};

const Line: React.FC<{ at: number; dim?: boolean; children: React.ReactNode }> = ({
  at,
  dim,
  children,
}) => <div style={{ ...useReveal(at, 22, 20), color: dim ? color.secondary : color.label }}>{children}</div>;

const Stage: React.FC<{ from: number; to?: number; children: React.ReactNode }> = ({
  from,
  to,
  children,
}) => {
  const frame = useCurrentFrame();
  const inP = interpolate(frame, [from, from + 30], [0, 1], { ...clamp, easing: out });
  const outP = to ? interpolate(frame, [to, to + 16], [0, 1], { ...clamp, easing: inOut }) : 0;
  const settle = interpolate(frame, [from, from + 120], [0, 1], { ...clamp, easing: out });
  return (
    <div
      style={{
        position: "absolute",
        right: 72,
        top: "50%",
        transform: `translateY(-50%) perspective(2200px) rotateY(${-16 + settle * 9}deg) rotateX(${
          5 - settle * 3
        }deg) translateX(${(1 - inP) * 90 - outP * 60}px) scale(${0.96 + settle * 0.04})`,
        transformOrigin: "100% 50%",
        opacity: inP * (1 - outP),
        filter: blur((1 - inP) * 14 + outP * 10),
      }}
    >
      {children}
    </div>
  );
};

export const Features: React.FC = () => {
  const frame = useCurrentFrame();
  const enter = interpolate(frame, [FEATURES.from, FEATURES.from + 22], [0, 1], clamp);
  const beatAOut = useExit(BEAT_B - 8, 14);
  const turboPulse = interpolate(frame, [BEAT_B + 40, BEAT_B + 60, BEAT_B + 100], [0, 1, 0.6], clamp);
  const logosIn = useReveal(at(4.9), 22, 12);

  return (
    <AbsoluteFill style={{ background: "#0a0a0c", opacity: enter }}>
      <Glow x="72%" y="52%" size={1500} opacity={0.14} />
      <Glow x="12%" y="90%" size={900} color="196,84,120" opacity={0.08} />

      <div style={{ position: "absolute", left: 136, top: "50%" }}>
        {frame < BEAT_B + 8 ? (
          <div style={{ ...headline, ...beatAOut, position: "absolute", top: 0, transform: "translateY(-50%)", whiteSpace: "nowrap" }}>
            <Line at={at(1.44)}>Clean text.</Line>
            <Line at={at(2.3)} dim>
              In any app.
            </Line>
          </div>
        ) : null}
        {frame >= BEAT_B - 4 ? (
          <div style={{ ...headline, position: "absolute", top: 0, transform: "translateY(-50%)", whiteSpace: "nowrap" }}>
            <Line at={at(3.5)}>Private.</Line>
            <Line at={at(4.15)} dim>
              On your computer.
            </Line>
            <div
              style={{
                ...logosIn,
                marginTop: 44,
                display: "flex",
                alignItems: "center",
                gap: 26,
                fontSize: 26,
                fontWeight: 500,
                letterSpacing: "-0.01em",
                color: color.secondary,
              }}
            >
              <span>
                Or bring your own key:{" "}
                <span style={{ color: color.label }}>OpenAI, Groq, xAI, Vercel</span>
              </span>
            </div>
          </div>
        ) : null}
      </div>

      <Stage from={FEATURES.from + 4} to={BEAT_B - 6}>
        <ScreenshotWindow src="shots/features.png" width={920} />
      </Stage>
      <Stage from={BEAT_B}>
        <ScreenshotWindow src="shots/models.png" width={920}>
          <div
            style={{
              position: "absolute",
              left: "19.4%",
              width: "61.2%",
              top: "40.6%",
              height: "6.4%",
              borderRadius: 10,
              boxShadow: `0 0 0 2px rgba(255,179,64,${turboPulse * 0.8}), 0 0 ${
                40 * turboPulse
              }px rgba(255,179,64,${turboPulse * 0.45})`,
            }}
          />
        </ScreenshotWindow>
      </Stage>
      <Grain opacity={0.05} />
    </AbsoluteFill>
  );
};
