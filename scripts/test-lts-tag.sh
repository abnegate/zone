#!/bin/sh
set -eu

script=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)/lts-tag.sh
shell='sh'
if [ -n "${BASH_VERSION:-}" ]; then
    shell=bash
fi
directory=$(mktemp -d)
trap 'rm -rf "$directory"' EXIT HUP INT TERM

registry=$directory/registry
mkdir -p "$directory/bin" "$registry/tags" "$registry/labels"

cat > "$directory/bin/docker" <<'DOCKER'
#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$FAKE_REGISTRY/calls"
key() {
    printf '%s' "$1" | tr '/:@' '___'
}
if [ "$1 $2" != 'buildx imagetools' ]; then
    printf 'unexpected docker %s\n' "$*" >&2
    exit 64
fi
shift 2
case $1 in
    inspect)
        [ "$2" = --format ] || exit 64
        tag=$FAKE_REGISTRY/tags/$(key "$4")
        if [ "$4" = "${FAKE_UNREACHABLE:-}" ]; then
            printf 'ERROR: failed to do request: Head "https://%s": connection refused\n' "$4" >&2
            exit 1
        fi
        if [ ! -f "$tag" ]; then
            printf 'ERROR: %s: not found\n' "$4" >&2
            exit 1
        fi
        digest=$(cat "$tag")
        case $3 in
            '{{json .Manifest}}')
                printf '{"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","digest":"%s","size":743}\n' "$digest"
                ;;
            '{{json .Image}}')
                label=$(cat "$FAKE_REGISTRY/labels/${digest#sha256:}")
                printf '{"linux/amd64":{"config":{"Labels":{%s}}},"linux/arm64":{"config":{"Labels":{%s}}}}\n' "$label" "$label"
                ;;
            *) exit 64 ;;
        esac
        ;;
    create)
        [ "$2" = --tag ] || exit 64
        cp "$FAKE_REGISTRY/tags/$(key "$4")" "$FAKE_REGISTRY/tags/$(key "$3")"
        ;;
    *) exit 64 ;;
esac
DOCKER
chmod +x "$directory/bin/docker"

PATH=$directory/bin:$PATH
FAKE_REGISTRY=$registry
GITHUB_STEP_SUMMARY=$directory/summary
export PATH FAKE_REGISTRY GITHUB_STEP_SUMMARY

image=registry.example/zone/server
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

key() {
    printf '%s' "$1" | tr '/:@' '___'
}

publish() {
    digest=sha256:build-$1
    printf '%s\n' "$digest" > "$registry/tags/$(key "$image:$1")"
    printf '"org.opencontainers.image.version":"%s"\n' "$1" > "$registry/labels/${digest#sha256:}"
}

digest() {
    cat "$registry/tags/$(key "$image:$1")"
}

pointing() {
    if [ -f "$registry/tags/$(key "$image:$1")" ]; then
        digest "$1"
    else
        printf 'missing'
    fi
}

run() {
    : > "$registry/calls"
    : > "$GITHUB_STEP_SUMMARY"
    status=0
    output=$("$shell" "$script" "$@" 2>&1) || status=$?
}

created() {
    grep -c '^buildx imagetools create' "$registry/calls" || true
}

publish 1.4.3-lts
run "$image" 1.4.3-lts
check 'a missing line tag exits zero' 0 "$status"
check 'a missing line tag is created at the release' "$(digest 1.4.3-lts)" "$(pointing 1.4-lts)"
check 'a missing line tag is created once' 1 "$(created)"
contains 'a missing line tag is reported' '1.4-lts does not exist yet' "$output"

publish 1.4.4-lts
run "$image" 1.4.4-lts
check 'a newer release exits zero' 0 "$status"
check 'a newer release moves the line tag' "$(digest 1.4.4-lts)" "$(pointing 1.4-lts)"
contains 'a newer release names both versions' 'moves from 1.4.3-lts to 1.4.4-lts' "$output"

run "$image" 1.4.4-lts
check 'an equal release exits zero' 0 "$status"
check 'an equal release keeps the line tag' "$(digest 1.4.4-lts)" "$(pointing 1.4-lts)"
check 'an equal release writes nothing' 0 "$(created)"
contains 'an equal release says so' 'already points at 1.4.4-lts' "$output"

run "$image" 1.4.3-lts
check 'an older release exits zero' 0 "$status"
check 'an older release leaves the line tag on the newer release' "$(digest 1.4.4-lts)" "$(pointing 1.4-lts)"
check 'an older release writes nothing' 0 "$(created)"
contains 'an older release logs a notice' '::notice::' "$output"
contains 'an older release names the version it kept' 'stays at 1.4.4-lts: 1.4.3-lts is older' "$output"
contains 'an older release writes the job summary' 'stays at 1.4.4-lts: 1.4.3-lts is older' "$(cat "$GITHUB_STEP_SUMMARY")"

publish 1.4.10-lts
run "$image" 1.4.10-lts
check 'a two-digit patch is newer than a one-digit patch' "$(digest 1.4.10-lts)" "$(pointing 1.4-lts)"
publish 1.4.9-lts
run "$image" 1.4.9-lts
check 'patches compare as numbers, not text' "$(digest 1.4.10-lts)" "$(pointing 1.4-lts)"

publish 1.5.0-lts
run "$image" 1.5.0-lts
check 'another line gets its own tag' "$(digest 1.5.0-lts)" "$(pointing 1.5-lts)"
check 'another line leaves this line alone' "$(digest 1.4.10-lts)" "$(pointing 1.4-lts)"

printf '"org.opencontainers.image.version":"1.4.4-lts"\n' > "$registry/labels/build-1.4.10-lts"
run "$image" 1.4.4-lts
check 'a line tag labelled as this release but holding another build fails' 1 "$status"
contains 'the mismatch names the release' 'is labelled 1.4.4-lts but points at' "$output"
printf '"org.opencontainers.image.version":"1.4.10-lts"\n' > "$registry/labels/build-1.4.10-lts"

publish 1.4.11-lts
: > "$registry/labels/build-1.4.10-lts"
run "$image" 1.4.11-lts
check 'an unlabelled line tag fails' 1 "$status"
contains 'an unlabelled line tag is named' 'carries no single org.opencontainers.image.version label' "$output"
printf '"org.opencontainers.image.version":"1.4.10-lts"\n' > "$registry/labels/build-1.4.10-lts"

FAKE_UNREACHABLE=$image:1.4-lts
export FAKE_UNREACHABLE
run "$image" 1.4.11-lts
unset FAKE_UNREACHABLE
check 'a registry error fails' 1 "$status"
check 'a registry error moves nothing' "$(digest 1.4.10-lts)" "$(pointing 1.4-lts)"
contains 'a registry error is shown' 'connection refused' "$output"

run "$image" 1.4.12-lts
check 'a release that is not published fails' 1 "$status"
check 'a release that is not published moves nothing' "$(digest 1.4.10-lts)" "$(pointing 1.4-lts)"

for malformed in 1.4.4 v1.4.4-lts 1.04.4-lts 1.4-lts 1.4.4.4-lts; do
    run "$image" "$malformed"
    check "$malformed is refused" 1 "$status"
    contains "$malformed is named" "'$malformed' is not X.Y.Z-lts" "$output"
done

run "$image"
check 'a missing argument prints usage' 2 "$status"
contains 'a missing argument prints usage text' 'usage: lts-tag.sh IMAGE X.Y.Z-lts' "$output"

if [ "$failures" -ne 0 ]; then
    printf '%s lts-tag check(s) failed\n' "$failures" >&2
    exit 1
fi
printf 'lts-tag: all checks passed\n'
