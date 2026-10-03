#!/bin/sh
# Self-signed certificate whose SAN lists the local console hosts. Traefik's
# built-in default cert does not, and browsers then fail fetch() after the
# document has already loaded.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
directory=$root/certs
certificate=$directory/local.crt
key=$directory/local.key

mkdir -p "$directory"

if [ -f "$certificate" ] && [ -f "$key" ] \
    && openssl x509 -in "$certificate" -noout -text 2>/dev/null \
    | grep -q 'DNS:manager.localhost'; then
    exit 0
fi

openssl req -x509 -newkey rsa:2048 -sha256 -days 825 -nodes \
    -keyout "$key" \
    -out "$certificate" \
    -subj "/CN=Zone local" \
    -addext "subjectAltName=DNS:localhost,DNS:*.localhost,DNS:manager.localhost,DNS:webui.localhost,DNS:*.webui.localhost,DNS:manager.webui.localhost,DNS:traefik.webui.localhost,IP:127.0.0.1,IP:0:0:0:0:0:0:0:1"

chmod 600 "$key"
