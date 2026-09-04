#!/usr/bin/env bash
# Copy the CLI into a Tauri-built Zone.app and zip it.
# Usage: package-tauri.sh <version> <arch> <app> <zone-bin> <manager-ui> <outdir>
set -euo pipefail

if [[ $# -ne 6 ]]; then
  echo "usage: package-tauri.sh <version> <arch> <app> <zone-bin> <manager-ui> <outdir>" >&2
  exit 2
fi

version=$1
arch=$2
app=$3
zone_bin=$4
manager_ui=$5
outdir=$6

if [[ ! -d $app ]]; then
  echo "missing Zone.app: $app" >&2
  exit 1
fi
if [[ ! -f $manager_ui/index.html ]]; then
  echo "missing manager UI: $manager_ui/index.html" >&2
  exit 1
fi

macos="$app/Contents/MacOS"
resources="$app/Contents/Resources"
mkdir -p "$macos" "$resources/manager"
cp "$zone_bin" "$macos/zone"
chmod 755 "$macos/zone"
if [[ -x $macos/zone-desktop ]]; then
  chmod 755 "$macos/zone-desktop"
fi
cp -R "$manager_ui"/. "$resources/manager/"

if command -v codesign >/dev/null; then
  codesign --force --deep --sign - "$app" >/dev/null
fi

mkdir -p "$outdir"
ditto -c -k --keepParent "$app" "$outdir/Zone-${version}-darwin-${arch}.zip"
echo "wrote $outdir/Zone-${version}-darwin-${arch}.zip"
