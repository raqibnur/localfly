#!/usr/bin/env bash
# Builds Localfly.app and Localfly.dmg into dist/.
# Signs ad-hoc by default; set DEVELOPER_ID="Developer ID Application: Name (TEAMID)" to sign for distribution.
set -euo pipefail
cd "$(dirname "$0")/.."

IDENTITY="${DEVELOPER_ID:--}"
DIST=dist
APP="$DIST/Localfly.app"

command -v xcodegen >/dev/null || { echo "xcodegen missing: brew install xcodegen" >&2; exit 1; }
xcodegen generate --quiet

xcodebuild -project Localfly.xcodeproj -scheme Localfly -configuration Release \
  -derivedDataPath build/dd -destination 'generic/platform=macOS' \
  ARCHS="arm64 x86_64" ONLY_ACTIVE_ARCH=NO CODE_SIGNING_ALLOWED=NO -quiet clean build

rm -rf "$DIST" && mkdir -p "$DIST"
cp -R build/dd/Build/Products/Release/Localfly.app "$APP"

if [[ "$IDENTITY" == "-" ]]; then TIMESTAMP=--timestamp=none; else TIMESTAMP=--timestamp; fi
codesign --force --options runtime "$TIMESTAMP" --sign "$IDENTITY" "$APP"
codesign --verify --strict "$APP"

STAGE=$(mktemp -d)
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname Localfly -srcfolder "$STAGE" -ov -format UDZO "$DIST/Localfly.dmg"
rm -rf "$STAGE"
if [[ "$IDENTITY" != "-" ]]; then codesign --force --timestamp --sign "$IDENTITY" "$DIST/Localfly.dmg"; fi

echo "Built $APP and $DIST/Localfly.dmg"
