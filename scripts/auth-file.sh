#!/bin/sh
# Traefik's basic-auth users file, as docker-compose.yml bind-mounts it.
#
#   sh scripts/auth-file.sh path      print the resolved path
#   sh scripts/auth-file.sh prepare   clear the way for htpasswd -c and print the path
#   sh scripts/auth-file.sh check     fail unless the file holds at least one user
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)

# shellcheck source=SCRIPTDIR/env-file.sh
. "$root/scripts/env-file.sh"

ZONE_ENV_FILE=${ZONE_ENV_FILE:-$root/.env}
DEFAULT_FILE=./auth/users.htpasswd
SETUP_HINT="Run 'make setup-auth' to create it."
RECREATE_HINT="then recreate Traefik with './scripts/compose.sh up -d --force-recreate traefik'."

resolve() {
    file=${SECURITY_BASIC_AUTH_USERS_FILE:-}
    if [ -z "$file" ]; then
        file=$(read_env_value SECURITY_BASIC_AUTH_USERS_FILE "$ZONE_ENV_FILE")
    fi
    file=${file:-$DEFAULT_FILE}
    case "$file" in
        /*) printf '%s\n' "$file" ;;
        *) printf '%s\n' "$root/${file#./}" ;;
    esac
}

is_empty_directory() {
    [ -z "$(ls -A -- "$1")" ]
}

prepare() {
    file=$(resolve)
    if [ -d "$file" ]; then
        if ! is_empty_directory "$file"; then
            printf '%s\n' "$file is a directory with files in it, not a users file. Move it aside and try again." >&2
            exit 1
        fi
        if ! rmdir -- "$file"; then
            printf '%s\n' "Could not remove the empty directory at $file. Docker may own it: remove it with 'sudo rmdir $file' and try again." >&2
            exit 1
        fi
        printf '%s\n' "Removed the empty directory Docker left at $file when the users file was missing." >&2
    fi
    mkdir -p -- "$(dirname -- "$file")"
    printf '%s\n' "$file"
}

check() {
    file=$(resolve)
    if [ -d "$file" ]; then
        printf '%s\n' "Basic auth file $file is a directory: Docker created it because the file was missing, and Traefik rejects every login until it is a file. Run 'make setup-auth', $RECREATE_HINT"
        exit 1
    fi
    if [ ! -e "$file" ]; then
        printf '%s\n' "Basic auth file $file is missing. $SETUP_HINT"
        exit 1
    fi
    if [ ! -s "$file" ]; then
        printf '%s\n' "Basic auth file $file is empty, so Traefik accepts no login. $SETUP_HINT"
        exit 1
    fi
}

case "${1:-}" in
    path) resolve ;;
    prepare) prepare ;;
    check) check ;;
    *)
        printf '%s\n' "Usage: $0 path|prepare|check" >&2
        exit 2
        ;;
esac
