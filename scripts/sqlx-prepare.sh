#!/bin/sh
# Regenerate zone_server's sqlx offline query cache (runner/zone_server/.sqlx),
# or with --check, fail when it is missing a query, holds a stale or
# hand-written entry, or holds an entry no query uses.
#
# Uses sqlx-cli at the sqlx version runner/Cargo.lock pins, installing it into
# runner/target/sqlx-cli/<version> when the sqlx on PATH is another version.
# DATABASE_URL must name a Postgres database; pending migrations are applied.
# CARGO overrides the cargo binary every step runs.
#
#   DATABASE_URL=postgres://localhost/zone scripts/sqlx-prepare.sh
#   DATABASE_URL=postgres://localhost/zone scripts/sqlx-prepare.sh --check
set -eu

usage='usage: scripts/sqlx-prepare.sh [--check]'
check=false
case "${1:-}" in
    '') ;;
    --check) check=true ;;
    *)
        printf '%s\n' "$usage" >&2
        exit 2
        ;;
esac

: "${DATABASE_URL:?set DATABASE_URL to the Postgres database to describe queries against}"
export DATABASE_URL

repository=$(cd "$(dirname "$0")/.." && pwd)
runner="$repository/runner"
server="$runner/zone_server"
cargo=${CARGO:-cargo}

version=$(sed -n '/^name = "sqlx"$/{n;s/^version = "\(.*\)"$/\1/p;}' "$runner/Cargo.lock")
if [ -z "$version" ]; then
    printf '%s\n' "no sqlx entry in $runner/Cargo.lock" >&2
    exit 1
fi

provides_version() {
    [ -x "$1/cargo-sqlx" ] && [ "$("$1/sqlx" --version 2>/dev/null)" = "sqlx-cli $version" ]
}

list_queries() {
    find "$1" -maxdepth 1 -name 'query-*.json' -exec basename {} \; | LC_ALL=C sort
}

bin=
if path_sqlx=$(command -v sqlx 2>/dev/null) && provides_version "$(dirname "$path_sqlx")"; then
    bin=$(dirname "$path_sqlx")
else
    root="$runner/target/sqlx-cli/$version"
    bin="$root/bin"
    if ! provides_version "$bin"; then
        (cd "$runner" && "$cargo" install sqlx-cli --version "$version" --locked \
            --no-default-features --features postgres,rustls,sqlx-toml --root "$root")
    fi
fi

cd "$server"
"$bin/sqlx" migrate run

if [ "$check" = false ]; then
    rm -rf "$runner/.sqlx"
    CARGO="$cargo" "$bin/cargo-sqlx" sqlx prepare -- --all-targets --no-default-features
    exit 0
fi

if [ -d "$runner/.sqlx" ]; then
    printf '%s\n' "runner/.sqlx exists; the query macros fall back to it for any query missing from zone_server/.sqlx, so delete it and run scripts/sqlx-prepare.sh" >&2
    exit 1
fi

CARGO="$cargo" "$bin/cargo-sqlx" sqlx prepare --check -- --all-targets --no-default-features

target=$("$cargo" metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
if [ ! -d "$target/sqlx-prepare-check" ]; then
    printf '%s\n' "sqlx prepare --check left no generated cache under '$target'" >&2
    exit 1
fi
listing=$(mktemp -d)
trap 'rm -rf "$listing"' EXIT
list_queries "$target/sqlx-prepare-check" > "$listing/generated"
list_queries .sqlx > "$listing/committed"
unused=$(LC_ALL=C comm -13 "$listing/generated" "$listing/committed")
if [ -n "$unused" ]; then
    printf '%s\n' 'runner/zone_server/.sqlx holds entries no query uses; run scripts/sqlx-prepare.sh:' "$unused" >&2
    exit 1
fi
printf '%s\n' "runner/zone_server/.sqlx matches the queries (sqlx-cli $version)"
