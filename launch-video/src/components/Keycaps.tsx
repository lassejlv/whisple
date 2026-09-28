import { interpolate, spring, useCurrentFrame, useVideoConfig } from "remotion";
import { color, fontFamily } from "../theme";

/** The ⌃ ⌥ Space shortcut, styled like the keys in Whisple's onboarding. */
export const Keycaps: React.FC<{ press: number; size?: number }> = ({ press, size = 76 }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const keys = [
    { label: "⌃", w: size },
    { label: "⌥", w: size },
    { label: "Space", w: size * 1.9 },
  ];
  return (
    <div style={{ display: "flex", gap: size * 0.18 }}>
      {keys.map((k, i) => {
        const down = spring({
          frame: frame - press - i * 2,
          fps,
          config: { damping: 14, stiffness: 260 },
          durationInFrames: 10,
        });
        const up = spring({
          frame: frame - press - 9 - i * 2,
          fps,
          config: { damping: 16, stiffness: 180 },
        });
        const d = Math.max(0, down - up);
        const lit = interpolate(d, [0, 1], [0, 1]);
        return (
          <div
            key={k.label}
            style={{
              width: k.w,
              height: size,
              borderRadius: size * 0.2,
              background: `linear-gradient(180deg, ${color.raised}, #1c1c20)`,
              boxShadow: `0 0 0 1px rgba(255,255,255,${0.08 + lit * 0.18}), 0 ${6 - d * 4}px ${
                18 - d * 10
              }px rgba(0,0,0,0.45), inset 0 1px 0 rgba(255,255,255,0.07), 0 0 ${lit * 36}px rgba(255,179,64,${
                lit * 0.35
              })`,
              transform: `translateY(${d * 4}px)`,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              fontFamily,
              fontWeight: 600,
              fontSize: k.label.length > 1 ? size * 0.3 : size * 0.4,
              color: lit > 0.3 ? color.amber : color.label,
            }}
          >
            {k.label}
          </div>
        );
      })}
    </div>
  );
};
