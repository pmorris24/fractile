#!/usr/bin/env bash
# Renders named Settings scenes headless, so a UI change carries its before and after.
#   OUT=captures/after scripts/snap-set.sh general hud-customize layout
#   SIZES="920x660 860x580 1200x840"   window sizes (default: the size Settings opens at)
#   THEMES="dark light"                 appearances (default: dark)
#   TREE=1                              also dump each scene's element tree as JSON
# Files are <scene>-<width>[-light].png in $OUT. List the scenes with
# `cargo run -p fractile-agent -- --list-scenes`. See docs/design-system.md.
set -euo pipefail
cd "$(dirname "$0")/.."
out="${OUT:?set OUT to the folder to render into}"
sizes="${SIZES:-920x660}"
themes="${THEMES:-dark}"
mkdir -p "$out"
cargo build --locked -q -p fractile-agent
target="$(cargo metadata --locked --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
bin="$target/debug/fractile-agent"
for scene in "$@"; do
  for size in $sizes; do
    for theme in $themes; do
      name="$scene-${size%%x*}"
      [ "$theme" = dark ] || name="$name-$theme"
      extra=()
      if [ -n "${TREE:-}" ] && [ "$theme" = dark ]; then extra+=(--tree "$out/$name.json"); fi
      "$bin" --size "$size" --scene "$scene" --theme "$theme" --snap "$out/$name.png" \
        "${extra[@]+"${extra[@]}"}" >/dev/null 2>&1 || echo "failed: $scene at $size ($theme)" >&2
    done
  done
done
ls "$out"
