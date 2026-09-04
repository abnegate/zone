#!/bin/sh
set -eu

file=${1:-.env}
if [ ! -f "$file" ]; then
    printf '%s\n' 'Missing .env file. Run make setup first.' >&2
    exit 1
fi

# Keep the selection across Compose rebuilds and Manager recreation.
temporary=$(mktemp "${file}.XXXXXX")
trap 'rm -f "$temporary"' EXIT HUP INT TERM
awk '
    /^[[:space:]]*(export[[:space:]]+)?MODEL_SEARCH_PROXY_URL[[:space:]]*=/ {
        if (!written) print "MODEL_SEARCH_PROXY_URL=http://gluetun:8888"
        written = 1
        next
    }
    { print }
    END { if (!written) print "MODEL_SEARCH_PROXY_URL=http://gluetun:8888" }
' "$file" > "$temporary"
mv "$temporary" "$file"
