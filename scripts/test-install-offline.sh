#!/bin/sh
set -eu

unset ZONE_AIRGAP_REGISTRY ZONE_LICENSE_RENEWAL COMPOSE_FILE COMPOSE_PROFILES COMPOSE_VERSION \
    COMPOSE_PULL_STATUS CRANE_PUSH_STATUS CRANE_DIGEST DOCKER_RUN_STATUS DOCKER_LOADED

shell='sh'
if [ -n "${BASH_VERSION:-}" ]; then
    shell=bash
fi

sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT HUP INT TERM
scripts=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
bundle=$sandbox/zone-bundle-1.4.3-lts-amd64
work=$sandbox/work
tab=$(printf '\t')

server_digest=sha256:4c87d70e9a2424b49f323069c0967a063645e337d2df1b722a7da3912841cf89
manager_digest=sha256:7b4f4327c2c1b5b4de4b23faa96171d6c792b5bd6cbde1127388c2a3552ec24e
litellm_digest=sha256:9f535807e02583a2a5220d8a4d58ca3ffc3d6c047c7db803541f8b165438091f
traefik_digest=sha256:9c2a54d87f76f5c2f5f2682c68394af92fb12c0a2686798d6462a3f84bd78eaf
valkey_digest=sha256:b21fd94099dcd4bc6b2b9230daef69b6558b887ad4a2a1afe56ff6e745a88cdb
ollama_digest=sha256:020e4134285e2ef4d8fd801234176de3b4faadc992a3eb06c8e66a2f9d4c4ba2
cloudnative_digest=sha256:34cd4159d07b3410a1b29da35072e2e927a0905e717a61afa3814624a2e8859a
curl_digest=sha256:58adaa4e8dca9c988bae2aba4ab3434a0bb2da16bbe3f92dec39ec7785166777
upstream_litellm_digest=sha256:a53a7d3ffebede1925bd3ee8a21e4a7b9b63e2e68ec883af136edcccb6eeb82c
postgres_digest=sha256:fe0737ba566a2c5b2a28f34433c0a423261900ec17b9bf7ad115e1aae7e57f1b

fail() {
    printf 'FAIL: %s\n' "$1" >&2
    for log in output docker.log crane.log; do
        if [ -s "$sandbox/$log" ]; then
            sed "s/^/  $log: /" "$sandbox/$log" >&2
        fi
    done
    exit 1
}

checksum() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$@"
    else
        shasum -a 256 "$@"
    fi
}

image_line() {
    printf '%s\t%s\t%s\t%s\timages/%s\t%s\n' "$1" "$2" "$3" "$4" "$1" "$5"
}

write_images() {
    {
        image_line zone-server zone/server 1.4.3-lts "$server_digest" \
            compose:manager,helm-split:zone-apps:server.image,helm-split:zone-apps:migration.image
        image_line zone-manager zone/manager 1.4.3-lts "$manager_digest" \
            compose:console,helm-split:zone-apps:manager.image
        image_line zone-litellm zone/litellm 1.4.3-lts "$litellm_digest" compose:litellm
        image_line traefik library/traefik v3.7.12 "$traefik_digest" compose:traefik
        image_line valkey valkey/valkey 8.1.10-alpine "$valkey_digest" \
            compose:valkey,helm-split:zone-infra:valkey.image
        image_line ollama ollama/ollama 0.33.2 "$ollama_digest" \
            compose:ollama,compose:ollama-init,helm-split:zone-ai:ollama.image
        image_line cloudnative-postgresql cloudnative-pg/postgresql 16.15 "$cloudnative_digest" \
            helm-string:zone-infra:postgres.image
        image_line curl curlimages/curl 8.22.0 "$curl_digest" helm-split:zone-ai:ollama.init.image
        image_line litellm berriai/litellm v1.99.1 "$upstream_litellm_digest" helm-split:zone-ai:litellm.image
        image_line postgres library/postgres 15-alpine "$postgres_digest" \
            helm-string:zone-apps:initContainers.postgres.image
    } > "$bundle/images.txt"
}

