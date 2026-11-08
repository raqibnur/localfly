#!/usr/bin/env bash
# Archives, signs with Developer ID and notarizes Localfly using the Apple ID signed in to Xcode
# (Xcode → Settings → Accounts). No certificates or passwords are needed on disk; Xcode uses a
# cloud-managed Developer ID certificate. The output is dist/Localfly.app (stapled) and dist/Localfly.dmg.
#
# Usage: TEAM_ID=ABCDE12345 scripts/notarize-xcode.sh
set -euo pipefail
cd "$(dirname "$0")/.."

: "${TEAM_ID:?Set TEAM_ID to your Apple Developer team ID}"
ARCHIVE=build/Localfly.xcarchive
EXPORT=build/export
DIST=dist

xcodegen generate --quiet
rm -rf "$ARCHIVE" "$EXPORT"

xcodebuild -project Localfly.xcodeproj -scheme Localfly -configuration Release \
  -destination 'generic/platform=macOS' -archivePath "$ARCHIVE" \
  -allowProvisioningUpdates \
  CODE_SIGN_STYLE=Automatic DEVELOPMENT_TEAM="$TEAM_ID" CODE_SIGN_IDENTITY="Apple Development" \
  -quiet archive

OPTIONS=$(mktemp -t localfly-export).plist
cat > "$OPTIONS" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>method</key><string>developer-id</string>
  <key>teamID</key><string>$TEAM_ID</string>
  <key>signingStyle</key><string>automatic</string>
  <key>destination</key><string>upload</string>
</dict>
</plist>
PLIST

echo "Uploading to Apple's notary service…"
xcodebuild -exportArchive -archivePath "$ARCHIVE" -exportOptionsPlist "$OPTIONS" \
  -exportPath "$EXPORT" -allowProvisioningUpdates -quiet

echo "Waiting for notarization (usually 1–10 minutes)…"
for _ in $(seq 1 60); do
  if xcodebuild -exportNotarizedApp -archivePath "$ARCHIVE" -exportPath "$EXPORT" >/dev/null 2>&1; then
    break
  fi
  sleep 20
done
test -d "$EXPORT/Localfly.app" || { echo "Notarization didn't finish; check Xcode's Organizer for details." >&2; exit 1; }

rm -rf "$DIST" && mkdir -p "$DIST"
cp -R "$EXPORT/Localfly.app" "$DIST/"
xcrun stapler validate "$DIST/Localfly.app"
spctl --assess --type execute -v "$DIST/Localfly.app"

STAGE=$(mktemp -d)
cp -R "$DIST/Localfly.app" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname Localfly -srcfolder "$STAGE" -ov -format UDZO "$DIST/Localfly.dmg"
rm -rf "$STAGE" "$OPTIONS"

echo "Notarized app: $DIST/Localfly.app"
echo "Disk image:    $DIST/Localfly.dmg (contains the stapled app)"
