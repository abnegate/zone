#!/bin/sh
set -eu

file=${1:-.env}
mode=${2:-vpn}
case "$mode" in
    vpn) proxy=http://gluetun:8888 ;;
    direct) proxy= ;;
    *)
        printf '%s\n' 'Expected mode vpn or direct.' >&2
        exit 1
        ;;
esac
if [ ! -f "$file" ]; then
    printf '%s\n' 'Missing .env file. Run make setup first.' >&2
    exit 1
fi

# Keep the selection across Compose rebuilds and Manager recreation.
temporary=$(mktemp "${file}.XXXXXX")
trap 'rm -f "$temporary"' EXIT HUP INT TERM
awk -v proxy="$proxy" '
    /^[[:space:]]*(export[[:space:]]+)?MODEL_SEARCH_PROXY_URL[[:space:]]*=/ {
        if (!written) print "MODEL_SEARCH_PROXY_URL=" proxy
        written = 1
        next
    }
    { print }
    END { if (!written) print "MODEL_SEARCH_PROXY_URL=" proxy }
' "$file" > "$temporary"
mv "$temporary" "$file"
