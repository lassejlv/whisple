import {
  AbsoluteFill,
  Easing,
  interpolate,
  spring,
  staticFile,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { AppIcon, Glow } from "../components/Atmosphere";
import { useReveal } from "../components/Reveal";
import { END, VO } from "../timeline";
import { color, ease, fontFamily } from "../theme";

// "Say hello to Whisple. Just talk." Word offsets from speech-to-text on vo_4.
const at = (s: number) => VO.end + Math.round(s * 30);
const clamp = { extrapolateLeft: "clamp", extrapolateRight: "clamp" } as const;

export const EndCard: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const enter = interpolate(frame, [END.from, END.from + 20], [0, 1], clamp);
  const pop = spring({ frame: frame - (END.from + 6), fps, config: { damping: 20, stiffness: 90 } });
  const bloom = interpolate(frame, [END.from, END.from + 60], [0, 1], {
    ...clamp,
    easing: Easing.bezier(...ease.out),
  });
  const sweep = interpolate(frame, [END.from + 18, END.from + 58], [-120, 220], clamp);
  const lift = interpolate(frame, [at(0.6), at(0.6) + 26], [0, 1], {
    ...clamp,
    easing: Easing.bezier(...ease.out),
  });

  const word = useReveal(at(0.66), 24, 18);
  const tag = useReveal(at(1.24), 22, 14, 0);
  const foot = useReveal(at(1.5), 22, 10, 0);
  const fadeOut = interpolate(frame, [END.to - 16, END.to], [1, 0], clamp);

  return (
    <AbsoluteFill style={{ background: "#08080a", opacity: enter * fadeOut }}>
      <Glow y="44%" size={1600} opacity={0.2 * bloom} />
      <AbsoluteFill style={{ alignItems: "center", justifyContent: "center" }}>
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            transform: `translateY(${(1 - lift) * 70}px)`,
          }}
        >
          <div
            style={{
              position: "relative",
              width: 210,
              height: 210,
              transform: `scale(${0.82 + pop * 0.18})`,
              opacity: pop,
              filter: `${pop < 0.995 ? `blur(${(1 - pop) * 12}px) ` : ""}drop-shadow(0 24px 60px rgba(255,179,64,0.25))`,
            }}
          >
            <AppIcon size={210} />
            <div
              style={{
                position: "absolute",
                inset: 0,
                background: `linear-gradient(105deg, transparent ${sweep - 30}%, rgba(255,255,255,0.5) ${sweep}%, transparent ${
                  sweep + 30
                }%)`,
                WebkitMaskImage: `url(${staticFile("whisple-icon.png")})`,
                WebkitMaskSize: "100% 100%",
                mixBlendMode: "overlay",
              }}
            />
          </div>
          <div
            style={{
              ...word,
              marginTop: 38,
              fontFamily,
              fontSize: 132,
              fontWeight: 700,
              letterSpacing: "-0.05em",
              color: color.label,
              lineHeight: 1,
            }}
          >
            Whisple
          </div>
          <div
            style={{
              ...tag,
              marginTop: 22,
              fontFamily,
              fontSize: 50,
              fontWeight: 500,
              letterSpacing: "-0.03em",
              color: color.secondary,
            }}
          >
            Just talk.
          </div>
        </div>
      </AbsoluteFill>
      <div
        style={{
          ...foot,
          position: "absolute",
          bottom: 78,
          width: "100%",
          textAlign: "center",
          fontFamily,
          fontSize: 28,
          fontWeight: 500,
          letterSpacing: "-0.01em",
          color: color.tertiary,
        }}
      >
        <span style={{ color: color.amber, fontWeight: 600 }}>whisple.app</span>
        <span style={{ margin: "0 18px" }}>·</span>
        For macOS and Windows
      </div>
    </AbsoluteFill>
  );
};
