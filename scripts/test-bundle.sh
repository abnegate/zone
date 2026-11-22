#!/bin/sh
set -eu

directory=$(mktemp -d)
trap 'rm -rf "$directory"' EXIT HUP INT TERM
source_directory=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
shell=$(command -v "${BASH:-sh}")
version=1.4.3-lts
tab=$(printf '\t')
architecture=amd64
failing_pull=''
run_path=''
top=zone-bundle-$version-amd64
secret=zone-bundle-test-secret-sentinel
helper='registry:3.1.1@sha256:325b4b29b041e82803abeb703e201655e4e23ab83264ec1a7c9ddb0a5b14a6e0'
traefik='traefik:v3.7.12@sha256:9c2a54d87f76f5c2f5f2682c68394af92fb12c0a2686798d6462a3f84bd78eaf'
ollama='ollama/ollama:0.33.2@sha256:020e4134285e2ef4d8fd801234176de3b4faadc992a3eb06c8e66a2f9d4c4ba2'
postgres='ghcr.io/cloudnative-pg/postgresql:16.15@sha256:34cd4159d07b3410a1b29da35072e2e927a0905e717a61afa3814624a2e8859a'

digest() {
    printf 'sha256:%064d' "$1"
}

if command -v sha256sum > /dev/null 2>&1; then
    verify_checksums() {
        sha256sum -c "$1" > /dev/null
    }
else
    verify_checksums() {
        shasum -a 256 -c "$1" > /dev/null
    }
fi

mkdir -p "$directory/bin" "$directory/crane" "$directory/runs"

cat > "$directory/bin/crane" <<'CRANE'
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
        *) break ;;
    esac
