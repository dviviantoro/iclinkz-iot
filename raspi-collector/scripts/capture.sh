#!/usr/bin/env bash
# Capture a flowmeter image and save as YYYYMMDD_<unixtime>.jpg
# CAMERA_SOURCE in .env selects the capture backend:
#   picamera   — libcamera-still (RPi 5/4) or raspistill (legacy)  [default]
#   webcam     — fswebcam via /dev/video0 (or WEBCAM_DEVICE)
#   ffmpeg     — ffmpeg v4l2 grab (fallback if fswebcam unavailable)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
IMAGES_DIR="$SCRIPT_DIR/../images"
ENV_FILE="$SCRIPT_DIR/../.env"
mkdir -p "$IMAGES_DIR"

# Load .env
CAMERA_SOURCE=""
WEBCAM_DEVICE=""
if [[ -f "$ENV_FILE" ]]; then
  CAMERA_SOURCE="$(grep -E '^CAMERA_SOURCE' "$ENV_FILE" | sed 's/^[^=]*=\s*//' | tr -d '"'"' \t\n" || true)"
  WEBCAM_DEVICE="$(grep -E '^WEBCAM_DEVICE'  "$ENV_FILE" | sed 's/^[^=]*=\s*//' | tr -d '"'"' \t\n" || true)"
fi

CAMERA_SOURCE="${CAMERA_SOURCE:-picamera}"
WEBCAM_DEVICE="${WEBCAM_DEVICE:-/dev/video0}"

DATE="$(date +%Y%m%d)"
TS="$(date +%s)"
OUT="$IMAGES_DIR/${DATE}_${TS}.jpg"

case "$CAMERA_SOURCE" in
  webcam)
    if command -v fswebcam &>/dev/null; then
      fswebcam -d "$WEBCAM_DEVICE" -r 1920x1080 --no-banner --jpeg 95 "$OUT"
    elif command -v ffmpeg &>/dev/null; then
      ffmpeg -y -f v4l2 -video_size 1920x1080 -i "$WEBCAM_DEVICE" -frames:v 1 "$OUT" -loglevel error
    else
      echo "Webcam capture requires fswebcam or ffmpeg" >&2
      exit 1
    fi
    ;;
  picamera|*)
    if command -v libcamera-still &>/dev/null; then
      libcamera-still --nopreview --width 1920 --height 1080 -o "$OUT"
    elif command -v raspistill &>/dev/null; then
      raspistill --nopreview --width 1920 --height 1080 -o "$OUT"
    else
      echo "Pi camera capture requires libcamera-still or raspistill" >&2
      exit 1
    fi
    ;;
esac

echo "Captured: $OUT"
