import { Img, staticFile } from "remotion";
import { color, fontFamily } from "../theme";

export const TrafficLights: React.FC<{ size?: number }> = ({ size = 13 }) => (
  <div style={{ display: "flex", gap: size * 0.62 }}>
    {["#ff5f57", "#febc2e", "#28c840"].map((c) => (
      <div
        key={c}
        style={{
          width: size,
          height: size,
          borderRadius: "50%",
          background: c,
          boxShadow: "inset 0 0 0 0.5px rgba(0,0,0,0.25)",
        }}
      />
    ))}
  </div>
);

/** A macOS-style window around a real Whisple screenshot (captured at 2x). */
export const ScreenshotWindow: React.FC<{
  src: string;
  width: number;
  title?: string;
  children?: React.ReactNode;
  style?: React.CSSProperties;
}> = ({ src, width, title = "Whisple", children, style }) => {
  const bar = Math.round(width * 0.036);
  return (
    <div
      style={{
        width,
        borderRadius: width * 0.016,
        overflow: "hidden",
        background: color.hud,
        boxShadow:
          "0 0 0 1px rgba(255,255,255,0.09), 0 40px 120px rgba(0,0,0,0.55), 0 12px 40px rgba(0,0,0,0.35)",
        ...style,
      }}
    >
      <div
        style={{
          height: bar,
          display: "flex",
          alignItems: "center",
          padding: `0 ${bar * 0.5}px`,
          background: "#141416",
          borderBottom: `1px solid ${color.hairline}`,
          position: "relative",
        }}
      >
        <TrafficLights size={bar * 0.36} />
        <div
          style={{
            position: "absolute",
            inset: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            fontFamily,
            fontWeight: 600,
            fontSize: bar * 0.38,
            color: color.secondary,
          }}
        >
          {title}
        </div>
      </div>
      <div style={{ position: "relative" }}>
        <Img src={staticFile(src)} style={{ width: "100%", display: "block" }} />
        {children}
      </div>
    </div>
  );
};