done
case $command in
    digest)
        digest=$(awk -v reference="$1" -v platform="$platform" '$1 == reference && $2 == platform { print $3 }' "$CRANE_DIGESTS")
        if [ -z "$digest" ]; then
            printf 'MANIFEST_UNKNOWN: %s for %s\n' "$1" "$platform" >&2
            exit 1
        fi
        printf '%s\n' "$digest"
        ;;
    pull)
        if [ "$(basename -- "$2")" = "${CRANE_FAILING_PULL:-}" ]; then
            printf 'UNAUTHORIZED: %s\n' "$1" >&2
            exit 1
        fi
        case $format in
            oci)
                for sibling in "$(dirname -- "$2")"/* "$(dirname -- "$2")"/.[!.]*; do
                    if [ -e "$sibling" ]; then
                        printf '%s present while pulling %s\n' "$(basename -- "$sibling")" "$1" >> "$CRANE_SIBLINGS"
                    fi
                done
                mkdir -p "$2/blobs/sha256"
                printf '{"imageLayoutVersion":"1.0.0"}\n' > "$2/oci-layout"
                printf '{"reference":"%s"}\n' "$1" > "$2/index.json"
                printf '%s %s\n' "$1" "$platform" > "$2/blobs/sha256/layer"
                ;;
            tarball)
                printf '%s %s\n' "$1" "$platform" > "$2"
                ;;
            *) exit 1 ;;
        esac
        ;;
    *) exit 1 ;;
esac
CRANE

cat > "$directory/bin/helm" <<'HELM'
#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$HELM_LOG"
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
printf '%s\n' "$*" >> "$GIT_LOG"
if [ "$1" != -C ] || [ "$3" != archive ] || [ "$4" != --format=tar ] || [ "$5" != -o ] || [ "$7" != HEAD ]; then
    exit 1
fi
printf 'source of %s\n' "$(basename -- "$2")" > "$6"
GIT

chmod +x "$directory/bin/crane" "$directory/bin/helm" "$directory/bin/git"
cp "$directory/bin/crane" "$directory/crane/crane"
printf '%s\n' 'Apache License, Version 2.0' > "$directory/crane/LICENSE"

{
    printf 'zone-server   zone:server   zone/server   compose:manager,helm-split:zone-apps:server.image\n'
    printf 'zone-manager\tzone:manager\tzone/manager\tcompose:console,helm-split:zone-apps:manager.image\n'
    printf 'zone-litellm \t zone:litellm \t zone/litellm \t compose:litellm\n'
    printf '\n'
    printf '# Third-party images, pinned by index digest\n'
    printf 'traefik  %s  library/traefik  compose:traefik\n' "$traefik"
    printf '   \t\n'
    printf 'ollama  %s  ollama/ollama  compose:ollama,compose:ollama-init,helm-split:zone-ai:ollama.image\n' "$ollama"
    printf 'cloudnative-postgresql  %s  cloudnative-pg/postgresql  helm-string:zone-infra:postgres.image\n' "$postgres"
    printf '\n'
    printf '#excluded comfyui builds a CUDA image from source\n'
    printf '#excluded comfyui-model-setup downloads models from the internet'
} > "$directory/bundle-images.txt"

write_digests() {
    target=$1
    shift
    : > "$directory/digests.tmp"
    for platform in amd64 arm64; do
        offset=0
        [ "$platform" = amd64 ] || offset=10
        number=1
        for reference in \
            "127.0.0.1:5000/zone/server:$version" \
            "127.0.0.1:5000/zone/manager:$version" \
            "127.0.0.1:5000/zone/litellm:$version" \
            "$traefik" \
            "$ollama" \
            "$postgres"; do
            line="$reference linux/$platform $(digest $((number + offset)))"
            skip=0
            for missing do
                [ "$missing" != "$reference linux/$platform" ] || skip=1
            done
            [ "$skip" -eq 1 ] || printf '%s\n' "$line" >> "$directory/digests.tmp"
            number=$((number + 1))
        done
    done
    mv "$directory/digests.tmp" "$target"
}

make_sandbox() {
    sandbox=$directory/zone-$1
    rm -rf "$sandbox"
    mkdir -p "$sandbox/scripts" "$sandbox/helm/zone-apps" "$sandbox/helm/zone-infra" "$sandbox/helm/zone-ai"
    cp "$source_directory/bundle.sh" "$sandbox/scripts/bundle.sh"
    cp "${2:-$directory/bundle-images.txt}" "$sandbox/scripts/bundle-images.txt"
    printf '#!/bin/sh\nprintf "%%s\\n" "offline installer"\n' > "$sandbox/scripts/install-offline.sh"
    for chart in zone-apps:0.4.0 zone-infra:0.2.1 zone-ai:0.3.0; do
        printf 'apiVersion: v2\nname: %s\nversion: %s\n' "${chart%%:*}" "${chart#*:}" > "$sandbox/helm/${chart%%:*}/Chart.yaml"
    done
    script=$sandbox/scripts/bundle.sh
}

fail() {
    printf '%s\n' "$1" >&2
    if [ -f "$directory/output" ]; then
        sed 's/^/  output: /' "$directory/output" >&2
    fi
    if [ -f "$directory/crane.log" ]; then
        sed 's/^/  crane: /' "$directory/crane.log" >&2
    fi
    exit 1
}

bundle() {
    : > "$directory/crane.log"
    : > "$directory/siblings.log"
    : > "$directory/helm.log"
    : > "$directory/git.log"
    status=0
    env \
        PATH="${run_path:-$directory/bin:$PATH}" \
        CRANE_LOG="$directory/crane.log" \
        CRANE_SIBLINGS="$directory/siblings.log" \
        CRANE_DIGESTS="$digests_file" \
        CRANE_FAILING_PULL="$failing_pull" \
        HELM_LOG="$directory/helm.log" \
        GIT_LOG="$directory/git.log" \
        ZONE_BUNDLE_TEST_SECRET="$secret" \
        AWS_SECRET_ACCESS_KEY="$secret" \
        "$shell" "$script" "$@" > "$directory/output" 2>&1 || status=$?
    if grep -qF -- "$secret" "$directory/output"; then
        fail 'Expected the bundler never to print the environment'
    fi
    return "$status"
}

bundle_to() {
    out=$directory/runs/$1
    shift
    bundle --version "$version" --architecture "$architecture" --registry 127.0.0.1:5000 \
        --crane "$directory/crane/crane" --output "$out" "$@"
}

assert_output() {
    grep -qF -- "$1" "$directory/output" || fail "Expected output containing: $1"
}

assert_crane_called() {
    grep -qxF -- "$1" "$directory/crane.log" || fail "Expected a crane call: $1"
}

assert_nothing_pulled() {
    if grep -q '^pull' "$directory/crane.log"; then
        fail 'Expected nothing to be pulled'
    fi
}

assert_nothing_written() {
    if [ -d "$out" ] && [ -n "$(ls -A "$out")" ]; then
        fail "Expected $out to stay empty, found: $(ls -A "$out")"
    fi
}

assert_refused() {
    expectation=$1
    message=$2
    shift 2
    if bundle_to refused "$@"; then
        fail "Expected the bundler to refuse $expectation"
    fi
    assert_output "$message"
    [ ! -s "$directory/crane.log" ] || fail "Expected no crane call when refusing $expectation"
    assert_nothing_written
}

assert_manifest_refused() {
    expectation=$1
    message=$2
    shift 2
    printf '%s\n' "$@" > "$directory/broken.txt"
    make_sandbox broken "$directory/broken.txt"
    if bundle_to broken; then
        fail "Expected the bundler to refuse a manifest with $expectation"
    fi
    assert_output "$message"
    [ ! -s "$directory/crane.log" ] || fail "Expected no crane call for a manifest with $expectation"
    assert_nothing_written
}

members() {
    tar -tf "$1" | sed -e "s|^$2/||" -e 's|^\(images/[^/]*\)/.*|\1/|' | uniq
}

digests_file=$directory/digests
write_digests "$digests_file"

make_sandbox main
bundle_to amd64 || fail 'Expected a bundle to build'
archive=$out/$top.tar

[ "$(cd "$out" && find . ! -name . | LC_ALL=C sort | tr '\n' ' ')" = "./$top.tar ./$top.tar.sha256 " ] \
    || fail "Expected only the tar and its checksum in the output, found: $(ls -A "$out")"

cat > "$directory/expected-members" <<MEMBERS
VERSION
install.sh
bin/crane
LICENSES/crane
source.tar
compose/docker-compose.airgap.yml
helm/zone-ai-0.3.0.tgz
helm/zone-apps-0.4.0.tgz
helm/zone-infra-0.2.1.tgz
images/registry.tar
images/zone-server/
images/zone-manager/
images/zone-litellm/
images/traefik/
images/ollama/
images/cloudnative-postgresql/
images.txt
SHA256SUMS
MEMBERS
members "$archive" "$top" > "$directory/actual-members"
cmp -s "$directory/expected-members" "$directory/actual-members" \
    || fail "Expected the members in contract order, found: $(tr '\n' ' ' < "$directory/actual-members")"
if tar -tf "$archive" | grep -v "^$top/" > /dev/null; then
    fail "Expected every member under $top/"
fi
case $(tar --version 2> /dev/null || true) in
    *'GNU tar'*) owners=$(tar -tvf "$archive" | awk '{ print $2 }') ;;
    *) owners=$(tar -tvf "$archive" | awk '{ print $3 "/" $4 }') ;;
esac
foreign=$(printf '%s\n' "$owners" | grep -vx '0/0' || true)
[ -z "$foreign" ] \
    || fail "Expected every member owned by uid 0 and gid 0 with no names, found: $(printf '%s\n' "$foreign" | sort -u | tr '\n' ' ')"

[ ! -s "$directory/siblings.log" ] \
    || fail "Expected each layout to be appended and deleted before the next pull: $(cat "$directory/siblings.log")"

printf '%s  %s\n' "$(cd "$out" && verify_checksums "$top.tar.sha256" && sed 's/ .*//' "$top.tar.sha256")" "$top.tar" > "$directory/outer"
cmp -s "$directory/outer" "$out/$top.tar.sha256" || fail 'Expected the outer checksum to be one sha256sum line for the tar'

