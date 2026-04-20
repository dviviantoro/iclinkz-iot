#!/usr/bin/env bash
# Capture a flowmeter image then immediately read it via Groq OCR.
# This is the single entry point invoked by cron.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] Starting capture..." >&2
IMAGE="$("$SCRIPT_DIR/capture.sh")"
IMAGE_PATH="${IMAGE#Captured: }"

echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] Running OCR on: $(basename "$IMAGE_PATH")" >&2
"$SCRIPT_DIR/ocr-meter-groq.sh" --force "$IMAGE_PATH"
