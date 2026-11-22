#!/bin/sh
set -eu

script=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)/bundle-upload.sh
shell='sh'
if [ -n "${BASH_VERSION:-}" ]; then
    shell=bash
fi
directory=$(mktemp -d)
trap 'rm -rf "$directory"' EXIT HUP INT TERM

bucket=$directory/bucket
mkdir -p "$directory/bin" "$bucket" "$bucket.metadata"

cat > "$directory/bin/aws" <<'AWS'
#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$FAKE_BUCKET.calls"
object() {
    printf '%s/%s' "$FAKE_BUCKET" "$(printf '%s' "$1" | tr '/' '_')"
}
metadata() {
    printf '%s.metadata/%s' "$FAKE_BUCKET" "$(printf '%s' "$1" | tr '/' '_')"
}
denied() {
    printf 'An error occurred (AccessDenied) when calling the %s operation: explicit deny in a resource-based policy\n' "$1" >&2
    exit 254
}
store() {
    cp "$2" "$(object "$1")"
    printf '%s' "$3" > "$(metadata "$1")"
}
service=$1
operation=$2
shift 2
key='' query='' body='' conditional='' metadata='' source='' destination=''
while [ $# -gt 0 ]; do
    case $1 in
        --bucket | --content-type | --server-side-encryption | --sse | --output) shift 2 ;;
        --key) key=$2; shift 2 ;;
        --query) query=$2; shift 2 ;;
        --body) body=$2; shift 2 ;;
        --if-none-match) conditional=$2; shift 2 ;;
        --metadata) metadata=${2#sha256=}; shift 2 ;;
        --no-overwrite) conditional='*'; shift ;;
        --only-show-errors) shift ;;
        s3://*) destination=${1#s3://*/}; shift ;;
        *) source=$1; shift ;;
    esac
done
case "$service $operation" in
    's3api head-object')
        if [ -n "${FAKE_HEAD_ERROR:-}" ]; then
            printf 'An error occurred (403) when calling the HeadObject operation: %s\n' "$FAKE_HEAD_ERROR" >&2
            exit 254
        fi
        if [ ! -f "$(object "$key")" ]; then
            printf 'An error occurred (404) when calling the HeadObject operation: Not Found\n' >&2
            exit 254
        fi
        if [ "$query" = Metadata.sha256 ]; then
            if [ -s "$(metadata "$key")" ]; then
                cat "$(metadata "$key")"
                printf '\n'
            else
                printf 'None\n'
            fi
        else
            printf '{"ContentLength": 1}\n'
        fi
        ;;
    's3api put-object')
        [ "$conditional" = '*' ] || denied PutObject
        if [ -f "$(object "$key")" ]; then
            printf 'An error occurred (PreconditionFailed) when calling the PutObject operation: At least one of the pre-conditions you specified did not hold\n' >&2
            exit 254
        fi
        store "$key" "$body" ''
        printf '{"ETag": "\\"fake\\""}\n'
        ;;
    's3 cp')
        [ "$conditional" = '*' ] || denied CompleteMultipartUpload
        if [ -n "${FAKE_RACE:-}" ]; then
            printf 'another build\n' > "$FAKE_BUCKET.race"
            store "$destination" "$FAKE_BUCKET.race" "$FAKE_RACE"
        fi
        if [ -f "$(object "$destination")" ]; then
            exit 0
        fi
        store "$destination" "$source" "$metadata"
        ;;
    *)
        printf 'unexpected aws %s %s\n' "$service" "$operation" >&2
        exit 64
        ;;
esac
AWS
chmod +x "$directory/bin/aws"

PATH=$directory/bin:$PATH
FAKE_BUCKET=$bucket
export PATH FAKE_BUCKET

version=1.4.3-lts
name=zone-bundle-$version-amd64.tar
archive=$directory/$name
printf 'bundle\n' > "$archive"
hash=$(cksum < "$archive" | cut -d ' ' -f 1)
printf '%s  %s\n' "$hash" "$name" > "$archive.sha256"
printf '{"mediaType":"application/vnd.dev.sigstore.bundle.v0.3+json"}\n' > "$archive.sha256.sigstore.json"

tar_key=bundles_${version}_$name
checksum_key=$tar_key.sha256
marker_key=$checksum_key.sigstore.json

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