write_layouts() {
    while IFS="$tab" read -r name _ _ digest layout _; do
        mkdir -p "$bundle/$layout/blobs/sha256"
        printf '%s\n' '{"imageLayoutVersion":"1.0.0"}' > "$bundle/$layout/oci-layout"
        printf '{"schemaVersion":2,"manifests":[{"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"%s","size":2}]}\n' \
            "$digest" > "$bundle/$layout/index.json"
        printf '%s manifest\n' "$name" > "$bundle/$layout/blobs/sha256/${digest#sha256:}"
    done < "$bundle/images.txt"
}

write_override() {
    mkdir -p "$bundle/compose"
    cat > "$bundle/compose/docker-compose.airgap.yml" <<YAML
services:
  manager:
    image: \${ZONE_AIRGAP_REGISTRY:-127.0.0.1:5000}/zone/server@$server_digest
    build: !reset null
    pull_policy: missing
  traefik:
    image: \${ZONE_AIRGAP_REGISTRY:-127.0.0.1:5000}/library/traefik@$traefik_digest
    pull_policy: missing
YAML
}

write_source() {
    variant=$1
    source_directory=$sandbox/source
    rm -rf "$source_directory"
    mkdir -p "$source_directory/scripts"
    printf '%s\n' 'services: {}' > "$source_directory/docker-compose.yml"
    printf '%s\n' '# ZONE_LICENSE_RENEWAL=true' > "$source_directory/.env.example"
    cp "$scripts/env-file.sh" "$source_directory/scripts/env-file.sh"
    set -- docker-compose.yml .env.example scripts
    if [ "$variant" = with-env ]; then
        printf '%s\n' 'FROM_SOURCE=1' > "$source_directory/.env"
        set -- "$@" .env
    fi
    (cd "$source_directory" && tar -cf "$bundle/source.tar" "$@")
}

write_crane() {
    mkdir -p "$bundle/bin" "$bundle/LICENSES"
    cat > "$bundle/bin/crane" <<'CRANE'
#!/bin/sh
printf '%s\n' "$*" >> "$CRANE_LOG"
case "$1" in
    catalog) exit 0 ;;
    push)
        shift
        if [ "$1" = --insecure ]; then
            shift
        fi
        [ "${CRANE_PUSH_STATUS:-0}" -eq 0 ] || exit "$CRANE_PUSH_STATUS"
        digest=${CRANE_DIGEST:-$(sed -n 's/.*"digest":"\(sha256:[0-9a-f]*\)".*/\1/p' "$1/index.json")}
        repository=${2%@*}
        case "${repository##*/}" in
            *:*) repository=${repository%:*} ;;
        esac
        printf '%s@%s\n' "$repository" "$digest"
        ;;
    *) exit 1 ;;
esac
CRANE
    chmod +x "$bundle/bin/crane"
    printf '%s\n' 'Apache License 2.0' > "$bundle/LICENSES/crane"
}

seal() {
    (
        cd "$bundle"
        find . -type f ! -path ./SHA256SUMS | sed 's|^\./||' | LC_ALL=C sort > "$sandbox/members"
        while IFS= read -r member; do
            checksum "$member"
        done < "$sandbox/members" > SHA256SUMS
    )
}

build_bundle() {
    rm -rf "$bundle"
    mkdir -p "$bundle/images" "$bundle/helm"
    printf '%s\n' 1.4.3-lts > "$bundle/VERSION"
    cp "$scripts/install-offline.sh" "$bundle/install.sh"
    write_crane
    write_source "${1:-}"
    write_override
    printf '%s\n' 'chart' > "$bundle/helm/zone-apps-0.1.0.tgz"
    printf '%s\n' 'registry image' > "$bundle/images/registry.tar"
    write_images
    write_layouts
    seal
}

