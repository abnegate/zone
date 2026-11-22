#!/bin/sh
set -eu

unset ZONE_AIRGAP_REGISTRY ZONE_LICENSE_RENEWAL ZONE_TRUSTED_PROXIES ZONE_LTS_VERSION ZONE_LTS_REGISTRY COMPOSE_FILE COMPOSE_PROFILES

shell='sh'
if [ -n "${BASH_VERSION:-}" ]; then
    shell=bash
fi

directory=$(mktemp -d)
trap 'rm -rf "$directory"' EXIT HUP INT TERM
scripts=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
root=$(dirname -- "$scripts")
version=1.4.3-lts
architecture=amd64
top=zone-bundle-$version-$architecture
mirror=mirror.example.internal:5443/zone
tab=$(printf '\t')
repository=$directory/repository
output=$directory/output
extract=$directory/extract
bundle=$extract/$top
installation=$directory/zone

fail() {
    printf 'FAIL: %s\n' "$1" >&2
    for log in run.log crane.log docker.log compose.log; do
        if [ -s "$directory/$log" ]; then
            sed "s/^/  $log: /" "$directory/$log" >&2
        fi
    done
    exit 1
}

checksum() {
    if command -v sha256sum > /dev/null 2>&1; then
        sha256sum "$@"
    else
        shasum -a 256 "$@"
    fi
}

mkdir -p "$directory/bin" "$directory/crane"

cat > "$directory/crane/crane" <<'CRANE'
#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$CRANE_LOG"
command=$1
shift
platform=''
format=''
while [ $# -gt 0 ]; do
    case $1 in
        --platform) platform=$2; shift 2 ;;
        --format=*) format=${1#--format=}; shift ;;
        --insecure) shift ;;
        *) break ;;
    esac
done
hash() {
    if command -v sha256sum > /dev/null 2>&1; then
        sha256sum | cut -c 1-64
    else
        shasum -a 256 | cut -c 1-64
    fi
}
case $command in
    digest)
        printf 'sha256:%s\n' "$(printf '%s %s' "$1" "$platform" | hash)"
        ;;
    pull)
        case $format in
            oci)
                digest=${1##*@}
                mkdir -p "$2/blobs/sha256"
                printf '{"imageLayoutVersion":"1.0.0"}\n' > "$2/oci-layout"
                printf '{"schemaVersion":2,"manifests":[{"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"%s","size":2}]}\n' \
                    "$digest" > "$2/index.json"
                printf '%s %s\n' "$1" "$platform" > "$2/blobs/sha256/${digest#sha256:}"
                ;;
            tarball)
                printf 'registry helper %s %s\n' "$1" "$platform" > "$2"
                ;;
            *) exit 1 ;;
        esac
        ;;
    push)
        digest=$(sed -n 's/.*"digest":"\(sha256:[0-9a-f]*\)".*/\1/p' "$1/index.json")
        target=${2%@*}
        case ${target##*/} in
            *:*) target=${target%:*} ;;
        esac
        printf '%s@%s\n' "$target" "$digest"
        ;;
    catalog) ;;
    *) exit 1 ;;
esac
CRANE
printf '%s\n' 'Apache License, Version 2.0' > "$directory/crane/LICENSE"

cat > "$directory/bin/helm" <<'HELM'
#!/bin/sh
set -eu
if [ "$1" != package ] || [ "$3" != --destination ]; then
    exit 1
fi
name=$(sed -n 's/^name: *//p' "$2/Chart.yaml")
version=$(sed -n 's/^version: *//p' "$2/Chart.yaml")
mkdir -p "$4"
printf 'chart %s %s\n' "$name" "$version" > "$4/$name-$version.tgz"
HELM

cat > "$directory/bin/git" <<'GIT'
#!/bin/sh
set -eu
if [ "$1" != -C ] || [ "$3" != archive ] || [ "$4" != --format=tar ] || [ "$5" != -o ] || [ "$7" != HEAD ]; then
    exit 1
fi
tar -cf "$6" -C "$2" docker-compose.yml docker-compose.lts.yml scripts/env-file.sh
GIT

cat > "$directory/bin/docker" <<'DOCKER'
#!/bin/sh
printf '%s\n' "$*" >> "$DOCKER_LOG"
case $1 in
    compose)
        case " $* " in
            *' version --short '*) printf '%s\n' 2.24.0 ;;
            *' config --profiles '*) printf '%s\n' bundled-ollama monitoring vpn ;;
            *' pull '*) printf 'pull-environment ZONE_AIRGAP_REGISTRY=%s\n' "${ZONE_AIRGAP_REGISTRY:-}" >> "$DOCKER_LOG" ;;
            *) exit 1 ;;
        esac
        ;;
    load) printf '%s\n' 'Loaded image: registry:i-was-a-digest' ;;
    volume)
        if [ "$2" = create ]; then
            printf '%s\n' "$3"
        fi
        ;;
    run) printf '%s\n' 0123456789ab ;;
    rm) ;;
    *) exit 1 ;;
esac
DOCKER

