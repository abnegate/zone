#!/bin/sh
# The manager image builds the runner workspace, and Cargo reads every
# member's manifest before it builds any one package, so a member missing
# from the build context fails the build. This checks that every member
# named in runner/Cargo.toml reaches the image: either a whole-runner COPY
# or one COPY line per member.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
manifest=${CARGO_MANIFEST:-$root/runner/Cargo.toml}
dockerfile=${DOCKERFILE:-$root/manager/Dockerfile}
dockerignore=${DOCKERIGNORE:-$root/.dockerignore}

members=$(sed -n 's/^members = \[\(.*\)\]/\1/p' "$manifest" | tr ',' '\n' | tr -d ' "')
if [ -z "$members" ]; then
    printf '%s\n' "no [workspace].members found in $manifest" >&2
    exit 1
fi

for member in $members; do
    if [ ! -f "$root/runner/$member/Cargo.toml" ]; then
        printf '%s\n' "workspace member $member has no runner/$member/Cargo.toml" >&2
        exit 1
    fi
done

copies_whole_runner() {
    grep -Eq '^COPY[[:space:]]+runner/?[[:space:]]+(\./?|/build/runner/?)[[:space:]]*$' "$dockerfile"
}

if copies_whole_runner; then
    for member in $members; do
        if grep -Eq "^runner/$member/?\$" "$dockerignore"; then
            printf '%s\n' ".dockerignore excludes workspace member runner/$member" >&2
            exit 1
        fi
    done
    printf '%s\n' "Dockerfile copies the whole runner workspace ($(printf '%s\n' "$members" | wc -l | tr -d ' ') members)"
    exit 0
fi

missing=''
for member in $members; do
    if ! grep -Eq "^COPY[[:space:]]+runner/$member(/[^[:space:]]*)?[[:space:]]" "$dockerfile"; then
        missing="$missing $member"
    fi
done

if [ -n "$missing" ]; then
    printf '%s\n' "manager/Dockerfile does not copy workspace member(s):$missing" >&2
    printf '%s\n' "add a COPY line for each, or copy runner/ whole" >&2
    exit 1
fi

printf '%s\n' 'Dockerfile copies every workspace member'