cat > "$sandbox/docker" <<'DOCKER'
#!/bin/sh
printf '%s\n' "$*" >> "$DOCKER_LOG"
case "$1" in
    compose)
        case " $* " in
            *' version --short '*) printf '%s\n' "${COMPOSE_VERSION:-2.24.0}" ;;
            *' config --profiles '*) printf '%s\n' bundled-ollama monitoring vpn ;;
            *' pull '*)
                printf 'pull-environment ZONE_AIRGAP_REGISTRY=%s directory=%s\n' "${ZONE_AIRGAP_REGISTRY:-}" "$PWD" >> "$DOCKER_LOG"
                exit "${COMPOSE_PULL_STATUS:-0}"
                ;;
            *) exit 1 ;;
        esac
        ;;
    load)
        case "${DOCKER_LOADED:-reference}" in
            reference) printf '%s\n' 'Loaded image: registry:i-was-a-digest' ;;
            id) printf '%s\n' 'Loaded image ID: sha256:5f1ee4f4d2b2d7e2ab7e8f3f0e0e4a1b1c7d0f8f4a6a2c3d9e8b7a6f5e4d3c2b' ;;
            nothing) ;;
        esac
        ;;
    volume)
        if [ "$2" = create ]; then
            printf '%s\n' "$3"
        fi
        ;;
    run)
        [ "${DOCKER_RUN_STATUS:-0}" -eq 0 ] || exit "$DOCKER_RUN_STATUS"
        printf '%s\n' 0123456789ab
        ;;
    rm) ;;
    *) exit 1 ;;
esac
DOCKER
mkdir -p "$sandbox/bin"
mv "$sandbox/docker" "$sandbox/bin/docker"
chmod +x "$sandbox/bin/docker"

run_installer_with() {
    environment_words=$1
    shift
    : > "$sandbox/docker.log"
    : > "$sandbox/crane.log"
    mkdir -p "$work"
    status=0
    (
        cd "$work"
        # environment_words holds NAME=value words for env.
        # shellcheck disable=SC2086
        env PATH="$sandbox/bin:$PATH" DOCKER_LOG="$sandbox/docker.log" CRANE_LOG="$sandbox/crane.log" \
            $environment_words "$shell" "$bundle/install.sh" "$@"
    ) > "$sandbox/output" 2>&1 || status=$?
    return "$status"
}

run_installer() {
    run_installer_with '' "$@"
}

assert_output() {
    grep -qF -- "$1" "$sandbox/output" || fail "Expected output containing: $1"
}

assert_docker() {
    grep -qxF -- "$1" "$sandbox/docker.log" || fail "Expected the docker call: $1"
}

assert_docker_containing() {
    grep -qF -- "$1" "$sandbox/docker.log" || fail "Expected a docker call containing: $1"
}

assert_no_docker_containing() {
    if grep -qF -- "$1" "$sandbox/docker.log"; then
        fail "Expected no docker call containing: $1"
    fi
}

assert_crane() {
    grep -qxF -- "$1" "$sandbox/crane.log" || fail "Expected the crane call: $1"
}

assert_env_line() {
    count=$(grep -cxF -- "$1" "$2" || true)
    [ "$count" -eq 1 ] || fail "Expected exactly one '$1' line in $2, found $count"
}

assert_env_absent() {
    if grep -q "^$1=" "$2"; then
        fail "Expected no $1 line in $2"
    fi
}

assert_registry_removed() {
    container=$(sed -n 's/^volume create \(zone-airgap-registry-[0-9]*\)$/\1/p' "$sandbox/docker.log")
    [ -n "$container" ] || fail 'Expected a temporary registry volume to be created'
    assert_docker "rm --force --volumes $container"
    assert_docker "volume rm --force $container"
}

target=$work/zone
environment=$target/.env

build_bundle

run_installer verify || fail 'Expected a pristine bundle to verify'
assert_output 'Verified '
assert_output ' files against SHA256SUMS'

printf '%s\n' 'tampered' >> "$bundle/images/zone-server/blobs/sha256/${server_digest#sha256:}"
if run_installer compose; then
    fail 'Expected a tampered layout to abort compose'
