#!/bin/sh
set -eu

usage='usage: bundle-upload.sh BUCKET X.Y.Z-lts ARCHIVE'

fail() {
    printf '::error::%s\n' "$1"
    exit 1
}

if [ $# -ne 3 ]; then
    printf '%s\n' "$usage" >&2
    exit 2
fi

bucket=$1
version=$2
archive=$3

for file in "$archive" "$archive.sha256" "$archive.sha256.sigstore.json"; do
    [ -f "$file" ] || fail "$file is missing"
done

name=$(basename -- "$archive")
prefix=bundles/$version
tar=$prefix/$name
checksum=$tar.sha256
marker=$checksum.sigstore.json
hash=$(cut -d ' ' -f 1 "$archive.sha256")

absent() {
    if probe=$(aws s3api head-object --bucket "$bucket" --key "$1" 2>&1); then
        return 1
    fi
    case $probe in
        *'(404)'* | *'Not Found'*) return 0 ;;
    esac
    printf '%s\n' "$probe" >&2
    fail "Could not check whether s3://$bucket/$1 exists"
}

put() {
    if ! aws s3api put-object \
        --bucket "$bucket" \
        --key "$1" \
        --body "$2" \
        --content-type "$3" \
        --if-none-match '*' \
        --server-side-encryption AES256 > /dev/null; then
        fail "Could not create s3://$bucket/$1; it may already exist"
    fi
}

if ! absent "$marker"; then
    fail "Bundle $name is already published: s3://$bucket/$marker exists"
fi
for leftover in "$checksum" "$tar"; do
    if ! absent "$leftover"; then
        fail "s3://$bucket/$leftover is left from an upload that never committed; nothing was published, so delete it and rerun"
    fi
done

aws s3 cp --only-show-errors --no-overwrite --sse AES256 \
    --metadata "sha256=$hash" "$archive" "s3://$bucket/$tar" \
    || fail "Could not upload s3://$bucket/$tar"
uploaded=$(aws s3api head-object --bucket "$bucket" --key "$tar" --query Metadata.sha256 --output text) \
    || fail "Could not read back s3://$bucket/$tar"
if [ "$uploaded" != "$hash" ]; then
    fail "s3://$bucket/$tar holds sha256 '$uploaded', not $hash; another upload created it first"
fi
put "$checksum" "$archive.sha256" text/plain
put "$marker" "$archive.sha256.sigstore.json" application/json
printf 'Published s3://%s/%s\n' "$bucket" "$tar"
