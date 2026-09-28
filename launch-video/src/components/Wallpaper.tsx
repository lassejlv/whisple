import { AbsoluteFill, useCurrentFrame } from "remotion";

type Blob = { x: number; y: number; r: number; c: string; a: number; drift: number };

const blobs: Blob[] = [
  { x: 18, y: 88, r: 950, c: "255,170,60", a: 0.62, drift: 0.9 },
  { x: 82, y: 14, r: 1050, c: "196,84,120", a: 0.5, drift: 1.2 },
  { x: 62, y: 96, r: 800, c: "92,74,214", a: 0.36, drift: 0.7 },
  { x: 4, y: 10, r: 700, c: "60,40,90", a: 0.5, drift: 1.1 },
  { x: 50, y: 48, r: 600, c: "255,140,90", a: 0.16, drift: 1.5 },
];

/** A warm, slowly drifting gradient desktop in the spirit of macOS wallpapers. */
export const Wallpaper: React.FC = () => {
  const frame = useCurrentFrame();
  const t = frame / 30;
  return (
    <AbsoluteFill style={{ background: "#120e14", overflow: "hidden" }}>
      {blobs.map((b, i) => {
        const dx = Math.sin(t * 0.22 * b.drift + i) * 3;
        const dy = Math.cos(t * 0.18 * b.drift + i * 2) * 3;
        return (
          <div
            key={i}
            style={{
              position: "absolute",
              left: `${b.x + dx}%`,
              top: `${b.y + dy}%`,
              width: b.r,
              height: b.r,
              transform: "translate(-50%, -50%)",
              borderRadius: "50%",
              background: `radial-gradient(circle, rgba(${b.c},${b.a}) 0%, rgba(${b.c},0) 68%)`,
            }}
          />
        );
      })}
      <AbsoluteFill
        style={{
          background:
            "linear-gradient(180deg, rgba(10,8,12,0.1) 0%, rgba(10,8,12,0) 40%, rgba(10,8,12,0.35) 100%)",
        }}
      />
    </AbsoluteFill>
  );
};
