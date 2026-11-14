#!/bin/sh
# Builds the manager image through its package layers, the part that adds system
# users, and checks zone still holds the uid and gid existing volumes carry.
# Stops short of the language toolchains, which add no users.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
dockerfile=${DOCKERFILE:-$root/manager/Dockerfile}
tag=zone-manager-user-test:$$
pinned=999

trap 'docker image rm --force "$tag" >/dev/null 2>&1 || true' EXIT

if ! grep -q '^FROM runtime-packages AS runtime$' "$dockerfile"; then
    printf '%s\n' "the runtime stage of $dockerfile should build on runtime-packages, or this checks a stage the image does not use" >&2
    exit 1
fi

docker build --progress=plain --file "$dockerfile" --target runtime-packages --tag "$tag" "$root"

actual=$(docker run --rm "$tag" sh -c '
    printf "uid=%s gid=%s passwd=%s group=%s home=%s\n" \
        "$(id -u zone)" \
        "$(id -g zone)" \
        "$(getent passwd '"$pinned"' | cut -d: -f1)" \
        "$(getent group '"$pinned"' | cut -d: -f1)" \
        "$(stat -c %u:%g /home/zone)"
')
expected="uid=$pinned gid=$pinned passwd=zone group=zone home=$pinned:$pinned"

if [ "$actual" != "$expected" ]; then
    printf 'zone drifted from the id existing volumes carry\n expected: %s\n      got: %s\n' "$expected" "$actual" >&2
    exit 1
fi
printf '%s\n' "zone is uid and gid $pinned after every package layer."
