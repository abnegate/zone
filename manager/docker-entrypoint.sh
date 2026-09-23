#!/bin/sh
set -eu

# Named volumes created before the image contained these directories are
# root-owned. Reclaim each one for the service user without opening it to others.
reclaim() {
    directory=$1
    mode=$2
    if [ -d "$directory" ]; then
        chown zone:zone "$directory"
        chmod "$mode" "$directory"
    fi
}

if [ "$(id -u)" -eq 0 ]; then
    reclaim /app/artifacts 0755
    reclaim /app/agent-state 0700
    exec runuser -u zone -- "$@"
fi

exec "$@"
