#!/bin/sh
# package-dmg.sh: builds the signed Fractile.app (scripts/build-app.sh), puts it in a
# drag-to-Applications disk image dist/Fractile-<version>.dmg, signs the image with the same
# Developer ID, and has Apple notarize and staple it, so it opens on any Mac with no
# Gatekeeper warning.
#
# Notarization uses the notarytool keychain profile FRACTILE_NOTARY_PROFILE (default
# "fractile"). Make it once with:
#   xcrun notarytool store-credentials fractile --apple-id <you@example.com> \
#     --team-id <TEAMID> --password <app-specific password from appleid.apple.com>
# Without the profile, the image is still built and signed but not notarized, and the script
# says so.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
profile=${FRACTILE_NOTARY_PROFILE:-fractile}
version=$(/usr/bin/plutil -extract CFBundleShortVersionString raw "$root/app/Info.plist")
dist="$root/dist"
dmg="$dist/Fractile-$version.dmg"
stage="$dist/dmg-stage"

bundle=$("$root/scripts/build-app.sh" | tail -n 1)
[ -d "$bundle/Contents" ] || { echo "build-app.sh did not make an app bundle." >&2; exit 1; }
identity=$(/usr/bin/codesign --display --verbose=2 "$bundle" 2>&1 \
  | sed -n 's/^Authority=\(Developer ID Application:.*\)$/\1/p' | head -1)
[ -n "$identity" ] || { echo "Fractile.app is not signed with a Developer ID." >&2; exit 1; }

notarize() {
  /usr/bin/xcrun notarytool submit "$1" --keychain-profile "$profile" --wait
  /usr/bin/xcrun stapler staple "$1"
}
have_profile=false
if /usr/bin/xcrun notarytool history --keychain-profile "$profile" >/dev/null 2>&1; then
  have_profile=true
fi

# Notarize the app on its own first, so a copy dragged out of the image keeps its ticket
# offline.
if $have_profile; then
  zip="$dist/Fractile-notarize.zip"
  mkdir -p "$dist"
  rm -f "$zip"
  /usr/bin/ditto -c -k --keepParent "$bundle" "$zip"
  /usr/bin/xcrun notarytool submit "$zip" --keychain-profile "$profile" --wait
  /usr/bin/xcrun stapler staple "$bundle"
  rm -f "$zip"
fi

rm -rf "$stage" "$dmg"
mkdir -p "$stage"
/usr/bin/ditto "$bundle" "$stage/Fractile.app"
ln -s /Applications "$stage/Applications"
cp "$bundle/Contents/Resources/AppIcon.icns" "$stage/.VolumeIcon.icns"
/usr/bin/hdiutil create -quiet -volname Fractile -srcfolder "$stage" -fs HFS+ \
  -format UDRW -ov "$dist/Fractile-rw.dmg"
# Mark the volume as having a custom icon, so the mounted disk shows the Fractile icon.
mount=$(/usr/bin/hdiutil attach -nobrowse -noautoopen "$dist/Fractile-rw.dmg" \
  | sed -n 's|.*\(/Volumes/.*\)$|\1|p' | tail -1)
/usr/bin/xattr -wx com.apple.FinderInfo \
  "0000000000000000040000000000000000000000000000000000000000000000" "$mount"
/usr/bin/hdiutil detach -quiet "$mount"
/usr/bin/hdiutil convert -quiet "$dist/Fractile-rw.dmg" -format UDZO \
  -imagekey zlib-level=9 -o "$dmg"
rm -rf "$stage" "$dist/Fractile-rw.dmg"

/usr/bin/codesign --force --timestamp --sign "$identity" "$dmg"
/usr/bin/codesign --verify --strict "$dmg"

if $have_profile; then
  notarize "$dmg"
  /usr/bin/xcrun stapler validate "$dmg"
  /usr/sbin/spctl --assess --type open --context context:primary-signature --verbose "$dmg"
  echo "notarized: $dmg" >&2
else
  echo "signed but NOT notarized: no notarytool profile '$profile' (see the top of $0)." >&2
fi
printf '%s\n' "$dmg"
