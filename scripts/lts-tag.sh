#!/bin/sh
set -eu

usage='usage: lts-tag.sh IMAGE X.Y.Z-lts'
label=org.opencontainers.image.version

fail() {
    printf 'lts-tag: %s\n' "$1" >&2
    exit 1
}

if [ $# -ne 2 ]; then
    printf '%s\n' "$usage" >&2
    exit 2
fi

image=$1
version=$2

errors=$(mktemp)
trap 'rm -f "$errors"' EXIT
trap 'exit 1' HUP INT TERM

parse() {
    core=${1%-lts}
    case $1 in
        *-lts) ;;
        *) return 1 ;;
    esac
    case $core in
        '' | *[!0-9.]* | *.*.*.* | .* | *. | *..*) return 1 ;;
        *.*.*) ;;
        *) return 1 ;;
    esac
    major=${core%%.*}
    rest=${core#*.}
    minor=${rest%%.*}
    patch=${rest#*.}
    for part in "$major" "$minor" "$patch"; do
        case $part in
            '' | 0?*) return 1 ;;
        esac
    done
}

inspect() {
    docker buildx imagetools inspect --format "$1" "$2" 2> "$errors"
}

digest_of() {
    manifest=$(inspect '{{json .Manifest}}' "$1") || fail "could not inspect $1: $(cat "$errors")"
    found=$(printf '%s' "$manifest" | jq -r '.digest // ""')
    case $found in
        sha256:*) printf '%s\n' "$found" ;;
        *) fail "$1 has no digest" ;;
    esac
}

version_of() {
    printf '%s' "$1" | jq -r --arg label "$label" '
        (if has("config") then [.] else [.[]] end)
        | map(.config.Labels[$label] // "")
        | unique
        | if length == 1 then .[0] else "" end'
}

notice() {
    printf '::notice::%s\n' "$1"
    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
        printf '%s\n' "$1" >> "$GITHUB_STEP_SUMMARY"
    fi
}

move() {
    docker buildx imagetools create --tag "$moving" "$source"
    moved=$(digest_of "$moving")
    [ "$moved" = "$digest" ] || fail "$moving points at $moved after the move, not $digest"
    printf '%s now points at %s (%s)\n' "$moving" "$version" "$digest"
}

parse "$version" || fail "'$version' is not X.Y.Z-lts with no leading zeros"
line=$major.$minor
requested_patch=$patch
source=$image:$version
moving=$image:$line-lts
digest=$(digest_of "$source")

if ! configs=$(inspect '{{json .Image}}' "$moving"); then
    case $(cat "$errors") in
        *': not found'*)
            printf '%s does not exist yet\n' "$moving"
            move
            exit 0
            ;;
    esac
    fail "could not inspect $moving: $(cat "$errors")"
fi

current=$(version_of "$configs")
[ -n "$current" ] || fail "$moving carries no single $label label, so it cannot be compared with $version"
parse "$current" || fail "$moving is labelled '$current', which is not X.Y.Z-lts"
[ "$major.$minor" = "$line" ] || fail "$moving is labelled $current, which is not on the $line line"

if [ "$requested_patch" -gt "$patch" ]; then
    printf '%s moves from %s to %s\n' "$moving" "$current" "$version"
    move
elif [ "$requested_patch" -eq "$patch" ]; then
    held=$(digest_of "$moving")
    [ "$held" = "$digest" ] || fail "$moving is labelled $version but points at $held, not $source ($digest)"
    printf '%s already points at %s\n' "$moving" "$version"
else
    notice "$moving stays at $current: $version is older, so it was published without moving the $line-lts tag"
fi
