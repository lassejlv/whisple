import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { Glow } from "../components/Atmosphere";
import { useExit, Words } from "../components/Reveal";
import { VO } from "../timeline";
import { color, fontFamily } from "../theme";

// "You think faster than you type." Word offsets from speech-to-text on vo_1.
const at = (s: number) => VO.intro + Math.round(s * 30);

export const Intro: React.FC = () => {
  const frame = useCurrentFrame();
  const exit = useExit(98, 18);
  const breathe = 0.16 + Math.sin(frame / 18) * 0.03;
  const drift = interpolate(frame, [0, 118], [1, 1.045]);

  return (
    <AbsoluteFill style={{ background: "#09090b", alignItems: "center", justifyContent: "center" }}>
      <Glow size={1300} opacity={breathe} y="54%" />
      <div
        style={{
          fontFamily,
          fontSize: 104,
          fontWeight: 600,
          letterSpacing: "-0.045em",
          color: color.label,
          transform: `scale(${drift})`,
          ...exit,
        }}
      >
        <Words
          words={[
            { text: "You", at: at(0.1) },
            { text: "think", at: at(0.24) },
            {
              text: "faster",
              at: at(0.52),
              style: {
                background: "linear-gradient(180deg, #ffd28a 0%, #ffb340 60%, #f08a24 100%)",
                WebkitBackgroundClip: "text",
                backgroundClip: "text",
                color: "transparent",
              },
            },
            { text: "than", at: at(0.94) },
            { text: "you", at: at(1.12) },
            { text: "type.", at: at(1.26) },
          ]}
        />
      </div>
    </AbsoluteFill>
  );
};
