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

ZONE_PATH="$PATH:/home/zone/.local/bin:/home/zone/.bun/bin:/home/zone/.cargo/bin:/home/zone/go/bin:/home/zone/.gem/bin:/home/zone/.config/composer/vendor/bin:/home/zone/.deno/bin:/opt/tools/bin"

if [ "$(id -u)" -eq 0 ]; then
    reclaim /app/artifacts 0755
    reclaim /app/agent-state 0700
    reclaim /tmp/repos 0700
    exec runuser -u zone -- env PATH="$ZONE_PATH" "$@"
fi

PATH="$ZONE_PATH" exec "$@"