chmod +x "$directory/crane/crane" "$directory/bin/helm" "$directory/bin/git" "$directory/bin/docker"

mkdir -p "$repository/scripts"
for file in bundle.sh bundle-images.txt install-offline.sh env-file.sh; do
    cp "$scripts/$file" "$repository/scripts/$file"
done
for file in docker-compose.yml docker-compose.lts.yml; do
    cp "$root/$file" "$repository/$file"
done
for chart in "$root"/helm/*/Chart.yaml; do
    name=$(basename -- "$(dirname -- "$chart")")
    mkdir -p "$repository/helm/$name"
    cp "$chart" "$repository/helm/$name/Chart.yaml"
done

run() {
    : > "$directory/crane.log"
    : > "$directory/docker.log"
    status=0
    env PATH="$directory/bin:$PATH" CRANE_LOG="$directory/crane.log" DOCKER_LOG="$directory/docker.log" \
        "$shell" "$@" > "$directory/run.log" 2>&1 || status=$?
    return "$status"
}

install() {
    run "$bundle/install.sh" "$@"
}

assert_crane() {
    grep -qxF -- "$1" "$directory/crane.log" || fail "Expected the crane call: $1"
}

assert_docker() {
    grep -qxF -- "$1" "$directory/docker.log" || fail "Expected the docker call: $1"
}

assert_pushed_everything() {
    prefix=$1
    while IFS="$tab" read -r name target tag digest layout _; do
        assert_crane "push $2$bundle/$layout $prefix/$target@$digest"
        assert_crane "push $2$bundle/$layout $prefix/$target:$tag"
        pushed=$((pushed + 2))
    done < "$bundle/images.txt"
    [ "$(grep -c '^push ' "$directory/crane.log")" -eq "$pushed" ] \
        || fail "Expected each image pushed exactly once by digest and once by tag into $prefix"
}

assert_trusted_proxies() {
    label=$1
    image=$2
    shift 2
    rendered=$directory/rendered.json
    (cd -- "$installation" && docker compose --env-file .env "$@" config --format json) \
        > "$rendered" 2> "$directory/compose.log" || fail "Expected docker compose config to render $label"
    python3 - "$rendered" "$label" "$image" <<'PY' || fail "Expected $label to keep Traefik pinned and trusted"
import json
import sys

rendered, label, image = sys.argv[1:]
services = json.load(open(rendered, encoding="utf-8"))["services"]
manager = services["manager"]
if not manager.get("image", "").startswith(image):
    raise SystemExit(f"{label}: manager must run from {image}, got {manager.get('image')!r}")
trusted = manager.get("environment", {}).get("ZONE_TRUSTED_PROXIES")
if trusted != "172.31.0.250":
    raise SystemExit(f"{label}: manager must trust 172.31.0.250, got {trusted!r}")
address = (services["traefik"].get("networks", {}).get("edge") or {}).get("ipv4_address")
if address != "172.31.0.250":
    raise SystemExit(f"{label}: traefik must stay pinned to 172.31.0.250 on edge, got {address!r}")
PY
}

extract_bundle() {
    rm -rf "$extract"
    mkdir -p "$extract"
    tar -xf "$output/$top.tar" -C "$extract"
}

manifest_entries() {
    grep -v '^#' "$scripts/bundle-images.txt" | awk 'NF { print $1 "\t" $4 }'
}

expected_values() {
    awk -F '\t' -v registry="$mirror" '
        {
            count = split($6, items, ",")
            for (item = 1; item <= count; item++) {
                split(items[item], fields, ":")
                if (fields[1] == "helm-split") {
                    print fields[2] "\t" fields[3] ".repository\t" registry "/" $2
                    print fields[2] "\t" fields[3] ".tag\t" $3 "@" $4
                } else if (fields[1] == "helm-string") {
                    print fields[2] "\t" fields[3] "\t" registry "/" $2 ":" $3 "@" $4
                }
            }
        }
    ' "$bundle/images.txt" | LC_ALL=C sort
}

written_values() {
    for file in "$directory"/values/*.yaml; do
        awk -v chart="$(basename -- "$file" .yaml)" '
            {
                match($0, /^ */)
                depth = RLENGTH / 2
                line = substr($0, RLENGTH + 1)
                key = line
                sub(/:.*/, "", key)
                rest = substr(line, length(key) + 2)
                path[depth] = key
                full = path[0]
                for (level = 1; level <= depth; level++) full = full "." path[level]
                if (rest != "") {
                    sub(/^ /, "", rest)
                    gsub(/^"|"$/, "", rest)
                    print chart "\t" full "\t" rest
                }
            }
        ' "$file"
    done | LC_ALL=C sort
}

run "$repository/scripts/bundle.sh" --version "$version" --architecture "$architecture" \
    --registry 127.0.0.1:5000 --crane "$directory/crane/crane" --output "$output" \
    || fail 'Expected the real bundle.sh to assemble a bundle from the real bundle-images.txt'
[ -f "$output/$top.tar" ] || fail "Expected $top.tar"
(cd "$output" && checksum -c "$top.tar.sha256" > /dev/null) || fail "Expected $top.tar.sha256 to match the tar"

