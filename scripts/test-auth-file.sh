#!/bin/sh
# shellcheck disable=SC2016
set -eu

unset SECURITY_BASIC_AUTH_USERS_FILE ZONE_ENV_FILE AUTH_ROOT_TEST_ONLY

source=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
directory=$(mktemp -d)
trap 'rm -rf "$directory"' EXIT HUP INT TERM
sandbox=$directory/zone
script=$sandbox/scripts/auth-file.sh
default=$sandbox/auth/users.htpasswd

mkdir -p "$sandbox/scripts"
cp "$source/auth-file.sh" "$source/env-file.sh" "$sandbox/scripts/"

fail() {
    printf '%s\n' "$1" >&2
    if [ -f "$directory/output" ]; then
        sed 's/^/  output: /' "$directory/output" >&2
    fi
    exit 1
}

run() {
    status=0
    sh "$script" "$@" > "$directory/output" 2>&1 || status=$?
    return "$status"
}

assert_output() {
    grep -qF -- "$1" "$directory/output" || fail "Expected output containing: $1"
}

reset() {
    rm -rf "$sandbox/auth" "$sandbox/.env" "$directory/elsewhere"
}

reset
run path || fail 'Expected path to succeed'
[ "$(cat "$directory/output")" = "$default" ] || fail 'Expected the default users file under the project root'

printf '%s\n' 'AUTH_ROOT_TEST_ONLY=./custom' 'SECURITY_BASIC_AUTH_USERS_FILE=${AUTH_ROOT_TEST_ONLY}/users # comment' > "$sandbox/.env"
run path || fail 'Expected path to succeed with a .env override'
[ "$(cat "$directory/output")" = "$sandbox/custom/users" ] || fail 'Expected the interpolated .env path relative to the project root'

SECURITY_BASIC_AUTH_USERS_FILE=$directory/elsewhere/users
export SECURITY_BASIC_AUTH_USERS_FILE
run path || fail 'Expected path to succeed with an environment override'
[ "$(cat "$directory/output")" = "$directory/elsewhere/users" ] || fail 'Expected the environment to win over .env'
unset SECURITY_BASIC_AUTH_USERS_FILE

reset
run prepare || fail 'Expected prepare to succeed without an auth directory'
[ "$(cat "$directory/output")" = "$default" ] || fail 'Expected prepare to print the users file'
[ -d "$sandbox/auth" ] || fail 'Expected prepare to create the auth directory'
[ ! -e "$default" ] || fail 'Expected prepare to leave creating the file to htpasswd'

reset
mkdir -p "$default"
run prepare || fail 'Expected prepare to replace an empty directory'
[ ! -e "$default" ] || fail 'Expected the empty directory Docker created to be removed'
grep -qx -- "$default" "$directory/output" || fail 'Expected prepare to print the users file'
assert_output 'Removed the empty directory Docker left'

reset
mkdir -p "$default"
printf '%s\n' keep > "$default/keep"
if run prepare; then
    fail 'Expected prepare to refuse a directory with files in it'
fi
assert_output 'is a directory with files in it'
[ -f "$default/keep" ] || fail 'Expected prepare to leave a non-empty directory untouched'

reset
mkdir -p "$sandbox/auth"
if run check; then
    fail 'Expected check to fail without a users file'
fi
assert_output "Basic auth file $default is missing. Run 'make setup-auth' to create it."

mkdir "$default"
if run check; then
    fail 'Expected check to fail when the users file is a directory'
fi
assert_output "Basic auth file $default is a directory"
assert_output '--force-recreate traefik'

rmdir "$default"
: > "$default"
if run check; then
    fail 'Expected check to fail on an empty users file'
fi
assert_output "Basic auth file $default is empty"

printf '%s\n' 'admin:hash' > "$default"
run check || fail 'Expected check to pass on a users file with a user'
[ ! -s "$directory/output" ] || fail 'Expected a passing check to print nothing'

if run unknown; then
    fail 'Expected an unknown command to fail'
fi
assert_output 'Usage:'

printf '%s\n' 'auth-file tests passed'
