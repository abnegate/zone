#!/bin/sh
set -eu

file=${1:-.env}
mode=${2:-vpn}
case "$mode" in
    vpn)
        proxy=http://gluetun:8888
        vpn=1
        ;;
    direct)
        proxy=
        vpn=
        ;;
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
# ZONE_VPN=1 also selects docker-compose.vpn.yml (full tunnel).
temporary=$(mktemp "${file}.XXXXXX")
trap 'rm -f "$temporary"' EXIT HUP INT TERM
awk -v proxy="$proxy" -v vpn="$vpn" '
    /^[[:space:]]*(export[[:space:]]+)?(MODEL_SEARCH_PROXY_URL|TOOL_RUNNER_PROXY_URL|ZONE_VPN)[[:space:]]*=/ {
        name = $0
        sub(/^[[:space:]]*(export[[:space:]]+)?/, "", name)
        sub(/[[:space:]]*=.*/, "", name)
        if (!written[name]) {
            if (name == "ZONE_VPN") print "ZONE_VPN=" vpn
            else print name "=" proxy
        }
        written[name] = 1
        next
    }
    { print }
    END {
        if (!written["MODEL_SEARCH_PROXY_URL"]) print "MODEL_SEARCH_PROXY_URL=" proxy
        if (!written["TOOL_RUNNER_PROXY_URL"]) print "TOOL_RUNNER_PROXY_URL=" proxy
        if (!written["ZONE_VPN"]) print "ZONE_VPN=" vpn
    }
' "$file" > "$temporary"
mv "$temporary" "$file"
