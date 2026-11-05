#!/usr/bin/env bash
# Point the Tauri Android project's release build at an upload keystore.
# Usage: configure-android-signing.sh <android-project> <keystore>
# Required env: ANDROID_KEYSTORE_PASSWORD, ANDROID_KEY_ALIAS, ANDROID_KEY_PASSWORD
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: configure-android-signing.sh <android-project> <keystore>" >&2
  exit 2
fi

project=$1
keystore=$2

: "${ANDROID_KEYSTORE_PASSWORD:?set ANDROID_KEYSTORE_PASSWORD}"
: "${ANDROID_KEY_ALIAS:?set ANDROID_KEY_ALIAS}"
: "${ANDROID_KEY_PASSWORD:?set ANDROID_KEY_PASSWORD}"

if [[ ! -f $project/gradle.properties ]]; then
  echo "missing Android project: $project/gradle.properties" >&2
  exit 1
fi
if [[ ! -s $keystore ]]; then
  echo "missing keystore: $keystore" >&2
  exit 1
fi

property() {
  printf '%s=%s\n' "$1" "${2//\\/\\\\}"
}

{
  echo
  property android.injected.signing.store.file "$keystore"
  property android.injected.signing.store.password "$ANDROID_KEYSTORE_PASSWORD"
  property android.injected.signing.key.alias "$ANDROID_KEY_ALIAS"
  property android.injected.signing.key.password "$ANDROID_KEY_PASSWORD"
} >>"$project/gradle.properties"
echo "configured release signing in $project/gradle.properties"