mkdir "$directory/extracted"
tar -xf "$archive" -C "$directory/extracted"
bundle_root=$directory/extracted/$top
(cd "$bundle_root" && verify_checksums SHA256SUMS) || fail 'Expected SHA256SUMS to verify every member'
(cd "$bundle_root" && find . -type f ! -name SHA256SUMS | sed 's|^\./||' | LC_ALL=C sort) > "$directory/files"
cut -c67- "$bundle_root/SHA256SUMS" | LC_ALL=C sort > "$directory/summed"
cmp -s "$directory/files" "$directory/summed" || fail 'Expected SHA256SUMS to cover every member exactly once'
grep -qx 'images/traefik/blobs/sha256/layer' "$directory/summed" || fail 'Expected layout files in SHA256SUMS'

[ "$(cat "$bundle_root/VERSION")" = "$version" ] || fail 'Expected VERSION to hold the version'
cmp -s "$bundle_root/bin/crane" "$directory/crane/crane" || fail 'Expected bin/crane to be the given crane'
[ -x "$bundle_root/bin/crane" ] || fail 'Expected bin/crane to be executable'
cmp -s "$bundle_root/LICENSES/crane" "$directory/crane/LICENSE" || fail 'Expected LICENSES/crane to be the crane license'
cmp -s "$bundle_root/install.sh" "$sandbox/scripts/install-offline.sh" || fail 'Expected install.sh to be scripts/install-offline.sh'
[ -x "$bundle_root/install.sh" ] || fail 'Expected install.sh to be executable'
[ "$(cat "$bundle_root/source.tar")" = "source of zone-main" ] || fail 'Expected source.tar from git archive'
grep -qxF -- "-C $sandbox archive --format=tar -o $out/$top.partial/$top/source.tar HEAD" "$directory/git.log" \
    || fail "Expected git archive of HEAD, found: $(cat "$directory/git.log")"