fi
assert_output "images/zone-server/blobs/sha256/${server_digest#sha256:}: FAILED"
assert_output 'does not match SHA256SUMS'
[ ! -s "$sandbox/docker.log" ] || fail 'Expected a checksum mismatch to abort before any docker call'
[ ! -s "$sandbox/crane.log" ] || fail 'Expected a checksum mismatch to abort before any crane call'
[ ! -d "$target" ] || fail 'Expected a checksum mismatch to abort before the directory is created'
for command in verify 'push --registry registry.example.internal' 'helm-values --registry registry.example.internal --output values'; do
    # Intentional split: each entry is a command and its options.
    # shellcheck disable=SC2086
    if run_installer $command; then
        fail "Expected a tampered layout to abort $command"
    fi
    [ ! -s "$sandbox/crane.log" ] || fail "Expected $command to abort before any crane call"
done
build_bundle

printf '%s\n' 'extra' > "$bundle/images/registry.tar"
if run_installer verify; then
    fail 'Expected a changed registry archive to fail verify'
fi
build_bundle

printf '%s  %s\n' "$(printf '%064d' 0)" '../outside' >> "$bundle/SHA256SUMS"
if run_installer verify; then
    fail 'Expected a SHA256SUMS path outside the bundle to be refused'
fi
assert_output 'names a path outside the bundle: ../outside'
build_bundle

rm -f "$bundle/SHA256SUMS"
if run_installer verify; then
    fail 'Expected a missing SHA256SUMS to be refused'
fi
assert_output 'SHA256SUMS is missing'
build_bundle

printf 'broken\tline\n' >> "$bundle/images.txt"
seal
if run_installer verify; then
    :
else
    fail 'Expected verify to check checksums only'
fi
if run_installer push --registry registry.example.internal; then
    fail 'Expected an images.txt line with two fields to be refused'
fi
assert_output 'images.txt line 11: expected 6 TAB-separated fields, found 2'
[ ! -s "$sandbox/crane.log" ] || fail 'Expected a malformed images.txt to stop before any push'
build_bundle

for version in 2.23.9 v2.23.0 1.29.2; do
    rm -rf "$target"
    mkdir -p "$target"
    printf '%s\n' 'KEEP=me' > "$environment"
    if run_installer_with "COMPOSE_VERSION=$version" compose --directory "$target"; then
        fail "Expected Compose $version to be refused"
    fi
    assert_output "Docker Compose ${version#v} is too old. This bundle needs 2.24 or later"
    assert_docker 'compose version --short'
    assert_no_docker_containing 'load'
    [ ! -f "$target/docker-compose.yml" ] || fail "Expected Compose $version to be refused before the source is extracted"
done

rm -rf "$target"
if run_installer compose; then
    fail 'Expected compose without a .env to stop'
fi
assert_output "$environment does not exist yet. Create it with 'cd $target && ./scripts/setup.sh --skip-models'"
[ -f "$target/scripts/env-file.sh" ] || fail 'Expected the source to be extracted before the .env check'
assert_no_docker_containing 'load'

printf '%s\n' '# Operator settings' 'KEEP=me' 'ZONE_LICENSE_RENEWAL=' > "$environment"
run_installer_with 'COMPOSE_VERSION=v2.24.0' compose \
    || fail 'Expected compose to load the bundle into the default ./zone directory'
assert_env_line '# Operator settings' "$environment"
assert_env_line 'KEEP=me' "$environment"
assert_env_line 'ZONE_AIRGAP_REGISTRY=127.0.0.1:5000' "$environment"
assert_env_line 'ZONE_LICENSE_RENEWAL=false' "$environment"
[ "$(grep -c '^ZONE_LICENSE_RENEWAL=' "$environment")" -eq 1 ] || fail 'Expected the blank ZONE_LICENSE_RENEWAL to be replaced, not repeated'
assert_env_absent COMPOSE_FILE "$environment"
cmp -s "$bundle/compose/docker-compose.airgap.yml" "$target/docker-compose.airgap.yml" \
    || fail 'Expected docker-compose.airgap.yml to be copied byte for byte'
