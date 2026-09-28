import { Composition } from "remotion";
import { WhispleLaunch } from "./WhispleLaunch";
import { DURATION, FPS } from "./timeline";

export const RemotionRoot: React.FC = () => (
  <Composition
    id="WhispleLaunch"
    component={WhispleLaunch}
    durationInFrames={DURATION}
    fps={FPS}
    width={1920}
    height={1080}
  />
);
