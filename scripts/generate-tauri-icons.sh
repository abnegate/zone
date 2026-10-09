#!/usr/bin/env bash
# Rasterize the Zone launcher mark into Tauri desktop/Android/iOS icon sets.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
src="$root/runner/zone_desktop/icons/icon.svg"
src_night="$root/runner/zone_desktop/icons/icon-night.svg"
dest="$root/runner/zone_desktop/icons"

if [[ ! -f $src ]]; then
  echo "missing launcher icon: $src" >&2
  exit 1
fi
if [[ ! -f $src_night ]]; then
  echo "missing night launcher icon: $src_night" >&2
  exit 1
fi

out="$(mktemp -d)"
night="$(mktemp -d)"
trap 'rm -rf "$out" "$night"' EXIT

cd "$root/runner/zone_desktop"
PATH="$HOME/.cargo/bin:$PATH" bunx --bun @tauri-apps/cli@2 icon "$src" -o "$out" --ios-color '#1a1612'

cp "$out/32x32.png" "$out/128x128.png" "$out/128x128@2x.png" "$out/icon.png" "$out/icon.icns" "$out/icon.ico" "$dest/"
cp "$out/icon.png" "$dest/icon-512.png"
rm -rf "$dest/android"
cp -R "$out/android" "$dest/android"

PATH="$HOME/.cargo/bin:$PATH" bunx --bun @tauri-apps/cli@2 icon "$src_night" -o "$night" --ios-color '#1a1612'
for density in mdpi hdpi xhdpi xxhdpi xxxhdpi; do
  mkdir -p "$dest/android/mipmap-night-$density"
  cp "$night/android/mipmap-$density/"*.png "$dest/android/mipmap-night-$density/"
done
echo "Wrote Zone launcher icons to $dest"