[ -f "$target/docker-compose.yml" ] || fail 'Expected source.tar to be extracted'
assert_docker 'load --input '"$bundle"'/images/registry.tar'
assert_docker_containing '--publish 127.0.0.1:5000:5000'
assert_docker_containing ':/var/lib/registry registry:i-was-a-digest'
if grep -qF 'registry:3.1.1' "$sandbox/docker.log"; then
    fail 'Expected compose to run the name docker load reported, not registry:3.1.1, which crane never tags'
fi
assert_docker 'compose -f docker-compose.yml -f docker-compose.airgap.yml config --profiles'
assert_docker 'compose -f docker-compose.yml -f docker-compose.airgap.yml --profile bundled-ollama --profile monitoring --profile vpn pull manager console litellm traefik valkey ollama ollama-init'
assert_docker "pull-environment ZONE_AIRGAP_REGISTRY=127.0.0.1:5000 directory=$target"
assert_registry_removed
assert_crane 'catalog --insecure 127.0.0.1:5000'
assert_crane "push --insecure $bundle/images/zone-server 127.0.0.1:5000/zone/server@$server_digest"
assert_crane "push --insecure $bundle/images/zone-server 127.0.0.1:5000/zone/server:1.4.3-lts"
assert_crane "push --insecure $bundle/images/postgres 127.0.0.1:5000/library/postgres@$postgres_digest"
assert_crane "push --insecure $bundle/images/postgres 127.0.0.1:5000/library/postgres:15-alpine"
[ "$(grep -c '^push ' "$sandbox/crane.log")" -eq 20 ] || fail 'Expected every image to be pushed by digest and by tag'
load_line=$(grep -n '^load ' "$sandbox/docker.log" | cut -d: -f1)
pull_line=$(grep -n ' pull ' "$sandbox/docker.log" | cut -d: -f1)
remove_line=$(grep -n '^rm --force' "$sandbox/docker.log" | cut -d: -f1)
[ "$load_line" -lt "$pull_line" ] && [ "$pull_line" -lt "$remove_line" ] \
    || fail 'Expected load, then pull, then the registry removal'
assert_output 'Added ZONE_AIRGAP_REGISTRY=127.0.0.1:5000'
assert_output 'Added ZONE_LICENSE_RENEWAL=false'
assert_output 'Zone 1.4.3-lts is loaded.'
assert_output '  make up'
assert_output '  ./scripts/compose.sh up -d'
assert_output '  docker compose -f docker-compose.yml -f docker-compose.airgap.yml up -d'

cp "$environment" "$sandbox/env.before"
run_installer_with 'COMPOSE_VERSION=5.7.0' compose --directory "$target" \
    || fail 'Expected a second compose run into the same directory to pass'
cmp -s "$sandbox/env.before" "$environment" || fail 'Expected a second run to leave .env unchanged'
assert_output 'Kept ZONE_AIRGAP_REGISTRY'
assert_output 'Kept ZONE_LICENSE_RENEWAL'

printf '%s\n' 'KEEP=me' 'export ZONE_AIRGAP_REGISTRY=127.0.0.1:5001' 'ZONE_LICENSE_RENEWAL=true # renews through a proxy' > "$environment"
cp "$environment" "$sandbox/env.before"
run_installer compose --directory "$target" || fail 'Expected compose to adopt the port already in .env'
cmp -s "$sandbox/env.before" "$environment" || fail 'Expected settings already in .env to be kept as they are'
assert_docker_containing '--publish 127.0.0.1:5001:5000'
assert_crane "push --insecure $bundle/images/zone-server 127.0.0.1:5001/zone/server@$server_digest"
assert_docker "pull-environment ZONE_AIRGAP_REGISTRY=127.0.0.1:5001 directory=$target"

if run_installer compose --directory "$target" --port 5002; then
    fail 'Expected a --port that disagrees with .env to be refused'
fi
assert_output 'ZONE_AIRGAP_REGISTRY in '"$environment"' is 127.0.0.1:5001. Run with --port 5001'
assert_no_docker_containing 'load'

printf '%s\n' 'ZONE_AIRGAP_REGISTRY=registry.example.internal:5443' > "$environment"
if run_installer compose --directory "$target"; then
    fail 'Expected a non-loopback ZONE_AIRGAP_REGISTRY to be refused'
