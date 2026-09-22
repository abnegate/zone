#!/bin/sh
# Move an existing install's database cluster out of the anonymous volume the
# pgvector image created at /var/lib/postgresql/data (when docker-compose.yml
# mounted zone_postgres_data one level up) into zone_postgres_data itself,
# which is where the compose file now mounts it and where make backup looks.
#
# Run with the postgres container stopped but not removed: `make stop` or
# `docker compose stop postgres`. A removed container leaves its anonymous
# volume dangling, so that case is handled by looking for a dangling volume
# that holds a cluster.
set -eu

target=${ZONE_POSTGRES_VOLUME:-zone_postgres_data}
pgdata=/var/lib/postgresql/data

holds_cluster() {
    docker run --rm -v "$1:/volume:ro" alpine test -f /volume/PG_VERSION >/dev/null 2>&1
}

if docker ps --format '{{.Names}}' | grep -qx postgres; then
    printf '%s\n' 'postgres is running; stop it first (make stop) so the cluster is not copied mid-write' >&2
    exit 1
fi

source=${ZONE_PGDATA_SOURCE:-}
if [ -z "$source" ]; then
    source=$(docker inspect postgres \
        --format "{{range .Mounts}}{{if eq .Destination \"$pgdata\"}}{{.Name}}{{end}}{{end}}" \
        2>/dev/null || true)
fi

if [ -z "$source" ] || [ "$source" = "$target" ]; then
    source=''
    for candidate in $(docker volume ls -q -f dangling=true | grep -E '^[0-9a-f]{64}$' || true); do
        if holds_cluster "$candidate"; then
            if [ -n "$source" ]; then
                printf '%s\n' "more than one dangling volume holds a cluster ($source, $candidate); pass the right one as ZONE_PGDATA_SOURCE" >&2
                exit 1
            fi
            source=$candidate
        fi
    done
fi

if [ -z "$source" ]; then
    printf '%s\n' "no anonymous PGDATA volume found; nothing to move (a fresh install already keeps its cluster in $target)"
    exit 0
fi

if ! holds_cluster "$source"; then
    printf '%s\n' "$source holds no PG_VERSION; refusing to copy something that is not a cluster" >&2
    exit 1
fi

if holds_cluster "$target"; then
    printf '%s\n' "$target already holds a cluster; refusing to overwrite it. Remove it first if it is the empty one a fresh start created: docker volume rm $target" >&2
    exit 1
fi

printf '%s\n' "copying the cluster from $source into $target..."
docker run --rm -v "$source:/source:ro" -v "$target:/target" alpine sh -c 'rmdir /target/data 2>/dev/null; cp -a /source/. /target/'
printf '%s\n' "done. Start the stack (make up), check the data is there, then remove the old volume: docker volume rm $source"
