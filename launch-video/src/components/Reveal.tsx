import { Easing, interpolate, useCurrentFrame } from "remotion";
import { ease } from "../theme";

const out = Easing.bezier(...ease.out);

/** Soft focus-pull entrance used for every headline: blur, rise and fade. */
export const useReveal = (at: number, duration = 22, rise = 18, focus = 14) => {
  const frame = useCurrentFrame();
  const p = interpolate(frame, [at, at + duration], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: out,
  });
  return {
    opacity: p,
    filter: blur((1 - p) * focus),
    transform: p < 1 ? `translateY(${(1 - p) * rise}px)` : "none",
  } satisfies React.CSSProperties;
};

// A settled element keeps no filter at all, so headless Chrome does not hold
// it on its own compositing layer.
export const blur = (px: number) => (px > 0.05 ? `blur(${px}px)` : "none");

export const useExit = (at: number, duration = 16) => {
  const frame = useCurrentFrame();
  const p = interpolate(frame, [at, at + duration], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: Easing.bezier(...ease.inOut),
  });
  return {
    opacity: 1 - p,
    filter: blur(p * 12),
  } satisfies React.CSSProperties;
};

export const Words: React.FC<{
  words: { text: string; at: number; style?: React.CSSProperties }[];
  style?: React.CSSProperties;
}> = ({ words, style }) => (
  <span style={style}>
    {words.map((w, i) => (
      <Word key={i} at={w.at} style={w.style}>
        {w.text}
        {i < words.length - 1 ? "\u00a0" : ""}
      </Word>
    ))}
  </span>
);

const Word: React.FC<{ at: number; style?: React.CSSProperties; children: React.ReactNode }> = ({
  at,
  style,
  children,
}) => {
  const reveal = useReveal(at, 20, 14);
  return <span style={{ display: "inline-block", ...reveal, ...style }}>{children}</span>;
};
