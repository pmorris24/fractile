#!/bin/sh
# Builds target/Spaceman.app and signs it with a Developer ID identity
# (SPACEMAN_CODESIGN_IDENTITY, or the first one in the keychain).
# SPACEMAN_RELEASE_UNSIGNED=1 skips signing: the Release workflow's build job
# makes the bundle that way and its sign job signs it (.github/release/).
# With it, SPACEMAN_RELEASE_CHANNEL (stable or dev) and SPACEMAN_RELEASE_VERSION
# stamp the channel and version the workflow publishes this build as
# (src/updater/stamp.rs); set both or neither.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
bundle="$root/target/Spaceman.app"
icon_output="$root/target/AppIcon.assets"
icon_info="$root/target/AppIcon-info.plist"
identity=${SPACEMAN_CODESIGN_IDENTITY:-}
unsigned=${SPACEMAN_RELEASE_UNSIGNED:-}
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

case "$unsigned" in
  ''|1) ;;
  *)
    echo "SPACEMAN_RELEASE_UNSIGNED must be 1 or unset, not '$unsigned'." >&2
    exit 1
    ;;
esac
if [ -z "$unsigned" ]; then
  if [ -z "$identity" ]; then
    identity=$(security find-identity -v -p codesigning \
      | sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p' \
      | head -1)
  fi
  if [ -z "$identity" ]; then
    echo "No Developer ID Application identity found; set SPACEMAN_CODESIGN_IDENTITY." >&2
    exit 1
  fi
  case "$identity" in
    "Developer ID Application:"*) ;;
    *)
      echo "SPACEMAN_CODESIGN_IDENTITY must be a Developer ID Application identity, not '$identity'." >&2
      exit 1
      ;;
  esac
fi

release_channel=${SPACEMAN_RELEASE_CHANNEL:-}
release_version=${SPACEMAN_RELEASE_VERSION:-}
case "$release_channel" in
  ''|stable|dev) ;;
  *)
    echo "SPACEMAN_RELEASE_CHANNEL must be stable, dev or unset, not '$release_channel'." >&2
    exit 1
    ;;
esac
case "$release_version" in
  '') ;;
  .*|*[!A-Za-z0-9.+_-]*)
    echo "SPACEMAN_RELEASE_VERSION must be a version name, not '$release_version'." >&2
    exit 1
    ;;
esac
if { [ -n "$release_channel" ] && [ -z "$release_version" ]; } ||
  { [ -z "$release_channel" ] && [ -n "$release_version" ]; }; then
  echo "Set both SPACEMAN_RELEASE_CHANNEL and SPACEMAN_RELEASE_VERSION, or neither." >&2
  exit 1
fi

# Only the Release workflow builds unsigned, and only its builds update themselves from
# anoma.ly/release (src/updater.rs); every other build is a source build that never checks.
if [ -n "$unsigned" ]; then
  export SPACEMAN_RELEASE_BUILD=1
  if [ -n "$release_channel" ]; then
    export SPACEMAN_RELEASE_CHANNEL="$release_channel" SPACEMAN_RELEASE_VERSION="$release_version"
  fi
else
  unset SPACEMAN_RELEASE_BUILD SPACEMAN_RELEASE_CHANNEL SPACEMAN_RELEASE_VERSION
fi
CARGO_TARGET_DIR="$root/target" \
  "$cargo_bin" build --manifest-path "$root/Cargo.toml" --release --locked --workspace --bins
# A global MACOSX_DEPLOYMENT_TARGET also applies to host-side proc-macro
# dylibs, which must remain loadable by the build host. Set the deployment
# command only on the final app executables before they are signed.
for executable in spaceman-agent spaceman; do
  path="$root/target/release/$executable"
  patched="$path.deployment-target"
  /usr/bin/vtool -set-build-version macos "$deployment_target" "$sdk_version" \
    -replace -output "$patched" "$path"
  /bin/chmod 755 "$patched"
  /bin/mv "$patched" "$path"
done
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$root/target/release/spaceman-agent" "$bundle/Contents/MacOS/spaceman-agent"
cp "$root/target/release/spaceman" "$bundle/Contents/MacOS/spaceman"
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
if [ -n "$unsigned" ]; then
  printf '%s\n' "$bundle"
  exit 0
fi
codesign --force --timestamp --options runtime --sign "$identity" \
  "$bundle/Contents/MacOS/spaceman-agent"
codesign --force --timestamp --options runtime --sign "$identity" \
  "$bundle/Contents/MacOS/spaceman"
codesign --force --timestamp --options runtime --sign "$identity" \
  --identifier anomaly.spaceman \
  "$bundle"
codesign --verify --deep --strict "$bundle"

printf '%s\n' "$bundle"
