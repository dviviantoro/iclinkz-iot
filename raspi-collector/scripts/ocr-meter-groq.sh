#!/usr/bin/env bash
# Flowmeter OCR via Groq vision API (llama-4-scout)
# Reads latest YYYYMMDD_<unixtime>.<ext> from ../images/
# Saves result to ../meter-data/YYYYMMDD_<unixtime>.json matching the image filename.
# Usage: ./ocr-meter-groq.sh [image] [--force]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
IMAGES_DIR="$SCRIPT_DIR/../images"
DATA_DIR="$SCRIPT_DIR/../meter-data"

IMAGE=""
FORCE=""
for arg in "$@"; do
  if [[ "$arg" == "--force" ]]; then FORCE="--force"
  else IMAGE="$arg"
  fi
done

if [[ -z "$IMAGE" ]]; then
  IMAGE="$(ls "$IMAGES_DIR"/*.jpg "$IMAGES_DIR"/*.jpeg "$IMAGES_DIR"/*.png "$IMAGES_DIR"/*.webp 2>/dev/null \
    | grep -E '/[0-9]{8}_[0-9]+\.(jpg|jpeg|png|webp)$' \
    | sort | tail -1 || true)"
  if [[ -z "$IMAGE" ]]; then
    echo "No image found matching YYYYMMDD_<unixtime>.<ext> in $IMAGES_DIR" >&2
    echo "Usage: $0 [image] [--force]" >&2
    exit 1
  fi
  echo "Auto-detected image: $(basename "$IMAGE")" >&2
fi

[[ -f "$IMAGE" ]] || { echo "File not found: $IMAGE" >&2; exit 1; }

# Derive output filename from image basename: YYYYMMDD_<ts>.json
IMAGE_BASE="$(basename "${IMAGE%.*}")"
OUT_FILE="$DATA_DIR/${IMAGE_BASE}.json"
mkdir -p "$DATA_DIR"

if [[ -f "$OUT_FILE" && "$FORCE" != "--force" ]]; then
  echo "Using cached result for $IMAGE_BASE" >&2
  cat "$OUT_FILE"
  exit 0
fi

ENV_FILE="$SCRIPT_DIR/../.env"
[[ -f "$ENV_FILE" ]] && \
  GROQ_API_KEY="$(grep -E '^GROQ_API_KEY' "$ENV_FILE" | sed 's/^[^=]*=\s*//' | tr -d '"'"' \t\n")"
[[ -n "${GROQ_API_KEY:-}" ]] || { echo "GROQ_API_KEY not set" >&2; exit 1; }

case "${IMAGE##*.}" in
  jpg|jpeg) MIME="image/jpeg" ;;
  png)      MIME="image/png"  ;;
  *)        MIME="image/jpeg" ;;
esac

echo "Encoding image..." >&2
URL_FULL="data:${MIME};base64,$(base64 < "$IMAGE" | tr -d '\n')"

PAYLOAD="$(mktemp /tmp/groq_meter_XXXXXX.json)"

jq -n --arg u "$URL_FULL" \
  '{model:"meta-llama/llama-4-scout-17b-16e-instruct",temperature:0,max_completion_tokens:256,stream:false,
    messages:[
      {role:"system",content:"You output ONLY valid JSON. No markdown, no explanation, no steps."},
      {role:"user",content:[
        {type:"text",text:"Read this water flowmeter photo carefully. The brand name is printed at the top of the dial face. The 6-digit roller counter shows the total reading — read each digit left to right, preserving ALL leading zeros (e.g. \"000331\"). Extract: brand, reading_m3 (6-digit zero-padded string), dn_mm, qn_m3h, pn_bar, max_temp_c, iso. Output only the JSON object."},
        {type:"image_url",image_url:{url:$u}}
      ]}
    ]}' > "$PAYLOAD"

echo "Calling GROQ API..." >&2

RESPONSE="$(curl -s "https://api.groq.com/openai/v1/chat/completions" \
  -X POST \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${GROQ_API_KEY}" \
  --data "@${PAYLOAD}" \
| jq -r '.choices[0].message.content' \
| sed '/^```/d')"

rm -f "$PAYLOAD"

FINAL="$(jq -n \
  --argjson data "$RESPONSE" \
  --arg ts   "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg src  "$(basename "$IMAGE")" \
  '$data + {timestamp:$ts, source_image:$src}')"

echo "$FINAL" | tee "$OUT_FILE"
echo "" >&2
echo "Saved → $OUT_FILE" >&2
