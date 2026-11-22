#!/bin/sh
set -eu

usage='usage: lts-version.sh vX.Y.Z-lts'

fail() {
    printf 'lts-version: %s\n' "$1" >&2
    exit 1
}

if [ $# -ne 1 ]; then
    printf '%s\n' "$usage" >&2
    exit 2
fi

tag=$1

malformed() {
    fail "'$tag' is not an LTS tag: expected vX.Y.Z-lts with no leading zeros"
}

case $tag in
    v*-lts) ;;
    *) malformed ;;
esac

core=${tag#v}
core=${core%-lts}

case $core in
    '' | *[!0-9.]* | *.*.*.* | .* | *. | *..*) malformed ;;
    *.*.*) ;;
    *) malformed ;;
esac

major=${core%%.*}
rest=${core#*.}
minor=${rest%%.*}
patch=${rest#*.}

for part in "$major" "$minor" "$patch"; do
    case $part in
        '' | 0?*) malformed ;;
    esac
done

line=$major.$minor
branch=release/$line-lts
reference=refs/remotes/origin/$branch

if ! git rev-parse --verify --quiet "$reference^{commit}" >/dev/null; then
    fail "origin/$branch is missing: fetch '+refs/heads/release/*-lts:refs/remotes/origin/release/*-lts' first, and cut $branch before tagging $tag"
fi

if ! git merge-base --is-ancestor HEAD "$reference"; then
    fail "HEAD ($(git rev-parse --short HEAD)) is not on origin/$branch, so $tag cannot be released from it"
fi

printf 'version=%s\n' "$core-lts"
printf 'line=%s\n' "$line"
printf 'cargo=%s\n' "$core"
printf 'branch=%s\n' "$branch"
