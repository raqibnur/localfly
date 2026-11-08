#!/usr/bin/env bash
# Signs with Developer ID, notarizes, and staples dist/Localfly.dmg.
#
# Required:
#   DEVELOPER_ID      "Developer ID Application: Your Name (TEAMID)"
# Plus either a stored notarytool profile:
#   NOTARY_PROFILE    created once with: xcrun notarytool store-credentials <name> --apple-id … --team-id … --password …
# or credentials directly:
#   APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD (an app-specific password)
set -euo pipefail
cd "$(dirname "$0")/.."

: "${DEVELOPER_ID:?Set DEVELOPER_ID to your Developer ID Application identity}"
DEVELOPER_ID="$DEVELOPER_ID" scripts/build.sh

if [[ -n "${NOTARY_PROFILE:-}" ]]; then
  AUTH=(--keychain-profile "$NOTARY_PROFILE")
else
  : "${APPLE_ID:?Set APPLE_ID or NOTARY_PROFILE}" "${APPLE_TEAM_ID:?Set APPLE_TEAM_ID}" "${APPLE_APP_PASSWORD:?Set APPLE_APP_PASSWORD}"
  AUTH=(--apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD")
fi

xcrun notarytool submit dist/Localfly.dmg "${AUTH[@]}" --wait
xcrun stapler staple dist/Localfly.dmg
xcrun stapler validate dist/Localfly.dmg
spctl --assess --type open --context context:primary-signature -v dist/Localfly.dmg
echo "Notarized dist/Localfly.dmg"
