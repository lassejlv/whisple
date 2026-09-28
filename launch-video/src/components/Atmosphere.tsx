import { AbsoluteFill, Img, random, staticFile, useCurrentFrame } from "remotion";

export const Grain: React.FC<{ opacity?: number }> = ({ opacity = 0.05 }) => {
  const frame = useCurrentFrame();
  // Step the texture every other frame so the grain shimmers like film, not static.
  const step = Math.floor(frame / 2);
  const x = Math.floor(random(`gx${step}`) * 512);
  const y = Math.floor(random(`gy${step}`) * 512);
  return (
    <AbsoluteFill
      style={{
        backgroundImage: `url(${staticFile("noise.png")})`,
        backgroundPosition: `${x}px ${y}px`,
        opacity,
        mixBlendMode: "overlay",
        pointerEvents: "none",
      }}
    />
  );
};

export const Vignette: React.FC<{ strength?: number }> = ({ strength = 0.55 }) => (
  <AbsoluteFill
    style={{
      background: `radial-gradient(ellipse 75% 70% at 50% 50%, transparent 55%, rgba(0,0,0,${strength}) 100%)`,
      pointerEvents: "none",
    }}
  />
);

export const Glow: React.FC<{
  x?: string;
  y?: string;
  size?: number;
  color?: string;
  opacity?: number;
}> = ({ x = "50%", y = "50%", size = 900, color = "255,179,64", opacity = 0.18 }) => (
  // Painted as a frame-sized gradient: an oversized, offset element sometimes
  // leaves unpainted strips in headless Chrome.
  <AbsoluteFill
    style={{
      background: `radial-gradient(circle at ${x} ${y}, rgba(${color},${opacity}) 0px, rgba(${color},${
        opacity * 0.35
      }) ${size * 0.25}px, rgba(${color},0) ${size * 0.5}px)`,
      pointerEvents: "none",
    }}
  />
);

export const AppIcon: React.FC<{ size: number; style?: React.CSSProperties }> = ({
  size,
  style,
}) => (
  <Img
    src={staticFile("whisple-icon.png")}
    style={{ width: size, height: size, display: "block", ...style }}
  />
);
