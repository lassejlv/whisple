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

tts() { # voice outfile stability text [model] [language]
  post "$API/text-to-speech/$1?output_format=mp3_44100_128" "$(jq -n --arg t "$4" --argjson s "$3" \
    --arg m "${5:-eleven_multilingual_v2}" --arg l "${6:-}" \
    '{text: $t, model_id: $m,
      voice_settings: {stability: $s, similarity_boost: 0.8, style: 0.1, use_speaker_boost: true}}
      + (if $l == "" then {} else {language_code: $l} end)')" "$2"
}

sfx() { # outfile seconds prompt
  post "$API/sound-generation?output_format=mp3_44100_128" "$(jq -n --arg t "$3" --argjson d "$2" \
    '{text: $t, duration_seconds: $d, prompt_influence: 0.6}')" "$1"
}

tts "$NARRATOR" vo_1.mp3 0.6 "You think faster than you type."
tts "$NARRATOR" vo_2.mp3 0.6 "So... just say it."
tts "$NARRATOR" vo_4.mp3 0.6 "Say hello to Whisple. Just talk."
tts "$SPEAKER" dictation.mp3 0.45 "Hey Maya, running ten minutes late, grab us a table by the window?"

# Lines dictated to Whisple for the feature beats. The Spanish and German
# ones were only filmed, not heard; they are kept for re-shooting.
tts "$SPEAKER" say_clean.mp3 0.45 "So, um, I I think we should ship it on Friday."
tts "$SPEAKER" say_command.mp3 0.45 "Go to github dot com."
tts hbLdgbgzfKYPCUr4Rs2V say_da.mp3 0.45 "Husk at købe mælk og kaffe på vej hjem." # Jonas
tts cgSgspJ2msm6clMCkdW9 say_es.mp3 0.5 "Nos vemos mañana a las nueve en la oficina, y trae los informes, por favor." eleven_turbo_v2_5 es
tts IKne3meq5aSn9XLyUdCD say_de.mp3 0.45 "Kannst du mir die Präsentation bis Freitag schicken?" # Charlie

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
