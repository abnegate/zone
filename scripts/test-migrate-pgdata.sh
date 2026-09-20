#!/bin/sh
# A fake docker on PATH: two dangling volumes both hold a cluster, the named
# volume is empty, and no postgres container exists. ZONE_PGDATA_SOURCE must
# pick one without the script stopping at the ambiguity.
set -eu
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
fake=$(mktemp -d)
trap 'rm -rf "$fake"' EXIT
cat >"$fake/docker" <<'FAKE'
#!/bin/sh
log=${FAKE_DOCKER_LOG:?}
echo "$*" >>"$log"
case "$1 $2" in
  "ps --format") exit 0 ;;
  "inspect postgres") exit 1 ;;
  "volume ls") printf '%s\n' aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb ;;
  "run --rm")
    case "$*" in
      *zone_postgres_data:/volume:ro*) exit 1 ;;
      *"/volume:ro alpine test"*) exit 0 ;;
      *) exit 0 ;;
    esac ;;
  *) exit 0 ;;
esac
FAKE
chmod +x "$fake/docker"
export FAKE_DOCKER_LOG=$fake/calls.log
: >"$FAKE_DOCKER_LOG"

if PATH="$fake:$PATH" sh "$here/migrate-pgdata.sh" >"$fake/out" 2>&1; then
    echo "without ZONE_PGDATA_SOURCE the ambiguity must stop the script" >&2
    exit 1
fi
grep -q 'more than one dangling volume' "$fake/out" || { cat "$fake/out" >&2; exit 1; }

: >"$FAKE_DOCKER_LOG"
PATH="$fake:$PATH" ZONE_PGDATA_SOURCE=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb sh "$here/migrate-pgdata.sh" >"$fake/out" 2>&1 || { cat "$fake/out" >&2; exit 1; }
grep -q 'copying the cluster from bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb into zone_postgres_data' "$fake/out" || { cat "$fake/out" >&2; exit 1; }
if grep -q 'volume ls' "$FAKE_DOCKER_LOG"; then
    echo "a named source must not scan the dangling volumes" >&2
    exit 1
fi
echo "migrate-pgdata: ZONE_PGDATA_SOURCE bypasses discovery"
