#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 6 ]]; then
  echo "usage: package-deb.sh <version> <arch> <zone-bin> <desktop-bin> <manager-ui> <outdir>" >&2
  exit 2
fi

version=$1
arch=$2
zone_bin=$3
desktop_bin=$4
manager_ui=$5
outdir=$6
root="$(cd "$(dirname "$0")/../.." && pwd)"

for path in "$zone_bin" "$desktop_bin" "$manager_ui/index.html"; do
  if [[ ! -e $path ]]; then
    echo "missing: $path" >&2
    exit 1
  fi
done

workdir=$(mktemp -d)
trap 'rm -rf "$workdir"' EXIT

pkg="zone_${version}-1_${arch}"
prefix="$workdir/$pkg"
mkdir -p \
  "$prefix/DEBIAN" \
  "$prefix/usr/bin" \
  "$prefix/usr/lib/zone" \
  "$prefix/usr/share/zone/manager" \
  "$prefix/usr/share/applications" \
  "$prefix/usr/share/icons/hicolor/512x512/apps"

cp "$zone_bin" "$prefix/usr/bin/zone"
cp "$desktop_bin" "$prefix/usr/bin/zone-desktop"
chmod 755 "$prefix/usr/bin/zone" "$prefix/usr/bin/zone-desktop"
ln -s zone-desktop "$prefix/usr/bin/zone-app"
cp -R "$manager_ui"/. "$prefix/usr/share/zone/manager/"
mkdir -p "$prefix/usr/lib/zone"
ln -s /usr/share/zone/manager "$prefix/usr/lib/zone/manager"
cp "$root/packaging/linux/zone.desktop" "$prefix/usr/share/applications/zone.desktop"
if [[ -f $root/manager/frontend/public/logo512.png ]]; then
  cp "$root/manager/frontend/public/logo512.png" "$prefix/usr/share/icons/hicolor/512x512/apps/zone.png"
fi

size_kb=$(du -sk "$prefix/usr" | awk '{print $1}')

cat >"$prefix/DEBIAN/control" <<EOF
Package: zone
Version: ${version}-1
Section: utils
Priority: optional
Architecture: ${arch}
Depends: libc6 (>= 2.35), libssl3, libwebkit2gtk-4.1-0, libgtk-3-0
Maintainer: Jake Barnby <jakeb994@gmail.com>
Installed-Size: ${size_kb}
Homepage: https://github.com/abnegate/zone
Description: Zone desktop app and CLI
 Tauri desktop client for the Zone self-hosted AI platform, plus the
 zone CLI.
EOF

mkdir -p "$outdir"
dpkg-deb --build --root-owner-group "$prefix" "$outdir/${pkg}.deb"
echo "wrote $outdir/${pkg}.deb"
