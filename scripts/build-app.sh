#!/bin/sh
# Builds target/Fractile.app and signs it with a Developer ID identity
# (FRACTILE_CODESIGN_IDENTITY, or the first one in the keychain).
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
bundle="$root/target/Fractile.app"
icon_output="$root/target/AppIcon.assets"
icon_info="$root/target/AppIcon-info.plist"
identity=${FRACTILE_CODESIGN_IDENTITY:-}
deployment_target=$(/usr/bin/plutil -extract LSMinimumSystemVersion raw "$root/app/Info.plist")
sdk_version=$(/usr/bin/xcrun --sdk macosx --show-sdk-version)
cargo_bin=${CARGO:-}

if [ -z "$cargo_bin" ]; then
  cargo_bin=$(command -v cargo 2>/dev/null || true)
fi
if [ -z "$cargo_bin" ] && [ -x "$HOME/.cargo/bin/cargo" ]; then
  cargo_bin="$HOME/.cargo/bin/cargo"
fi
if [ -z "$cargo_bin" ] || ! command -v "$cargo_bin" >/dev/null 2>&1; then
  echo "cargo was not found; install Rust 1.100.0 or set CARGO to its absolute path." >&2
  exit 1
fi

if [ -z "$identity" ]; then
  identity=$(security find-identity -v -p codesigning \
    | sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p' \
    | head -1)
fi
if [ -z "$identity" ]; then
  echo "No Developer ID Application identity found; set FRACTILE_CODESIGN_IDENTITY." >&2
  exit 1
fi
case "$identity" in
  "Developer ID Application:"*) ;;
  *)
    echo "FRACTILE_CODESIGN_IDENTITY must be a Developer ID Application identity, not '$identity'." >&2
    exit 1
    ;;
esac

CARGO_TARGET_DIR="$root/target" \
  "$cargo_bin" build --manifest-path "$root/Cargo.toml" --release --locked --workspace --bins
# A global MACOSX_DEPLOYMENT_TARGET also applies to host-side proc-macro
# dylibs, which must remain loadable by the build host. Set the deployment
# command only on the final app executables before they are signed.
for executable in fractile-agent fractile; do
  path="$root/target/release/$executable"
  patched="$path.deployment-target"
  /usr/bin/vtool -set-build-version macos "$deployment_target" "$sdk_version" \
    -replace -output "$patched" "$path"
  /bin/chmod 755 "$patched"
  /bin/mv "$patched" "$path"
done
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$root/target/release/fractile-agent" "$bundle/Contents/MacOS/fractile-agent"
cp "$root/target/release/fractile" "$bundle/Contents/MacOS/fractile"
cp "$root/app/Info.plist" "$bundle/Contents/Info.plist"
rm -rf "$icon_output"
mkdir -p "$icon_output"
# actool ships only with the full Xcode app. Without it, build with the generic app icon.
if xcrun --find actool >/dev/null 2>&1; then
  xcrun actool "$root/app/AppIcon.icon" \
    --compile "$icon_output" \
    --platform macosx \
    --minimum-deployment-target "$deployment_target" \
    --app-icon AppIcon \
    --standalone-icon-behavior all \
    --output-partial-info-plist "$icon_info" \
    --output-format human-readable-text \
    --warnings \
    --notices >&2
  cp "$icon_output/AppIcon.icns" "$icon_output/Assets.car" "$bundle/Contents/Resources/"
else
  echo "warning: actool not found (needs full Xcode); building without the app icon" >&2
fi
plutil -lint "$bundle/Contents/Info.plist" >/dev/null
codesign --force --timestamp --options runtime --sign "$identity" \
  "$bundle/Contents/MacOS/fractile-agent"
codesign --force --timestamp --options runtime --sign "$identity" \
  "$bundle/Contents/MacOS/fractile"
codesign --force --timestamp --options runtime --sign "$identity" \
  --identifier app.fractile \
  "$bundle"
codesign --verify --deep --strict "$bundle"

printf '%s\n' "$bundle"