[ "$(grep -c '^package ' "$directory/helm.log")" -eq 3 ] || fail 'Expected every helm/*/Chart.yaml to be packaged'
[ "$(cat "$bundle_root/images/registry.tar")" = "$helper linux/amd64" ] || fail 'Expected the registry helper tarball'

tr '|' '\t' > "$directory/golden-images.txt" <<IMAGES
zone-server|zone/server|$version|$(digest 1)|images/zone-server|compose:manager,helm-split:zone-apps:server.image
zone-manager|zone/manager|$version|$(digest 2)|images/zone-manager|compose:console,helm-split:zone-apps:manager.image
zone-litellm|zone/litellm|$version|$(digest 3)|images/zone-litellm|compose:litellm
traefik|library/traefik|v3.7.12|$(digest 4)|images/traefik|compose:traefik
ollama|ollama/ollama|0.33.2|$(digest 5)|images/ollama|compose:ollama,compose:ollama-init,helm-split:zone-ai:ollama.image
cloudnative-postgresql|cloudnative-pg/postgresql|16.15|$(digest 6)|images/cloudnative-postgresql|helm-string:zone-infra:postgres.image
IMAGES
cmp -s "$directory/golden-images.txt" "$bundle_root/images.txt" \
    || fail "Expected images.txt to equal the golden file, found: $(cat "$bundle_root/images.txt")"

cat > "$directory/golden-override.yml" <<OVERRIDE
services:
  manager:
    image: \${ZONE_AIRGAP_REGISTRY:-127.0.0.1:5000}/zone/server@$(digest 1)
    build: !reset null
    pull_policy: missing
  console:
    image: \${ZONE_AIRGAP_REGISTRY:-127.0.0.1:5000}/zone/manager@$(digest 2)
    build: !reset null
    pull_policy: missing
  litellm:
    image: \${ZONE_AIRGAP_REGISTRY:-127.0.0.1:5000}/zone/litellm@$(digest 3)
    build: !reset null
    pull_policy: missing
  traefik:
    image: \${ZONE_AIRGAP_REGISTRY:-127.0.0.1:5000}/library/traefik@$(digest 4)
    pull_policy: missing
  ollama:
    image: \${ZONE_AIRGAP_REGISTRY:-127.0.0.1:5000}/ollama/ollama@$(digest 5)
    pull_policy: missing
  ollama-init:
    image: \${ZONE_AIRGAP_REGISTRY:-127.0.0.1:5000}/ollama/ollama@$(digest 5)
    pull_policy: missing
OVERRIDE
override=$bundle_root/compose/docker-compose.airgap.yml
cmp -s "$directory/golden-override.yml" "$override" \
    || fail "Expected the Compose override to equal the golden file, found: $(cat "$override")"

built_services() {
    awk '/^  [^ ]/ { service = $1; sub(/:$/, "", service) } /^    build: !reset null$/ { printf "%s ", service }' "$1"
}
[ "$(built_services "$override")" = 'manager console litellm ' ] \
    || fail "Expected build: !reset null only on manager, console and litellm, found: $(built_services "$override")"