present() {
    if [ -f "$bucket/$1" ]; then
        printf 'present'
    else
        printf 'absent'
    fi
}

empty() {
    rm -f "$bucket"/* "$bucket.metadata"/*
    : > "$bucket.calls"
}

run() {
    : > "$bucket.calls"
    status=0
    output=$("$shell" "$script" "$@" 2>&1) || status=$?
}

writes() {
    grep -c -e '^s3api put-object' -e '^s3 cp' "$bucket.calls" || true
}

empty
run bucket "$version" "$archive"
check 'a fresh bundle uploads' 0 "$status"
contains 'a fresh bundle names the tar it published' "Published s3://bucket/bundles/$version/$name" "$output"
check 'the tar is the archive' "$(cat "$archive")" "$(cat "$bucket/$tar_key")"
check 'the tar carries its hash' "$hash" "$(cat "$bucket.metadata/$tar_key")"
check 'the checksum is the checksum file' "$(cat "$archive.sha256")" "$(cat "$bucket/$checksum_key")"
check 'the marker is the signature' "$(cat "$archive.sha256.sigstore.json")" "$(cat "$bucket/$marker_key")"
contains 'the tar upload refuses to overwrite' "s3 cp --only-show-errors --no-overwrite" "$(cat "$bucket.calls")"
check 'every put-object is conditional' "$(grep -c '^s3api put-object' "$bucket.calls")" "$(grep -c "^s3api put-object.*--if-none-match \*" "$bucket.calls")"

run bucket "$version" "$archive"
check 'a published bundle is refused' 1 "$status"
contains 'a published bundle is named' "is already published: s3://bucket/bundles/$version/$name.sha256.sigstore.json exists" "$output"
check 'a published bundle is not written again' 0 "$(writes)"

empty
cp "$archive.sha256" "$bucket/$checksum_key"
run bucket "$version" "$archive"
check 'a leftover checksum is refused' 1 "$status"
contains 'a leftover checksum is named' "s3://bucket/bundles/$version/$name.sha256 is left from an upload that never committed" "$output"
check 'a leftover checksum writes nothing' 0 "$(writes)"

empty
printf 'an aborted build\n' > "$bucket/$tar_key"
run bucket "$version" "$archive"
check 'a leftover tar is refused' 1 "$status"
contains 'a leftover tar is named' "s3://bucket/bundles/$version/$name is left from an upload that never committed" "$output"
check 'a leftover tar is kept' 'an aborted build' "$(cat "$bucket/$tar_key")"
check 'a leftover tar writes nothing' 0 "$(writes)"
check 'a leftover tar gets no checksum' absent "$(present "$checksum_key")"

empty
FAKE_RACE=0000000000
export FAKE_RACE
run bucket "$version" "$archive"
unset FAKE_RACE
check 'a tar created by another upload is refused' 1 "$status"
contains 'a tar created by another upload is named' "holds sha256 '0000000000', not $hash" "$output"
check 'a tar created by another upload is kept' 'another build' "$(cat "$bucket/$tar_key")"
check 'a tar created by another upload gets no checksum' absent "$(present "$checksum_key")"
check 'a tar created by another upload gets no marker' absent "$(present "$marker_key")"

empty
FAKE_HEAD_ERROR=Forbidden
export FAKE_HEAD_ERROR
run bucket "$version" "$archive"
unset FAKE_HEAD_ERROR
check 'a failed existence check is refused' 1 "$status"
contains 'a failed existence check is named' "Could not check whether s3://bucket/bundles/$version/$name.sha256.sigstore.json exists" "$output"
check 'a failed existence check writes nothing' 0 "$(writes)"

empty
run bucket "$version" "$directory/missing.tar"
check 'a missing archive is refused' 1 "$status"
contains 'a missing archive is named' "$directory/missing.tar is missing" "$output"

run bucket "$version"
check 'a missing argument prints usage' 2 "$status"
contains 'a missing argument prints usage text' 'usage: bundle-upload.sh BUCKET X.Y.Z-lts ARCHIVE' "$output"

if [ "$failures" -ne 0 ]; then
    printf '%s bundle-upload check(s) failed\n' "$failures" >&2
    exit 1
fi
printf 'bundle-upload: all checks passed\n'
