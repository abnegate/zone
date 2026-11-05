#!/usr/bin/env bash
# Copy the CLI into a Tauri-built Zone.app, sign it, and zip it.
# Usage: package-tauri.sh <version> <arch> <app> <zone-bin> <manager-ui> <outdir>
# Optional env: APPLE_SIGNING_IDENTITY signs with Developer ID instead of ad hoc;
# APPLE_API_KEY, APPLE_API_ISSUER and APPLE_API_KEY_PATH then notarize and staple.
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

archive="$outdir/Zone-${version}-darwin-${arch}.zip"
mkdir -p "$outdir"

if [[ -n ${APPLE_SIGNING_IDENTITY:-} ]]; then
  for binary in "$macos"/*; do
    codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$binary"
  done
  codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$app"
  codesign --verify --strict --verbose=2 "$app"
elif command -v codesign >/dev/null; then
  codesign --force --deep --sign - "$app" >/dev/null
fi

if [[ -n ${APPLE_SIGNING_IDENTITY:-} && -n ${APPLE_API_KEY_PATH:-} ]]; then
  : "${APPLE_API_KEY:?set APPLE_API_KEY}"
  : "${APPLE_API_ISSUER:?set APPLE_API_ISSUER}"
  ditto -c -k --keepParent "$app" "$archive"
  xcrun notarytool submit "$archive" \
    --key "$APPLE_API_KEY_PATH" \
    --key-id "$APPLE_API_KEY" \
    --issuer "$APPLE_API_ISSUER" \
    --wait
  xcrun stapler staple "$app"
  rm -f "$archive"
fi

ditto -c -k --keepParent "$app" "$archive"
echo "wrote $archive"
