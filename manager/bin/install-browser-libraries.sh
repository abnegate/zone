#!/bin/sh
# Installs the Debian packages carrying the shared libraries Playwright's
# browsers link against, then fails naming any library they did not deliver.
# Run by manager/Dockerfile and by the manager-image workflow against the same
# base image. Takes the directory holding browser-packages.txt and
# browser-libraries.txt.
set -eu

directory=${1:-$(dirname "$0")/..}

entries() {
    grep -vE '^[[:space:]]*(#|$)' "$directory/$1"
}

packages=$(entries browser-packages.txt)
libraries=$(entries browser-libraries.txt)

apt-get update
# shellcheck disable=SC2086
apt-get install -y --no-install-recommends $packages
rm -rf /var/lib/apt/lists/*

missing=''
for library in $libraries; do
    ldconfig -p | grep -qF "	$library (" || missing="$missing $library"
done

if [ -n "$missing" ]; then
    echo "Browsers need these, and the packages above did not deliver them:" >&2
    for library in $missing; do
        echo "    $library" >&2
    done
    exit 1
fi

echo "All $(echo "$libraries" | wc -l | tr -d ' ') browser libraries resolve."
