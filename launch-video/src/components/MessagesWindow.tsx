import { interpolate, spring, useCurrentFrame, useVideoConfig } from "remotion";
import { MESSAGE } from "../timeline";
import { color, fontFamily } from "../theme";
import { TrafficLights } from "./WindowFrame";

// Exactly what Whisple returned for the dictation in the recorded take.
export const DICTATED = "Hey, Maya, running 10 minutes late, grab us a table by the window.";

const FONT = 25;
const MINE_H = 104;
const REPLY_H = 62;
const GAP = 14;

const Bubble: React.FC<{
  mine?: boolean;
  children: React.ReactNode;
  style?: React.CSSProperties;
}> = ({ mine, children, style }) => (
  <div
    style={{
      position: "absolute",
      [mine ? "right" : "left"]: 28,
      maxWidth: 520,
      padding: "13px 20px",
      borderRadius: 24,
      [mine ? "borderBottomRightRadius" : "borderBottomLeftRadius"]: 8,
      background: mine ? color.amber : "#2c2c31",
      color: mine ? "#1c1305" : color.label,
      fontSize: FONT,
      lineHeight: 1.3,
      fontWeight: mine ? 500 : 400,
      letterSpacing: "-0.01em",
      ...style,
    }}
  >
    {children}
  </div>
);

export const MessagesWindow: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const sent = spring({ frame: frame - MESSAGE.send, fps, config: { damping: 18, stiffness: 140 } });
  const typing = spring({ frame: frame - MESSAGE.typingFrom, fps, config: { damping: 18 } });
  const reply = spring({ frame: frame - MESSAGE.reply, fps, config: { damping: 18, stiffness: 140 } });
  const replySlot = Math.max(typing, reply);

  const typed = Math.round(
    interpolate(frame, [MESSAGE.typeFrom, MESSAGE.typeTo], [0, DICTATED.length], {
      extrapolateLeft: "clamp",
      extrapolateRight: "clamp",
    }),
  );
  const draft = frame < MESSAGE.send ? DICTATED.slice(0, typed) : "";
  const caretOn = Math.floor(frame / 15) % 2 === 0 || (frame > MESSAGE.typeFrom && frame < MESSAGE.typeTo);

  const baseBottom = 22;
  const replyBottom = baseBottom;
  const mineBottom = baseBottom + replySlot * (REPLY_H + GAP);
  const mayaBottom = mineBottom + sent * (MINE_H + GAP);

  return (
    <div
      style={{
        width: 880,
        height: 480,
        borderRadius: 26,
        overflow: "hidden",
        background: "rgba(22,20,26,0.74)",
        backdropFilter: "blur(40px) saturate(1.5)",
        boxShadow:
          "0 0 0 1px rgba(255,255,255,0.1), 0 50px 120px rgba(0,0,0,0.45), inset 0 1px 0 rgba(255,255,255,0.06)",
        fontFamily,
        display: "flex",
        flexDirection: "column",
      }}
    >
      <div
        style={{
          height: 76,
          display: "flex",
          alignItems: "center",
          padding: "0 24px",
          borderBottom: `1px solid ${color.hairline}`,
          position: "relative",
        }}
      >
        <TrafficLights size={14} />
        <div
          style={{
            position: "absolute",
            inset: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            gap: 14,
          }}
        >
          <div
            style={{
              width: 40,
              height: 40,
              borderRadius: "50%",
              background: "linear-gradient(135deg, #f6a5b8, #b574e6)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              color: "white",
              fontWeight: 600,
              fontSize: 19,
            }}
          >
            M
          </div>
          <div style={{ color: color.label, fontWeight: 600, fontSize: 22 }}>Maya</div>
        </div>
      </div>

      <div style={{ position: "relative", flex: 1 }}>
        <div
          style={{
            position: "absolute",
            top: 18,
            width: "100%",
            textAlign: "center",
            color: color.tertiary,
            fontSize: 16,
            fontWeight: 500,
          }}
        >
          Today 6:48 PM
        </div>
        <Bubble style={{ bottom: mayaBottom }}>Still on for 7 tonight?</Bubble>
        <Bubble
          mine
          style={{
            bottom: mineBottom,
            opacity: sent,
            transform: `translateY(${(1 - sent) * 40}px) scale(${0.94 + sent * 0.06})`,
            transformOrigin: "bottom right",
          }}
        >
          {DICTATED}
        </Bubble>
        {frame < MESSAGE.reply + 2 ? (
          <Bubble style={{ bottom: replyBottom, opacity: typing * (1 - reply), padding: "20px 22px" }}>
            <div style={{ display: "flex", gap: 7 }}>
              {[0, 1, 2].map((i) => (
                <div
                  key={i}
                  style={{
                    width: 10,
                    height: 10,
                    borderRadius: "50%",
                    background: color.secondary,
                    opacity: 0.4 + 0.6 * Math.max(0, Math.sin((frame - i * 4) / 4)),
                  }}
                />
              ))}
            </div>
          </Bubble>
        ) : null}
        <Bubble
          style={{
            bottom: replyBottom,
            opacity: reply,
            transform: `translateY(${(1 - reply) * 16}px)`,
          }}
        >
          Window seat’s yours.
        </Bubble>
      </div>

      <div style={{ padding: "0 22px 22px" }}>
        <div
          style={{
            minHeight: 54,
            borderRadius: 27,
            border: `1px solid rgba(255,255,255,0.12)`,
            display: "flex",
            alignItems: "center",
            padding: "10px 12px 10px 22px",
            gap: 12,
          }}
        >
          <div
            style={{
              flex: 1,
              fontSize: 21,
              whiteSpace: "nowrap",
              lineHeight: 1.3,
              color: draft ? color.label : color.tertiary,
            }}
          >
            <span>{draft || "Message"}</span>
            {caretOn && frame < MESSAGE.send ? (
              <span
                style={{
                  display: "inline-block",
                  width: 2,
                  height: 26,
                  marginLeft: 2,
                  verticalAlign: "-5px",
                  background: color.amber,
                }}
              />
            ) : null}
          </div>
          <div
            style={{
              width: 34,
              height: 34,
              borderRadius: "50%",
              background: draft ? color.amber : "rgba(255,255,255,0.08)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              color: draft ? "#1c1305" : color.tertiary,
              fontSize: 20,
              fontWeight: 700,
            }}
          >
            ↑
          </div>
        </div>
      </div>
    </div>
  );
};
