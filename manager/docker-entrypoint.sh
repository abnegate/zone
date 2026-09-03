#!/bin/sh
set -eu

# Named volumes created before the image contained /app/artifacts are root-owned.
# Reclaim that directory for the service user without opening it to others.
if [ "$(id -u)" -eq 0 ]; then
    if [ -d /app/artifacts ]; then
        chown zone:zone /app/artifacts
        chmod 0755 /app/artifacts
    fi
    exec runuser -u zone -- "$@"
fi

exec "$@"
