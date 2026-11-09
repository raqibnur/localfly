#!/usr/bin/env bash
# Renders scripts/og/og.html into the social preview images with headless Chrome.
set -euo pipefail
cd "$(dirname "$0")/../.."

CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
SOURCE="file://$PWD/scripts/og/og.html"
TMP=$(mktemp -d)

render() { # width height output
  local w=$1 h=$2 out=$3
  # Render at 2x and downscale, for crisper edges than a 1x capture.
  sed "s/--w: 1200px; --h: 630px;/--w: ${w}px; --h: ${h}px;/" scripts/og/og.html > "$TMP/og.html"
  cp -R site "$TMP/site" 2>/dev/null || true
  mkdir -p "$TMP/scripts/og" && mv "$TMP/og.html" "$TMP/scripts/og/og.html"
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2 \
    --window-size="${w},${h}" --screenshot="$TMP/shot.png" "file://$TMP/scripts/og/og.html" >/dev/null 2>&1
  sips -z "$h" "$w" "$TMP/shot.png" --out "$out" >/dev/null
  echo "Wrote $out ($(sips -g pixelWidth -g pixelHeight "$out" | awk '/pixel/{printf "%s ", $2}'))"
}

render 1200 630 site/assets/og.png
render 1280 640 docs/images/social.png
rm -rf "$TMP"
