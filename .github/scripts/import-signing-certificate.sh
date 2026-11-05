#!/usr/bin/env bash
# Import a base64 .p12 into a temporary keychain that codesign searches.
# Usage: import-signing-certificate.sh <keychain>
# Required env: APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: import-signing-certificate.sh <keychain>" >&2
  exit 2
fi

keychain=$1

: "${APPLE_CERTIFICATE:?set APPLE_CERTIFICATE}"
: "${APPLE_CERTIFICATE_PASSWORD:?set APPLE_CERTIFICATE_PASSWORD}"

password="$(uuidgen)"
certificate="$(mktemp -t certificate).p12"
trap 'rm -f "$certificate"' EXIT

echo "$APPLE_CERTIFICATE" | base64 --decode >"$certificate"
security create-keychain -p "$password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
security import "$certificate" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" -f pkcs12 -A -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$password" "$keychain" >/dev/null
read -r -a existing < <(security list-keychains -d user | tr -d '"' | tr '\n' ' ')
security list-keychains -d user -s "$keychain" "${existing[@]}"
security find-identity -v -p codesigning "$keychain"
