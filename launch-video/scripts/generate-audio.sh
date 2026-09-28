#!/usr/bin/env bash
# Regenerates the ElevenLabs narration, dictation, sound effects and music in
# public/audio. Requires ELEVENLABS_API_KEY in the environment.
set -euo pipefail

: "${ELEVENLABS_API_KEY:?Set ELEVENLABS_API_KEY}"
cd "$(dirname "$0")/../public/audio"

API=https://api.elevenlabs.io/v1
NARRATOR=2NzqTfQARqdn4tcBKTSh # Brianna - Intimate & Understated
SPEAKER=bIHbv24MWmeRgasZH58o  # Will - Relaxed Optimist

post() { # url body outfile
  curl -sSf -X POST "$1" -H "xi-api-key: $ELEVENLABS_API_KEY" \
    -H "Content-Type: application/json" -d "$2" -o "$3"
}

tts() { # voice outfile stability text
  post "$API/text-to-speech/$1?output_format=mp3_44100_128" "$(jq -n --arg t "$4" --argjson s "$3" \
    '{text: $t, model_id: "eleven_multilingual_v2",
      voice_settings: {stability: $s, similarity_boost: 0.8, style: 0.1, use_speaker_boost: true}}')" "$2"
}

sfx() { # outfile seconds prompt
  post "$API/sound-generation?output_format=mp3_44100_128" "$(jq -n --arg t "$3" --argjson d "$2" \
    '{text: $t, duration_seconds: $d, prompt_influence: 0.6}')" "$1"
}

tts "$NARRATOR" vo_1.mp3 0.6 "You think faster than you type."
tts "$NARRATOR" vo_2.mp3 0.6 "So... just say it."
tts "$NARRATOR" vo_3.mp3 0.6 "Whisple turns your voice into clean text, in any app. Private, right on your computer."
tts "$NARRATOR" vo_4.mp3 0.6 "Say hello to Whisple. Just talk."
tts "$SPEAKER" dictation.mp3 0.45 "Hey Maya, running ten minutes late, grab us a table by the window?"

sfx whoosh.mp3 1.2 "Soft airy whoosh transition, gentle and cinematic, clean, no impact"
sfx pop.mp3 0.5 "Soft glassy user interface pop, subtle, premium Apple-like UI sound, very short"
sfx keys.mp3 0.7 "Two soft low-profile laptop keyboard key presses, quiet, close microphone, clean"
sfx rec_start.mp3 0.8 "Gentle soft rising two-note chime, warm, subtle, recording started notification"
sfx done.mp3 1.3 "Delicate warm soft bell ding confirmation, minimal, gentle, premium"
sfx shimmer.mp3 2.2 "Soft rising shimmer with warm glow, subtle sparkle swell, cinematic logo reveal, gentle"

post "$API/music?output_format=mp3_44100_128" "$(jq -n '{
  prompt: "Relaxed, warm, minimal instrumental for a calm Apple-style product launch film. Soft felt piano motif, gentle analog synth pads, subtle warm sub bass, light brushed percussion entering around 8 seconds, airy and optimistic, 84 BPM, spacious reverb, no vocals. Builds softly, then resolves with a gentle final chord and natural fade out at the end.",
  music_length_ms: 31000, force_instrumental: true}')" music.mp3

echo "Audio written to $(pwd). Word timings in src/timeline.ts assume these takes;"
echo "regenerated narration can land slightly differently."
