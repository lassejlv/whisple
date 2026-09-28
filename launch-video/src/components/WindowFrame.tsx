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