if grep -v -e '--platform linux/amd64' "$directory/crane.log" > /dev/null; then
    fail 'Expected every crane call to pass --platform linux/amd64'
fi
assert_crane_called "digest --platform linux/amd64 127.0.0.1:5000/zone/server:$version"
assert_crane_called "digest --platform linux/amd64 $traefik"
assert_crane_called "pull --platform linux/amd64 --format=tarball $helper $out/$top.partial/$top/images/registry.tar"
assert_crane_called "pull --platform linux/amd64 --format=oci 127.0.0.1:5000/zone/server@$(digest 1) $out/$top.partial/$top/images/zone-server"
assert_crane_called "pull --platform linux/amd64 --format=oci traefik@$(digest 4) $out/$top.partial/$top/images/traefik"
assert_crane_called "pull --platform linux/amd64 --format=oci ghcr.io/cloudnative-pg/postgresql@$(digest 6) $out/$top.partial/$top/images/cloudnative-postgresql"
[ "$(grep -c '^digest ' "$directory/crane.log")" -eq 6 ] || fail 'Expected one digest lookup per image'
[ "$(grep -c '^pull ' "$directory/crane.log")" -eq 7 ] || fail 'Expected one pull per image plus the registry helper'
first_pull=$(grep -n '^pull ' "$directory/crane.log" | head -n 1 | cut -d: -f1)
last_digest=$(grep -n '^digest ' "$directory/crane.log" | tail -n 1 | cut -d: -f1)
[ "$last_digest" -lt "$first_pull" ] || fail 'Expected every digest to resolve before anything is pulled'
cp "$bundle_root/SHA256SUMS" "$directory/primary-sums"

architecture=arm64
bundle_to arm64 || fail 'Expected an arm64 bundle to build'
architecture=amd64
if grep -v -e '--platform linux/arm64' "$directory/crane.log" > /dev/null; then
    fail 'Expected every crane call to pass --platform linux/arm64'
fi
arm64_archive=$out/zone-bundle-$version-arm64.tar
tar -xOf "$arm64_archive" "zone-bundle-$version-arm64/images.txt" > "$directory/arm64-images.txt"
grep -qF "zone-server${tab}zone/server${tab}$version${tab}$(digest 11)${tab}images/zone-server${tab}" "$directory/arm64-images.txt" \
    || fail 'Expected the arm64 platform digest in images.txt'
assert_crane_called "pull --platform linux/arm64 --format=oci traefik@$(digest 14) $out/zone-bundle-$version-arm64.partial/zone-bundle-$version-arm64/images/traefik"

digests_file=$directory/digests-missing
write_digests "$digests_file" "$ollama linux/amd64"
if bundle_to missing; then
    fail 'Expected a missing digest to fail the run'
fi
assert_output 'no linux/amd64 digest for:'
assert_output "ollama ($ollama)"
assert_nothing_pulled
assert_nothing_written

printf '%s linux/amd64 latest\n' "$traefik" > "$directory/digests-malformed"
grep -vF "$traefik linux/amd64" "$directory/digests" >> "$directory/digests-malformed"
digests_file=$directory/digests-malformed
if bundle_to malformed; then
    fail 'Expected a digest that is not sha256:<64 hex> to fail the run'
fi
assert_output "traefik ($traefik)"
assert_nothing_pulled
assert_nothing_written
digests_file=$directory/digests

failing_pull=ollama
if bundle_to failing; then
    fail 'Expected a failed pull to fail the run'
fi
failing_pull=''
assert_output "crane could not pull ollama/ollama@$(digest 5)"
assert_nothing_written

mkdir -p "$directory/runs/refused"
printf 'earlier bundle\n' > "$directory/runs/refused/$top.tar"
if bundle_to refused; then
    fail 'Expected an existing tar to be refused'
fi
assert_output "$top.tar already exists; refusing to overwrite it"
[ "$(cat "$directory/runs/refused/$top.tar")" = 'earlier bundle' ] || fail 'Expected the existing tar to be left alone'
[ ! -s "$directory/crane.log" ] || fail 'Expected no crane call when the output exists'
rm -f "$directory/runs/refused/$top.tar"

printf 'earlier checksum\n' > "$directory/runs/refused/$top.tar.sha256"
if bundle_to refused; then
    fail 'Expected an existing checksum to be refused'