fi
assert_output "install.sh push --registry registry.example.internal:5443"
assert_no_docker_containing 'load'

if run_installer compose --directory "$target" --port 70000; then
    fail 'Expected an out-of-range port to be refused'
fi
assert_output '--port must be a number from 1 to 65535: 70000'

build_bundle with-env
printf '%s\n' 'KEEP=me' > "$environment"
run_installer compose --directory "$target" || fail 'Expected compose to pass with a source.tar that carries a .env'
assert_env_line 'KEEP=me' "$environment"
assert_env_absent FROM_SOURCE "$environment"
build_bundle

printf '%s\n' 'KEEP=me' > "$environment"
cp "$environment" "$sandbox/env.before"
rm -f "$target/docker-compose.airgap.yml"
if run_installer_with 'CRANE_PUSH_STATUS=1' compose --directory "$target"; then
    fail 'Expected a failed push to fail compose'
fi
assert_output 'crane could not push zone-server'
assert_registry_removed
cmp -s "$sandbox/env.before" "$environment" || fail 'Expected a failed push to leave .env untouched'
[ ! -f "$target/docker-compose.airgap.yml" ] || fail 'Expected a failed push to leave the override uncopied'

if run_installer_with 'COMPOSE_PULL_STATUS=1' compose --directory "$target"; then
    fail 'Expected a failed pull to fail compose'
fi
assert_output 'docker compose pull failed'
assert_registry_removed

if run_installer_with 'DOCKER_RUN_STATUS=125' compose --directory "$target"; then
    fail 'Expected a registry that cannot start to fail compose'
fi
assert_output 'Could not start the temporary registry on 127.0.0.1:5000'
assert_registry_removed

run_installer_with 'DOCKER_LOADED=id' compose --directory "$target" \
    || fail 'Expected compose to run an image that docker load reports only by ID'
assert_docker_containing ':/var/lib/registry sha256:5f1ee4f4d2b2d7e2ab7e8f3f0e0e4a1b1c7d0f8f4a6a2c3d9e8b7a6f5e4d3c2b'

if run_installer_with 'DOCKER_LOADED=nothing' compose --directory "$target"; then
    fail 'Expected compose to stop when docker load names no image'
fi
assert_output 'docker load did not name the image it loaded from images/registry.tar'
assert_no_docker_containing 'run '

if run_installer_with "CRANE_DIGEST=$manager_digest" compose --directory "$target"; then
    fail 'Expected a push that changes the digest to fail compose'
fi
assert_output "crane pushed zone-server as $manager_digest, but images.txt pins $server_digest"
assert_registry_removed

run_installer push --registry registry.example.internal:5443/mirror/ || fail 'Expected push to pass'
[ ! -s "$sandbox/docker.log" ] || fail 'Expected push not to call docker'
while IFS="$tab" read -r name repository tag digest layout _; do
    assert_crane "push $bundle/$layout registry.example.internal:5443/mirror/$repository@$digest"
    assert_crane "push $bundle/$layout registry.example.internal:5443/mirror/$repository:$tag"
    assert_output "Pushed $name as registry.example.internal:5443/mirror/$repository:$tag@$digest"
done < "$bundle/images.txt"
[ "$(grep -c '^push ' "$sandbox/crane.log")" -eq 20 ] || fail 'Expected push to push each image twice, by digest and by tag'
if grep -q -- '--insecure' "$sandbox/crane.log"; then
    fail 'Expected push to use TLS unless --insecure is given'
fi

run_installer push --registry registry.example.internal --insecure || fail 'Expected push --insecure to pass'
assert_crane "push --insecure $bundle/images/zone-litellm registry.example.internal/zone/litellm@$litellm_digest"

if run_installer_with "CRANE_DIGEST=$manager_digest" push --registry registry.example.internal; then
    fail 'Expected push to refuse a changed digest'
fi
assert_output "crane pushed zone-server as $manager_digest, but images.txt pins $server_digest"

