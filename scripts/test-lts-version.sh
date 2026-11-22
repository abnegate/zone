#!/bin/sh
set -eu

script=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)/lts-version.sh
directory=$(mktemp -d)
trap 'rm -rf "$directory"' EXIT HUP INT TERM

HOME=$directory
GIT_CONFIG_NOSYSTEM=1
GIT_CONFIG_GLOBAL=/dev/null
GIT_AUTHOR_NAME=Zone
GIT_AUTHOR_EMAIL=zone@example.com
GIT_COMMITTER_NAME=Zone
GIT_COMMITTER_EMAIL=zone@example.com
export HOME GIT_CONFIG_NOSYSTEM GIT_CONFIG_GLOBAL GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL

repository=$directory/repository
mkdir "$repository"
cd "$repository"

git init --quiet
commit() {
    git commit --quiet --allow-empty --message "$1"
    git rev-parse HEAD
}

base=$(commit 'cut 1.4')
backport=$(commit 'backport a fix to 1.4')
git update-ref refs/remotes/origin/release/1.4-lts "$backport"
git update-ref refs/remotes/origin/release/10.20-lts "$backport"

git checkout --quiet --detach "$base"
side=$(commit 'unreviewed change')

git checkout --quiet --detach "$base"
next=$(commit 'cut 1.5')
git update-ref refs/remotes/origin/release/1.5-lts "$next"

failures=0

check() {
    if [ "$2" != "$3" ]; then
        printf 'FAIL %s\n expected: %s\n      got: %s\n' "$1" "$2" "$3" >&2
        failures=$((failures + 1))
    fi
}

contains() {
    case $3 in
        *"$2"*) ;;
        *)
            printf 'FAIL %s\n missing: %s\n in: %s\n' "$1" "$2" "$3" >&2
            failures=$((failures + 1))
            ;;
    esac
}

run() {
    status=0
    output=$(sh "$script" "$@" 2>"$directory/stderr") || status=$?
    errors=$(cat "$directory/stderr")
}

accepts() {
    git checkout --quiet --detach "$1"
    shift
    tag=$1
    expected=$2
    run "$tag"
    check "$tag exits zero" 0 "$status"
    check "$tag output" "$expected" "$output"
    check "$tag writes nothing to stderr" '' "$errors"
}

refuses() {
    git checkout --quiet --detach "$1"
    shift
    name=$1
    needle=$2
    shift 2
    run "$@"
    if [ "$status" -eq 0 ]; then
        printf 'FAIL %s exits non-zero\n' "$name" >&2
        failures=$((failures + 1))
    fi
    check "$name prints no outputs" '' "$output"
    contains "$name explains why" "$needle" "$errors"
}

lines() {
    printf 'version=%s\nline=%s\ncargo=%s\nbranch=%s' "$1" "$2" "$3" "$4"
}

accepts "$backport" v1.4.0-lts "$(lines 1.4.0-lts 1.4 1.4.0 release/1.4-lts)"
accepts "$base" v1.4.3-lts "$(lines 1.4.3-lts 1.4 1.4.3 release/1.4-lts)"
accepts "$backport" v10.20.30-lts "$(lines 10.20.30-lts 10.20 10.20.30 release/10.20-lts)"
accepts "$next" v1.5.0-lts "$(lines 1.5.0-lts 1.5 1.5.0 release/1.5-lts)"

for tag in \
    '' \
    v1.4-lts \
    v1.4.3 \
    1.4.3-lts \
    v01.4.3-lts \
    v1.04.3-lts \
    v1.4.03-lts \
    v1.4.3.1-lts \
    v1..3-lts \
    v.4.3-lts \
    v1.4.-lts \
    v1.4.3-LTS \
    v1.4.3-lts-rc1 \
    v1.4.3-rc1-lts \
    ' v1.4.3-lts' \
    'v1.4.3-lts ' \
    'v1.4.3
-lts' \
    'v1.4.3-lts
version=9.9.9-lts' \
    va.b.c-lts \
    refs/tags/v1.4.3-lts
do
    refuses "$backport" "malformed tag '$tag'" 'is not an LTS tag' "$tag"
done

refuses "$backport" 'no arguments' 'usage:'
refuses "$backport" 'two arguments' 'usage:' v1.4.3-lts origin/release/1.4-lts

refuses "$side" 'a tag on a commit the branch does not contain' 'is not on origin/release/1.4-lts' v1.4.4-lts
refuses "$next" 'a 1.4 tag on the 1.5 branch' 'is not on origin/release/1.4-lts' v1.4.4-lts
refuses "$backport" 'a 1.4 commit tagged for 1.5' 'is not on origin/release/1.5-lts' v1.5.1-lts
refuses "$backport" 'a line with no fetched branch' "fetch '+refs/heads/release/*-lts:refs/remotes/origin/release/*-lts' first" v1.6.0-lts

git branch --quiet release/1.6-lts "$backport"
refuses "$backport" 'a local branch instead of the fetched remote one' 'origin/release/1.6-lts is missing' v1.6.0-lts

if [ "$failures" -ne 0 ]; then
    printf '%s LTS version check(s) failed\n' "$failures" >&2
    exit 1
fi

printf '%s\n' 'LTS version checks passed'
