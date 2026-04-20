#!/usr/bin/env bash
# Test: simulate 4 cron-triggered capture+OCR runs using images/sample.jpeg
# Each run copies sample.jpeg as YYYYMMDD_<unixtime>.jpeg then calls OCR.
# Output: meter-data/YYYYMMDD_<unixtime>.json per run.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SAMPLE="$SCRIPT_DIR/../images/sample.jpeg"
IMAGES_DIR="$SCRIPT_DIR/../images"
RUNS=4

[[ -f "$SAMPLE" ]] || { echo "Sample image not found: $SAMPLE" >&2; exit 1; }

echo "=== raspi-collector test: $RUNS simulated cron runs ==="
echo ""

SAVED=()

for i in $(seq 1 $RUNS); do
  DATE="$(date +%Y%m%d)"
  TS="$(date +%s)"
  COPY="$IMAGES_DIR/${DATE}_${TS}.jpeg"

  echo "--- Run $i/$RUNS ---"
  cp "$SAMPLE" "$COPY"
  echo "Captured (copy): $(basename "$COPY")"

  "$SCRIPT_DIR/ocr-meter-groq.sh" "$COPY"
  echo ""

  SAVED+=("$SCRIPT_DIR/../meter-data/${DATE}_${TS}.json")

  # small gap so timestamps differ
  sleep 1
done

echo "=== Done. JSON files saved ==="
for f in "${SAVED[@]}"; do
  echo "  $f"
done
