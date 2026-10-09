#!/bin/sh
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
script="$root/scripts/setup.sh"

fail() {
    printf '%s\n' "$1" >&2
    exit 1
}

help=$(bash "$script" --help) || fail 'Expected setup.sh --help to succeed'
printf '%s\n' "$help" | grep -q -- '--features' \
    || fail 'Expected --help to list --features'
printf '%s\n' "$help" | grep -q '16 GB RAM' \
    || fail 'Expected --help to state the 16 GB RAM floor for vision/all'
printf '%s\n' "$help" | grep -q '145 GB' \
    || fail 'Expected --help to state the full-install disk need'

if bash "$script" --features; then
    fail 'Expected --features without a value to fail'
fi

if bash "$script" --not-a-flag; then
    fail 'Expected unknown flags to fail'
fi

printf '%s\n' 'Setup script checks passed'
