import { AbsoluteFill, useCurrentFrame } from "remotion";
import { Grain, Vignette } from "./components/Atmosphere";
import { Demo } from "./scenes/Demo";
import { EndCard } from "./scenes/EndCard";
import { Features } from "./scenes/Features";
import { Intro } from "./scenes/Intro";
import { Soundtrack } from "./Soundtrack";
import { DEMO, END, FEATURES, INTRO } from "./timeline";

// Scenes work in absolute frames (see timeline.ts). A Sequence would rebase
// the frame, so a scene is only mounted while its span is on screen.
const Scene: React.FC<{ span: { from: number; to: number }; children: React.ReactNode }> = ({
  span,
  children,
}) => {
  const frame = useCurrentFrame();
  if (frame < span.from || frame >= span.to) {
    return null;
  }
  return <AbsoluteFill>{children}</AbsoluteFill>;
};

export const WhispleLaunch: React.FC = () => (
  <AbsoluteFill style={{ background: "#08080a" }}>
    <Scene span={INTRO}>
      <Intro />
    </Scene>
    <Scene span={DEMO}>
      <Demo />
    </Scene>
    <Scene span={FEATURES}>
      <Features />
    </Scene>
    <Scene span={END}>
      <EndCard />
    </Scene>
    <Vignette strength={0.45} />
    <Grain opacity={0.045} />
    <Soundtrack />
  </AbsoluteFill>
);
