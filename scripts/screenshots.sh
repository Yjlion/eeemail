#!/usr/bin/env bash
#
# Regenerates screenshots/ from the desktop UI's demo fixtures.
#
# The images are of `desktop/src/fixtures.ts`, never of a real mailbox. That is
# the point: the only pictures of a mail client that could otherwise exist are
# pictures of somebody's mail, and a screenshot in a README should not be one.
#
# Fixture data is fixed rather than generated (see the `NOW` constant), so two
# runs produce the same bytes and a change to the images is a change to the UI.
#
# Usage: ./scripts/screenshots.sh

set -euo pipefail

cd "$(dirname "$0")/.."

OUT="screenshots"
PORT="${SCREENSHOT_PORT:-5175}"
WIDTH=1280
HEIGHT=820

CHROMIUM="${CHROMIUM:-$(command -v chromium || command -v chromium-browser || command -v google-chrome || true)}"
if [ -z "$CHROMIUM" ]; then
  echo "no chromium found; set CHROMIUM=/path/to/chromium" >&2
  exit 1
fi

echo "building the demo bundle"
(cd desktop && npm run build:demo >/dev/null)

# A plain static server rather than `vite preview`: fewer moving parts, and it
# exits cleanly when this script does.
python3 -m http.server "$PORT" --directory desktop/dist-demo --bind 127.0.0.1 >/dev/null 2>&1 &
SERVER=$!
trap 'kill "$SERVER" 2>/dev/null || true' EXIT

# Wait for it rather than sleeping a guessed number of seconds.
for _ in $(seq 1 50); do
  if curl -sf "http://127.0.0.1:$PORT/index.html" >/dev/null; then break; fi
  sleep 0.2
done

mkdir -p "$OUT"

shoot() {
  local name="$1" route="$2"
  echo "  $name"
  # --virtual-time-budget lets the page's promises and timers run to completion
  # before the frame is captured; without it the panes photograph empty, because
  # every view here paints after an await.
  "$CHROMIUM" \
    --headless=new \
    --disable-gpu \
    --hide-scrollbars \
    --force-device-scale-factor=1 \
    --window-size="$WIDTH,$HEIGHT" \
    --virtual-time-budget=6000 \
    --screenshot="$OUT/$name.png" \
    "http://127.0.0.1:$PORT/index.html$route" \
    >/dev/null 2>&1
}

echo "capturing"
shoot inbox          "#/tag/inbox"
shoot reading        "#/tag/inbox/101"
shoot thread         "#/tag/inbox/102"
shoot holding        "#/tag/holding/106"
shoot trash          "#/tag/trash/109"
shoot composer       "#/screen/composer"
shoot contacts       "#/screen/contacts"
shoot settings       "#/screen/settings"

echo "wrote $(ls -1 "$OUT"/*.png | wc -l) images to $OUT/"
