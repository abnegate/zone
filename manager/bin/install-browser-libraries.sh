#!/bin/sh
# Installs the Debian packages carrying the shared libraries Playwright's
# browsers link against, then checks they actually arrived.
#
# Run by manager/Dockerfile when building the image, and by the manager-image
# workflow against the same base image on any pull request that touches this.
# Without the workflow a wrong package name would reach whoever next built the
# image; without the check, a package that resolves but stops delivering a
# library would produce an image whose browsers die at launch.
#
# Takes the directory holding browser-packages.txt and browser-libraries.txt.
set -eu

directory=${1:-$(dirname "$0")/..}

entries() {
    grep -vE '^[[:space:]]*(#|$)' "$directory/$1"
}

packages=$(entries browser-packages.txt | tr '\n' ' ')
libraries=$(entries browser-libraries.txt)

apt-get update

# shellcheck disable=SC2086
if ! apt-get install -y --no-install-recommends $packages; then
    # Debian's 64-bit time_t transition renamed several of these. Retrying
    # without the suffix turns a rename into a missing library, which the
    # check below names, rather than an unresolvable package name.
    echo "Some packages did not resolve; retrying without t64 suffixes." >&2
    # shellcheck disable=SC2086
    renamed=$(printf '%s\n' $packages | sed 's/t64$//' | tr '\n' ' ')
    # shellcheck disable=SC2086
    apt-get install -y --no-install-recommends $renamed
fi

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
