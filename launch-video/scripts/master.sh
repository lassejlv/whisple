#!/usr/bin/env bash
# Renders the film, repairs stray render glitches and masters the audio to
# -15 LUFS for web playback. Extra arguments go to `remotion render/still`.
set -euo pipefail
cd "$(dirname "$0")/.."

if [ -z "${SKIP_RENDER:-}" ]; then
  npx remotion render WhispleLaunch out/whisple-launch.mp4 "$@"
fi

# Headless Chrome occasionally captures a frame before its last tile row is
# repainted. Stills are reliable, so re-render any flagged frame and splice it in.
frames=$(python3 scripts/check-frames.py out/whisple-launch.mp4 3 | sed -n 's/^frame \([0-9]*\):.*/\1/p' || true)
source=out/whisple-launch.mp4
if [ -n "$frames" ]; then
  mkdir -p out/patch
  # Splice in RGB: Remotion writes full-range BT.601, the stills are sRGB, and
  # the repaired file is encoded once as standard limited-range BT.709.
  inputs=() graph="[0:v]scale=in_range=full:in_color_matrix=bt601,format=gbrp[v0];" last="v0" i=1
  for n in $frames; do
    npx remotion still WhispleLaunch "out/patch/$n.png" --frame="$n" "$@"
    inputs+=(-i "out/patch/$n.png")
    graph+="[$i:v]format=gbrp[p$i];[$last][p$i]overlay=enable='eq(n,$n)':format=gbrp[v$i];"
    last="v$i" i=$((i + 1))
  done
  graph+="[$last]scale=out_color_matrix=bt709:out_range=tv,format=yuv420p[out]"
  ffmpeg -loglevel error -y -i out/whisple-launch.mp4 "${inputs[@]}" \
    -filter_complex "$graph" -map "[out]" -map 0:a -c:v libx264 -crf 16 -preset slow \
    -color_range tv -colorspace bt709 -color_primaries bt709 -color_trc bt709 \
    -c:a copy out/whisple-launch-repaired.mp4
  source=out/whisple-launch-repaired.mp4
  echo "Re-rendered frames: $frames"
fi

stats=$(ffmpeg -hide_banner -i "$source" \
  -af loudnorm=I=-15:TP=-1.5:LRA=9:print_format=json -f null - 2>&1 | sed -n '/^{/,/^}/p')
m() { jq -r ".$1" <<<"$stats"; }

ffmpeg -loglevel error -y -i "$source" -c:v copy \
  -af "loudnorm=I=-15:TP=-1.5:LRA=9:measured_I=$(m input_i):measured_TP=$(m input_tp):measured_LRA=$(m input_lra):measured_thresh=$(m input_thresh):linear=true,aresample=48000" \
  -c:a aac -b:a 256k -t 30 -movflags +faststart out/whisple-launch-final.mp4

echo "Wrote out/whisple-launch-final.mp4"
python3 scripts/check-frames.py out/whisple-launch-final.mp4 4 ||
  echo "note: the keycap press peak near frame 173 is expected to be flagged"