for registry in 'https://registry.example.internal' 'registry.example.internal/Upper' 'registry example' ''; do
    if run_installer push --registry "$registry"; then
        fail "Expected the registry '$registry' to be refused"
    fi
    [ ! -s "$sandbox/crane.log" ] || fail "Expected the registry '$registry' to be refused before any push"
done

if run_installer push; then
    fail 'Expected push without --registry to be refused'
fi
assert_output 'push needs --registry HOST[/PREFIX]'

cat > "$sandbox/zone-apps.yaml" <<YAML
server:
  image:
    repository: "registry.example.internal:5443/mirror/zone/server"
    tag: "1.4.3-lts@$server_digest"
migration:
  image:
    repository: "registry.example.internal:5443/mirror/zone/server"
    tag: "1.4.3-lts@$server_digest"
manager:
  image:
    repository: "registry.example.internal:5443/mirror/zone/manager"
    tag: "1.4.3-lts@$manager_digest"
initContainers:
  postgres:
    image: "registry.example.internal:5443/mirror/library/postgres:15-alpine@$postgres_digest"
YAML
cat > "$sandbox/zone-infra.yaml" <<YAML
valkey:
  image:
    repository: "registry.example.internal:5443/mirror/valkey/valkey"
    tag: "8.1.10-alpine@$valkey_digest"
postgres:
  image: "registry.example.internal:5443/mirror/cloudnative-pg/postgresql:16.15@$cloudnative_digest"
YAML
cat > "$sandbox/zone-ai.yaml" <<YAML
ollama:
  image:
    repository: "registry.example.internal:5443/mirror/ollama/ollama"
    tag: "0.33.2@$ollama_digest"
  init:
    image:
      repository: "registry.example.internal:5443/mirror/curlimages/curl"
      tag: "8.22.0@$curl_digest"
litellm:
  image:
    repository: "registry.example.internal:5443/mirror/berriai/litellm"
    tag: "v1.99.1@$upstream_litellm_digest"
YAML

for registry in registry.example.internal:5443/mirror registry.example.internal:5443/mirror//; do
    rm -rf "$sandbox/values"
    run_installer helm-values --registry "$registry" --output "$sandbox/values" || fail "Expected helm-values to pass for $registry"
    [ ! -s "$sandbox/docker.log" ] || fail 'Expected helm-values not to call docker'
    [ ! -s "$sandbox/crane.log" ] || fail 'Expected helm-values not to call crane'
    for chart in zone-apps zone-infra zone-ai; do
        if ! cmp -s "$sandbox/$chart.yaml" "$sandbox/values/$chart.yaml"; then
            diff -u "$sandbox/$chart.yaml" "$sandbox/values/$chart.yaml" >&2 || true
            fail "Expected $chart.yaml to equal the golden values for $registry"
        fi
        assert_output "Wrote $sandbox/values/$chart.yaml"
    done
    [ "$(find "$sandbox/values" -type f | wc -l | tr -d ' ')" -eq 3 ] || fail 'Expected exactly three values files'
    if grep -q 'imageRegistry' "$sandbox/values/zone-apps.yaml"; then
        fail 'Expected per-key repositories, never global.imageRegistry'
    fi
done

if run_installer helm-values --registry registry.example.internal; then
    fail 'Expected helm-values without --output to be refused'
fi
assert_output 'helm-values needs --output DIR'

image_line conflict library/conflict 1 "$server_digest" helm-string:zone-ai:ollama.image >> "$bundle/images.txt"
write_layouts
seal
if run_installer helm-values --registry registry.example.internal --output "$sandbox/conflict"; then
    fail 'Expected a path set both as a value and as a map to be refused'
fi
assert_output 'zone-ai: ollama.image is set as a value and as a map'
build_bundle

if run_installer unknown; then
    fail 'Expected an unknown command to be refused'
fi
assert_output 'Unknown command: unknown'
run_installer help || fail 'Expected help to pass'
assert_output 'helm-values --registry HOST[/PREFIX] --output DIR'

printf 'Offline installer checks passed under %s\n' "$shell"