extract_bundle
cmp -s "$scripts/install-offline.sh" "$bundle/install.sh" || fail 'Expected install.sh to be scripts/install-offline.sh'
tar -tf "$bundle/source.tar" | grep -qx 'scripts/env-file.sh' || fail 'Expected source.tar to carry scripts/env-file.sh'

cut -f 1,6 "$bundle/images.txt" > "$directory/bundled"
manifest_entries > "$directory/listed"
cmp -s "$directory/listed" "$directory/bundled" \
    || fail 'Expected images.txt to list every bundle-images.txt entry, in order, with its consumers'

install verify || fail 'Expected the extracted bundle to verify'
grep -qF 'files against SHA256SUMS' "$directory/run.log" || fail 'Expected verify to report the files it checked'

pushed=0
install push --registry "$mirror" || fail "Expected push into $mirror to pass"
[ ! -s "$directory/docker.log" ] || fail 'Expected push not to call docker'
assert_pushed_everything "$mirror" ''

install helm-values --registry "$mirror" --output "$directory/values" || fail 'Expected helm-values to pass'
for chart in zone-apps zone-infra zone-ai; do
    [ -f "$directory/values/$chart.yaml" ] || fail "Expected helm-values to write $chart.yaml"
done
[ "$(find "$directory/values" -type f | wc -l | tr -d ' ')" -eq 3 ] || fail 'Expected exactly three values files'
expected_values > "$directory/values.expected"
written_values > "$directory/values.written"
[ -s "$directory/values.expected" ] || fail 'Expected bundle-images.txt to name helm consumers'
if ! cmp -s "$directory/values.expected" "$directory/values.written"; then
    diff -u "$directory/values.expected" "$directory/values.written" >&2 || true
    fail 'Expected the helm values to set exactly every helm-split and helm-string consumer'
fi

mkdir -p "$installation"
printf '%s\n' 'KEEP=me' > "$installation/.env"
pushed=0
install compose --directory "$installation" || fail 'Expected compose to load the bundle'
assert_docker "load --input $bundle/images/registry.tar"
grep -q "^run .* registry:i-was-a-digest\$" "$directory/docker.log" \
    || fail 'Expected compose to run the registry helper under the name docker load reported'
assert_pushed_everything 127.0.0.1:5000 '--insecure '
services=$(cut -f 6 "$bundle/images.txt" | tr ',' '\n' | sed -n 's/^compose://p' | tr '\n' ' ')
assert_docker "compose -f docker-compose.yml -f docker-compose.airgap.yml --profile bundled-ollama --profile monitoring --profile vpn pull ${services% }"
assert_docker 'pull-environment ZONE_AIRGAP_REGISTRY=127.0.0.1:5000'
cmp -s "$bundle/compose/docker-compose.airgap.yml" "$installation/docker-compose.airgap.yml" \
    || fail 'Expected compose to copy docker-compose.airgap.yml into the installation'
for line in KEEP=me ZONE_AIRGAP_REGISTRY=127.0.0.1:5000 ZONE_LICENSE_RENEWAL=false; do
    grep -qxF -- "$line" "$installation/.env" || fail "Expected $line in .env"
done
if grep -q 'ZONE_TRUSTED_PROXIES' "$installation/.env"; then
    fail 'Expected compose to leave ZONE_TRUSTED_PROXIES to the Compose default'
fi
assert_trusted_proxies 'the air-gapped override' 127.0.0.1:5000/zone/server@sha256: \
    -f docker-compose.yml -f docker-compose.airgap.yml
printf '%s\n' 'ZONE_LTS_VERSION=1.4-lts' >> "$installation/.env"
assert_trusted_proxies 'the air-gapped LTS overlays' 127.0.0.1:5000/zone/server@sha256: \
    -f docker-compose.yml -f docker-compose.lts.yml -f docker-compose.airgap.yml
helper=$(sed -n 's/^volume create \(zone-airgap-registry-[0-9]*\)$/\1/p' "$directory/docker.log")
[ -n "$helper" ] || fail 'Expected a temporary registry volume'
assert_docker "rm --force --volumes $helper"
assert_docker "volume rm --force $helper"

layout=$(head -n 1 "$bundle/images.txt" | cut -f 5)
blob=$(find "$bundle/$layout/blobs" -type f | head -n 1)
printf '%s\n' tampered >> "$blob"
if install verify; then
    fail 'Expected a tampered image layer to fail verify'
fi
grep -qF "${blob#"$bundle"/}: FAILED" "$directory/run.log" || fail 'Expected verify to name the tampered layer'

extract_bundle
printf '%s\n' '  extra: {}' >> "$bundle/compose/docker-compose.airgap.yml"
if install verify; then
    fail 'Expected a tampered Compose override to fail verify'
fi
grep -qF 'compose/docker-compose.airgap.yml: FAILED' "$directory/run.log" || fail 'Expected verify to name the tampered override'

printf 'Bundle round trip passed under %s\n' "$shell"
