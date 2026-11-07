#!/bin/sh
set -eu

# Named volumes can be root-owned, or hold files from an image whose zone had
# another uid. Agents write to these volumes and this runs as root, so nothing
# here may follow a symlink: -h and the -L test keep it inside the volume.
reclaim() {
    directory=$1
    mode=$2
    if [ -d "$directory" ] && [ ! -L "$directory" ]; then
        if ! find "$directory" -xdev \( ! -user zone -o ! -group zone \) -exec chown -h zone:zone {} +; then
            printf 'zone may not be able to reach the paths above in %s\n' "$directory" >&2
        fi
        chmod "$mode" "$directory"
    fi
}

ZONE_PATH="$PATH:/home/zone/.local/bin:/home/zone/.bun/bin:/home/zone/.cargo/bin:/home/zone/go/bin:/home/zone/.gem/bin:/home/zone/.config/composer/vendor/bin:/home/zone/.deno/bin:/home/zone/.dotnet/tools:/home/zone/.pub-cache/bin:/opt/flutter/bin:/opt/tools/bin"

if [ "$(id -u)" -eq 0 ]; then
    reclaim /app/artifacts 0755
    reclaim /app/agent-state 0700
    reclaim /app/embed-cache 0755
    reclaim /tmp/repos 0700
    exec runuser -u zone -- env PATH="$ZONE_PATH" "$@"
fi

PATH="$ZONE_PATH" exec "$@"