fi
assert_output "$top.tar.sha256 already exists; refusing to overwrite it"
rm -f "$directory/runs/refused/$top.tar.sha256"

mkdir "$directory/runs/refused/$top.partial"
if bundle_to refused; then
    fail 'Expected a bundle already in progress to be refused'
fi
assert_output "$top.partial already exists; refusing to overwrite it"
[ -d "$directory/runs/refused/$top.partial" ] || fail 'Expected the other run to keep its work directory'
rmdir "$directory/runs/refused/$top.partial"

for architecture in 386 amd64x AMD64; do
    assert_refused "architecture $architecture" "architecture $architecture is not amd64 or arm64"
done
architecture=''
assert_refused 'an empty architecture' '--architecture is required'
architecture=amd64

for bad in 1.4.3 v1.4.3-lts 01.4.3-lts 1.04.3-lts 1.4.3-LTS 1.4.3-lts-rc 1.4-lts '1.4.3-lts
2.0.0-lts'; do
    if bundle --version "$bad" --architecture amd64 --registry 127.0.0.1:5000 \
        --crane "$directory/crane/crane" --output "$directory/runs/version"; then
        fail "Expected version $bad to be refused"
    fi
    assert_output 'is not X.Y.Z-lts'
    [ ! -s "$directory/crane.log" ] || fail "Expected no crane call for version $bad"
    [ ! -e "$directory/runs/version" ] || fail "Expected no output directory for version $bad"
done

assert_refused 'a registry with a scheme' 'registry https://registry.example is not HOST or HOST:PORT' \
    --registry https://registry.example
assert_refused 'an unknown argument' 'unknown argument: --insecure' --insecure
assert_refused 'a flag without a value' '--crane needs a value' --crane

if bundle --version "$version" --architecture amd64 --registry 127.0.0.1:5000 --crane "$directory/crane/crane"; then
    fail 'Expected a missing --output to be refused'
fi
assert_output '--output is required'

printf '#!/bin/sh\n' > "$directory/lonely-crane"
chmod +x "$directory/lonely-crane"
assert_refused 'a crane without its license' 'crane ships with its license' --crane "$directory/lonely-crane"

rm "$sandbox/scripts/install-offline.sh"
assert_refused 'a missing offline installer' 'scripts/install-offline.sh is missing; the bundle ships it as install.sh'
make_sandbox main

valid_server='zone-server zone:server zone/server compose:manager'
assert_manifest_refused 'leading whitespace' 'line 2: starts with whitespace' "$valid_server" " traefik $traefik library/traefik compose:traefik"
assert_manifest_refused 'an indented exclusion' 'line 2: starts with whitespace' "$valid_server" "${tab}#excluded comfyui needs a GPU"
assert_manifest_refused 'a duplicate name' 'line 2: name zone-server is listed twice' "$valid_server" 'zone-server zone:manager zone/manager compose:console'
assert_manifest_refused 'a duplicate source' 'line 2: source zone:server is listed twice' "$valid_server" 'zone-other zone:server zone/server compose:other'
assert_manifest_refused 'a duplicate consumer' 'line 2: consumer compose:manager is listed twice' "$valid_server" "traefik $traefik library/traefik compose:manager"
assert_manifest_refused 'three fields' 'line 1: does not have the four fields' 'zone-server zone:server zone/server'
assert_manifest_refused 'five fields' 'line 1: does not have the four fields' "$valid_server extra"
assert_manifest_refused 'an unknown Zone image' 'source zone:runner is not zone:server' 'zone-runner zone:runner zone/runner compose:runner'
assert_manifest_refused 'an unpinned source' 'source traefik:v3.7.12 is not' 'traefik traefik:v3.7.12 library/traefik compose:traefik'
assert_manifest_refused 'a source without a tag' 'is not <repository>:<tag>@sha256' 'traefik traefik@sha256:9c2a54d87f76f5c2f5f2682c68394af92fb12c0a2686798d6462a3f84bd78eaf library/traefik compose:traefik'
assert_manifest_refused 'an uppercase name' 'name Traefik does not match' "Traefik $traefik library/traefik compose:traefik"
assert_manifest_refused 'a bad target' 'target Library/Traefik does not match' "traefik $traefik Library/Traefik compose:traefik"
assert_manifest_refused 'an unknown consumer' 'consumers kustomize:traefik are not' "traefik $traefik library/traefik kustomize:traefik"
assert_manifest_refused 'a trailing comma' 'consumers compose:traefik, are not' "traefik $traefik library/traefik compose:traefik,"
assert_manifest_refused 'an exclusion without a reason' 'an exclusion is #excluded <compose service> <reason words>' "$valid_server" '#excluded comfyui'
assert_manifest_refused 'a run-together exclusion' 'an exclusion is #excluded' "$valid_server" '#excludedcomfyui needs a GPU'
assert_manifest_refused 'a twice-excluded service' 'line 3: excluded service comfyui is listed twice' "$valid_server" '#excluded comfyui needs a GPU' '#excluded comfyui again'
assert_manifest_refused 'no images' 'lists no images' '# nothing yet' '#excluded comfyui needs a GPU'
make_sandbox main

