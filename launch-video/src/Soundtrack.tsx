import { Audio, interpolate, Sequence, staticFile } from "remotion";
import { BEAT, COMMAND, DICTATION_AT, DURATION, END, FEATURES, HUD, MESSAGE, SAY, sec, VO } from "./timeline";

// Spans where speech sits on top of the music bed, in frames.
const SPEECH: [number, number][] = [
  [VO.intro, VO.intro + 58],
  [VO.sayIt, VO.sayIt + 44],
  [DICTATION_AT, DICTATION_AT + 104],
  [SAY.clean, SAY.clean + 70],
  [SAY.danish, SAY.danish + 62],
  [SAY.command, SAY.command + 44],
  [VO.end, VO.end + 60],
];

const RAMP = 10;
const BED = 0.34;
const DUCKED = 0.17;

const musicVolume = (f: number) => {
  let duck = 0;
  for (const [a, b] of SPEECH) {
    duck = Math.max(
      duck,
      interpolate(f, [a - RAMP, a, b, b + RAMP * 2], [0, 1, 1, 0], {
        extrapolateLeft: "clamp",
        extrapolateRight: "clamp",
      }),
    );
  }
  const fade = interpolate(f, [0, 24, DURATION - 50, DURATION], [0.2, 1, 1, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  return (BED + (DUCKED - BED) * duck) * fade;
};

const Cue: React.FC<{ at: number; src: string; volume: number }> = ({ at, src, volume }) => (
  <Sequence from={at} layout="none">
    <Audio src={staticFile(`audio/${src}`)} volume={volume} />
  </Sequence>
);

export const Soundtrack: React.FC = () => (
  <>
    <Audio src={staticFile("audio/music.mp3")} volume={musicVolume} />

    <Cue at={VO.intro} src="vo_1.mp3" volume={1} />
    <Cue at={VO.sayIt} src="vo_2.mp3" volume={1} />
    <Cue at={DICTATION_AT} src="dictation.mp3" volume={1} />
    <Cue at={SAY.clean} src="say_clean.mp3" volume={1} />
    <Cue at={SAY.danish} src="say_da.mp3" volume={1} />
    <Cue at={SAY.command} src="say_command.mp3" volume={1} />
    <Cue at={VO.end} src="vo_4.mp3" volume={1} />

    <Cue at={92} src="whoosh.mp3" volume={0.28} />
    <Cue at={HUD.appear} src="pop.mp3" volume={1} />
    <Cue at={HUD.press - 3} src="keys.mp3" volume={0.9} />
    <Cue at={HUD.press + 1} src="rec_start.mp3" volume={0.85} />
    <Cue at={HUD.stop - 2} src="keys.mp3" volume={0.6} />
    <Cue at={HUD.panelOpen + 1} src="done.mp3" volume={0.42} />
    <Cue at={MESSAGE.send} src="pop.mp3" volume={0.9} />
    <Cue at={MESSAGE.reply} src="pop.mp3" volume={0.6} />

    <Cue at={FEATURES.from - 4} src="whoosh.mp3" volume={0.26} />
    <Cue at={SAY.clean + sec(2.26) + 6} src="done.mp3" volume={0.3} />
    <Cue at={BEAT.languages.from - 4} src="whoosh.mp3" volume={0.16} />
    <Cue at={SAY.danish + 4} src="pop.mp3" volume={0.55} />
    <Cue at={SAY.danish + 22} src="pop.mp3" volume={0.5} />
    <Cue at={SAY.danish + 40} src="pop.mp3" volume={0.45} />
    <Cue at={BEAT.command.from - 4} src="whoosh.mp3" volume={0.16} />
    <Cue at={COMMAND.browserOpens} src="done.mp3" volume={0.35} />
    <Cue at={BEAT.models.from - 4} src="whoosh.mp3" volume={0.16} />
    <Cue at={BEAT.models.from + 2} src="pop.mp3" volume={0.5} />
    <Cue at={END.from} src="shimmer.mp3" volume={0.3} />
  </>
);
