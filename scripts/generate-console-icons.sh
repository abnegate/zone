#!/usr/bin/env bash
# Rasterize the console favicon and PWA marks into the bitmap icons the browser and installers load.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
public="$root/manager/frontend/public"
favicon="$public/favicon.svg"
pwa="$public/logo-pwa.svg"

for source in "$favicon" "$pwa"; do
  if [[ ! -f $source ]]; then
    echo "missing console icon source: $source" >&2
    exit 1
  fi
done

ico="$(mktemp -d)"
png="$(mktemp -d)"
trap 'rm -rf "$ico" "$png"' EXIT

cd "$root/runner/zone_desktop"
PATH="$HOME/.cargo/bin:$PATH" bunx --bun @tauri-apps/cli@2 icon "$favicon" -o "$ico"
PATH="$HOME/.cargo/bin:$PATH" bunx --bun @tauri-apps/cli@2 icon "$pwa" -o "$png" -p 180,192,512

cp "$ico/icon.ico" "$public/favicon.ico"
cp "$png/180x180.png" "$public/apple-touch-icon.png"
cp "$png/192x192.png" "$public/logo192.png"
cp "$png/512x512.png" "$public/logo512.png"
echo "Wrote console icons to $public"