restricted() {
    rm -rf "$directory/restricted"
    mkdir "$directory/restricted"
    for tool in awk basename cat chmod cmp cp cut dirname env find grep ln ls mkdir mv perl rm sed sort tar tr uniq sh "$@"; do
        location=$(command -v "$tool" 2> /dev/null) || continue
        ln -s "$location" "$directory/restricted/$tool"
    done
    ln -s "$directory/bin/helm" "$directory/restricted/helm"
    ln -s "$directory/bin/git" "$directory/restricted/git"
}

if command -v sha256sum > /dev/null 2>&1 && command -v shasum > /dev/null 2>&1; then
    restricted shasum
    run_path=$directory/restricted
    bundle_to fallback || fail 'Expected shasum to stand in for sha256sum'
    run_path=''
    tar -xOf "$out/$top.tar" "$top/SHA256SUMS" > "$directory/fallback-sums"
    cmp -s "$directory/primary-sums" "$directory/fallback-sums" || fail 'Expected shasum -a 256 to write the same SHA256SUMS as sha256sum'
    (cd "$out" && sha256sum -c "$top.tar.sha256" > /dev/null) || fail 'Expected the shasum outer checksum to verify with sha256sum'
fi

restricted
run_path=$directory/restricted
if bundle_to unsummed; then
    fail 'Expected the bundler to refuse to run without sha256sum or shasum'
fi
run_path=''
assert_output 'neither sha256sum nor shasum is installed'
assert_nothing_written

if [ -f "$source_directory/bundle-images.txt" ]; then
    make_sandbox real "$source_directory/bundle-images.txt"
    digests_file=$directory/digests-real
    awk -v version="$version" '
        /^#/ || NF == 0 { next }
        {
            reference = $2
            if (reference ~ /^zone:/) {
                reference = "127.0.0.1:5000/zone/" substr(reference, 6) ":" version
            }
            count++
            printf "%s linux/amd64 sha256:%064d\n", reference, count
        }
    ' "$source_directory/bundle-images.txt" > "$digests_file"
    bundle_to real || fail 'Expected scripts/bundle-images.txt to build a bundle'
    entries=$(grep -cv -e '^#' -e '^[[:blank:]]*$' "$source_directory/bundle-images.txt")
    tar -xOf "$out/$top.tar" "$top/images.txt" > "$directory/real-images.txt"
    [ "$(wc -l < "$directory/real-images.txt" | tr -d ' ')" -eq "$entries" ] \
        || fail 'Expected one images.txt line per scripts/bundle-images.txt entry'
    awk -F '\t' 'NF != 6 { exit 1 }' "$directory/real-images.txt" \
        || fail 'Expected six tab-separated fields on every images.txt line'
    tar -xOf "$out/$top.tar" "$top/compose/docker-compose.airgap.yml" > "$directory/real-override.yml"
    [ "$(built_services "$directory/real-override.yml")" = 'manager console litellm ' ] \
        || fail "Expected build: !reset null only on manager, console and litellm in the real override, found: $(built_services "$directory/real-override.yml")"
    if grep -q -e '^  comfyui:' -e '^  comfyui-model-setup:' "$directory/real-override.yml"; then
        fail 'Expected excluded services to stay out of the override'
    fi
fi

printf '%s\n' 'Bundle assembler checks passed'
