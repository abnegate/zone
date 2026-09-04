#!/bin/sh
set -eu

directory=$(mktemp -d)
trap 'rm -rf "$directory"' EXIT HUP INT TERM
script=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/configure-model-proxy.sh

printf '%s\n' '# Keep comment' 'OTHER=value' 'MODEL_SEARCH_PROXY_URL=' \
    'export MODEL_SEARCH_PROXY_URL=http://old:8888' > "$directory/environment"
printf '%s\n' '# Keep comment' 'OTHER=value' \
    'MODEL_SEARCH_PROXY_URL=http://gluetun:8888' > "$directory/expected"

sh "$script" "$directory/environment"
cmp "$directory/environment" "$directory/expected"
sh "$script" "$directory/environment"
cmp "$directory/environment" "$directory/expected"

printf '%s\n' '# Keep comment' 'OTHER=value' > "$directory/environment"
sh "$script" "$directory/environment"
cmp "$directory/environment" "$directory/expected"

if sh "$script" "$directory/missing" 2>/dev/null; then
    printf '%s\n' 'Expected missing environment file to be rejected' >&2
    exit 1
fi
test ! -e "$directory/missing"
printf '%s\n' 'Model proxy persistence checks passed'
