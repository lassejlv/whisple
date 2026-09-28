import {
  AbsoluteFill,
  Easing,
  interpolate,
  OffthreadVideo,
  Sequence,
  staticFile,
  useCurrentFrame,
} from "remotion";
import { Keycaps } from "../components/Keycaps";
import { blur } from "../components/Reveal";
import { MessagesWindow } from "../components/MessagesWindow";
import { Wallpaper } from "../components/Wallpaper";
import { DEMO, HUD } from "../timeline";
import { ease } from "../theme";

const HUD_SCALE = 1.1;
const HUD_W = 820 * HUD_SCALE;
const HUD_H = 378 * HUD_SCALE;
const BAR_H = 116 * HUD_SCALE;

const clamp = { extrapolateLeft: "clamp", extrapolateRight: "clamp" } as const;
const inOut = Easing.bezier(...ease.inOut);
const out = Easing.bezier(...ease.out);

const HudClip: React.FC<{ src: string; trimBefore: number; trimAfter?: number }> = ({
  src,
  trimBefore,
  trimAfter,
}) => (
  <OffthreadVideo
    src={staticFile(src)}
    transparent
    muted
    trimBefore={trimBefore}
    trimAfter={trimAfter}
    // The capture's bottom row shows the desktop taskbar through the window.
    style={{ width: HUD_W, height: HUD_H, display: "block", clipPath: `inset(0 0 ${HUD_SCALE * 1.2}px 0)` }}
  />
);

export const Demo: React.FC = () => {
  const frame = useCurrentFrame();

  const enter = interpolate(frame, [DEMO.from, DEMO.from + 30], [0, 1], { ...clamp, easing: out });

  // Lean in toward the bar while it listens, then settle back for the message.
  const lean = interpolate(
    frame,
    [HUD.press - 8, HUD.press + 102, HUD.panelOpen - 6, HUD.panelOpen + 40],
    [0, 1, 1, 0],
    { ...clamp, easing: inOut },
  );
  const scale = (1.06 - enter * 0.06) * (1 + lean * 0.07);

  const hudIn = interpolate(frame, [HUD.appear, HUD.appear + 16], [0, 1], { ...clamp, easing: out });
  // On X11 the result panel snaps open in one resize; ease it up from the bar
  // the way the macOS window grows.
  const panelInset = interpolate(frame, [HUD.panelOpen, HUD.panelOpen + 13], [HUD_H - BAR_H, 0], {
    ...clamp,
    easing: out,
  });

  const keysIn = interpolate(frame, [HUD.press - 48, HUD.press - 32], [0, 1], { ...clamp, easing: out });
  const keysOut = interpolate(frame, [HUD.press + 24, HUD.press + 40], [0, 1], { ...clamp, easing: inOut });

  const windowIn = interpolate(frame, [DEMO.from + 8, DEMO.from + 40], [0, 1], { ...clamp, easing: out });
  const windowDim = interpolate(lean, [0, 1], [1, 0.72]);

  return (
    <AbsoluteFill style={{ opacity: enter, filter: blur((1 - enter) * 16) }}>
      <AbsoluteFill style={{ transform: `scale(${scale})`, transformOrigin: "50% 88%" }}>
        <Wallpaper />

        <div
          style={{
            position: "absolute",
            left: "50%",
            top: 96,
            transform: `translateX(-50%) translateY(${(1 - windowIn) * 30}px)`,
            opacity: windowIn * windowDim,
          }}
        >
          <MessagesWindow />
        </div>

        <div
          style={{
            position: "absolute",
            left: "50%",
            bottom: 222,
            transform: `translateX(-50%) translateY(${(1 - keysIn) * 16 + keysOut * -10}px)`,
            opacity: keysIn * (1 - keysOut),
            filter: blur(keysOut * 8),
          }}
        >
          <Keycaps press={HUD.press - 2} />
        </div>

        <div
          style={{
            position: "absolute",
            left: "50%",
            bottom: 44,
            width: HUD_W,
            height: HUD_H,
            transform: `translateX(-50%) translateY(${(1 - hudIn) * 28}px)`,
            opacity: hudIn,
            filter: `${hudIn < 0.995 ? `blur(${(1 - hudIn) * 10}px) ` : ""}drop-shadow(0 30px 60px rgba(0,0,0,0.45))`,
          }}
        >
          <Sequence from={HUD.seg1.at} durationInFrames={HUD.seg2.at - HUD.seg1.at} layout="none">
            <HudClip src="footage/hud_record.webm" trimBefore={HUD.seg1.clipFrom} />
          </Sequence>
          <Sequence
            from={HUD.seg2.at}
            durationInFrames={HUD.result.at - HUD.seg2.at}
            layout="none"
          >
            <HudClip src="footage/hud_record.webm" trimBefore={HUD.seg2.clipFrom} />
          </Sequence>
          <Sequence from={HUD.result.at} layout="none">
            <div style={{ clipPath: `inset(${panelInset}px 0 0 0 round 30px 30px 0 0)` }}>
              <HudClip src="footage/hud_result.webm" trimBefore={HUD.result.clipFrom} />
            </div>
          </Sequence>
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  );
};
