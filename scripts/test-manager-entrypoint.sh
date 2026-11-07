#!/bin/sh
# Runs the manager's entrypoint as root against volumes left by an image whose
# zone user had another uid, on the same base image the manager is built from.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
dockerfile=${DOCKERFILE:-$root/manager/Dockerfile}
entrypoint=${ENTRYPOINT:-$root/manager/docker-entrypoint.sh}

image=$(sed -nE 's/^FROM ([^ ]+) AS runtime-base$/\1/p' "$dockerfile")
if [ -z "$image" ]; then
    printf '%s\n' "no runtime-base stage found in $dockerfile" >&2
    exit 1
fi

docker run --rm --interactive \
    --volume "$entrypoint:/entrypoint.sh:ro" \
    --volume /app/agent-state \
    --volume /app/artifacts \
    --volume /app/embed-cache \
    --tmpfs /app/agent-state/nested \
    "$image" sh -s <<'FIXTURE'
set -eu

failures=0

fail() {
    printf 'FAIL: %s\n' "$1" >&2
    failures=$((failures + 1))
}

assert_owner() {
    actual=$(stat -c '%u:%g' -- "$1" 2>/dev/null) || actual=missing
    [ "$actual" = "$2" ] || fail "$1 should be owned by $2, is $actual"
}

assert_mode() {
    actual=$(stat -c '%a' -- "$1" 2>/dev/null) || actual=missing
    [ "$actual" = "$2" ] || fail "$1 should have mode $2, has $actual"
}

zone=999:999
stale=996:996
foreign=1234:1234

groupadd --system --gid 999 zone
useradd --system --uid 999 --gid zone --create-home zone

login=/app/agent-state/org/claude/logins/id
mkdir -p "$login" /app/workspace /tmp/repos/clone /outside/directory
printf 'token' > "$login/credentials"
chmod 0600 "$login/credentials"
chmod -R u=rwX,go= /app/agent-state/org
printf 'secret' > /outside/secret
printf 'kept' > /outside/directory/child
chown -R "$foreign" /outside
chmod 0640 /outside/secret
chmod 0750 /outside/directory
ln -s /outside/secret /app/agent-state/org/file-link
ln -s /outside/directory /app/agent-state/org/directory-link
chown -hR "$stale" /app/agent-state/org

printf 'mounted' > /app/agent-state/nested/kept
chown "$foreign" /app/agent-state/nested/kept

printf 'image' > '/app/artifacts/a file with spaces'
chown "$stale" '/app/artifacts/a file with spaces'
printf 'settled' > /app/artifacts/settled
chown "$zone" /app/artifacts/settled
settled=$(stat -c '%z' /app/artifacts/settled)

mkdir -p /app/embed-cache/models
printf 'weights' > /app/embed-cache/models/weights
printf 'group' > /app/embed-cache/group-only
chown "999:996" /app/embed-cache/group-only

printf 'source' > /tmp/repos/clone/run.sh
chmod 0750 /tmp/repos/clone
chmod 0755 /tmp/repos/clone/run.sh
chown -R "$stale" /tmp/repos/clone

printf 'layer' > /app/workspace/untouched
chown "$foreign" /app/workspace/untouched

chown root:root /app/artifacts /app/embed-cache
chown "$stale" /tmp/repos
chown "$zone" /app/agent-state
chmod 0777 /app/agent-state /app/artifacts /app/embed-cache /tmp/repos

if ! sh /entrypoint.sh sh -c "
    test \"\$(id -u)\" = 999 \
    && cat '$login/credentials' >/dev/null \
    && touch '$login/written' /app/embed-cache/models/written /tmp/repos/clone/written
"; then
    fail 'zone could not reach and write what the entrypoint reclaimed'
fi

assert_owner /app/agent-state "$zone"
assert_owner /app/agent-state/org "$zone"
assert_owner "$login" "$zone"
assert_owner "$login/credentials" "$zone"
assert_owner '/app/artifacts/a file with spaces' "$zone"
assert_owner /app/embed-cache "$zone"
assert_owner /app/embed-cache/models "$zone"
assert_owner /app/embed-cache/models/weights "$zone"
assert_owner /app/embed-cache/group-only "$zone"
assert_owner /tmp/repos "$zone"
assert_owner /tmp/repos/clone/run.sh "$zone"

assert_mode /app/agent-state 700
assert_mode /app/artifacts 755
assert_mode /app/embed-cache 755
assert_mode /tmp/repos 700
assert_mode "$login" 700
assert_mode "$login/credentials" 600
assert_mode /tmp/repos/clone 750
assert_mode /tmp/repos/clone/run.sh 755

[ "$(stat -c '%z' /app/artifacts/settled)" = "$settled" ] \
    || fail 'a file already owned by zone should not have been chowned again'

assert_owner /app/agent-state/org/file-link "$zone"
assert_owner /app/agent-state/org/directory-link "$zone"
assert_owner /outside "$foreign"
assert_owner /outside/secret "$foreign"
assert_owner /outside/directory "$foreign"
assert_owner /outside/directory/child "$foreign"
assert_mode /outside/secret 640
assert_mode /outside/directory 750

assert_owner /app/agent-state/nested/kept "$foreign"
assert_owner /app/workspace/untouched "$foreign"

rm -rf /tmp/repos
ln -s /outside/directory /tmp/repos
if ! sh /entrypoint.sh true; then
    fail 'the entrypoint should start when a reclaimed path is a symlink'
fi
assert_owner /tmp/repos 0:0
assert_owner /outside/directory "$foreign"
assert_mode /outside/directory 750

if [ "$failures" -ne 0 ]; then
    printf '%s\n' "$failures failed" >&2
    exit 1
fi
printf '%s\n' 'The entrypoint recovers every volume and stays inside them.'
FIXTURE
