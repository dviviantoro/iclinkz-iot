#!/usr/bin/env bash
# Install cron jobs for capture + OCR based on CRON_TIME from ../.env
# Run once on the Raspberry Pi to activate the schedule.
# Re-run after changing CRON_TIME to update the schedule.
# Usage: ./setup-cron.sh [--remove]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env"

CRON_TIME=""
[[ -f "$ENV_FILE" ]] && \
  CRON_TIME="$(grep -E '^CRON_TIME' "$ENV_FILE" | sed 's/^[^=]*=\s*//' | tr -d '"'"' \t\n" || true)"

if [[ -z "$CRON_TIME" ]]; then
  echo "CRON_TIME not set in $ENV_FILE — defaulting to '0 */6 * * *' (every 6 hours)" >&2
  CRON_TIME="0 */6 * * *"
fi

RUN_JOB="$CRON_TIME $SCRIPT_DIR/run.sh >> /var/log/raspi-meter.log 2>&1"

MARKER_START="# raspi-collector: managed block — do not edit manually"
MARKER_END="# raspi-collector: end"

if [[ "${1:-}" == "--remove" ]]; then
  crontab -l 2>/dev/null \
    | awk "/$MARKER_START/{found=1} !found{print} /$MARKER_END/{found=0}" \
    | crontab -
  echo "Cron jobs removed."
  exit 0
fi

EXISTING="$(crontab -l 2>/dev/null || true)"

# Strip any previously installed block
STRIPPED="$(echo "$EXISTING" \
  | awk "/$MARKER_START/{found=1} !found{print} /$MARKER_END/{found=0}")"

NEW_BLOCK="$(printf '%s\n%s\n%s\n' \
  "$MARKER_START" \
  "$RUN_JOB" \
  "$MARKER_END")"

printf '%s\n%s\n' "$STRIPPED" "$NEW_BLOCK" | crontab -

echo "Cron schedule set to: $CRON_TIME"
echo "  run.sh (capture + OCR) → /var/log/raspi-meter.log"
echo ""
echo "Current crontab:"
crontab -l
