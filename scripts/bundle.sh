#!/bin/sh
set -eu

helper='registry:3.1.1@sha256:325b4b29b041e82803abeb703e201655e4e23ab83264ec1a7c9ddb0a5b14a6e0'
airgap="\${ZONE_AIRGAP_REGISTRY:-127.0.0.1:5000}"
built=' manager console litellm '
tab=$(printf '\t')
blank=$(printf ' \t')
newline='
'

version_pattern='(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-lts'
registry_pattern='[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?(:[0-9]+)?'
digest_pattern='sha256:[0-9a-f]{64}'
name_pattern='[a-z0-9-]+'
zone_pattern='zone:(server|manager|litellm)'
component='[a-z0-9]+([._-]+[a-z0-9]+)*'
reference_pattern="([A-Za-z0-9.-]+(:[0-9]+)?/)?$component(/$component)*:[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}@$digest_pattern"
target_pattern='[a-z0-9._/-]+'
compose_consumer='compose:[a-zA-Z0-9][a-zA-Z0-9_.-]*'
helm_consumer='helm-(split|string):[a-z0-9-]+:[A-Za-z0-9_-]+(\.[A-Za-z0-9_-]+)*'
consumer_pattern="($compose_consumer|$helm_consumer)"
consumers_pattern="$consumer_pattern(,$consumer_pattern)*"
exclusion_pattern='#excluded[[:blank:]]+[a-zA-Z0-9][a-zA-Z0-9_.-]*[[:blank:]]+[^[:space:]].*'

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
manifest=$root/scripts/bundle-images.txt
installer=$root/scripts/install-offline.sh

version=''
architecture=''
registry=''
crane=''
output=''
work=''

usage() {
    printf '%s\n' 'Usage: scripts/bundle.sh --version X.Y.Z-lts --architecture amd64|arm64 --registry HOST[:PORT] --crane PATH --output DIRECTORY' >&2
    exit 2
}

fail() {
    printf 'bundle: %s\n' "$1" >&2
    exit 1
}

cleanup() {
    if [ -n "$work" ]; then
        rm -rf -- "$work"
    fi
}

matches() {
    case $1 in
        *"$newline"*) return 1 ;;
    esac
    printf '%s\n' "$1" | grep -Eqx -- "$2"
}

assign() {
    case $1 in
        --version) version=$2 ;;
        --architecture) architecture=$2 ;;
        --registry) registry=$2 ;;
        --crane) crane=$2 ;;
        --output) output=$2 ;;
    esac
}

parse_arguments() {
    while [ $# -gt 0 ]; do
        case $1 in
            --version=* | --architecture=* | --registry=* | --crane=* | --output=*)
                assign "${1%%=*}" "${1#*=}"
                shift
                ;;
            --version | --architecture | --registry | --crane | --output)
                [ $# -ge 2 ] || fail "$1 needs a value"
                assign "$1" "$2"
                shift 2
                ;;
            -h | --help) usage ;;
            *) fail "unknown argument: $1" ;;
        esac
    done
}

validate() {
    for option in version architecture registry crane output; do
        eval "value=\$$option"
        [ -n "$value" ] || fail "--$option is required"
    done
    matches "$version" "$version_pattern" || fail "version $version is not X.Y.Z-lts"
    case $architecture in
        amd64 | arm64) ;;
        *) fail "architecture $architecture is not amd64 or arm64" ;;
    esac
    matches "$registry" "$registry_pattern" || fail "registry $registry is not HOST or HOST:PORT"
    if [ ! -f "$crane" ] || [ ! -x "$crane" ]; then
        fail "crane $crane is not an executable file"
    fi
    crane=$(CDPATH='' cd -- "$(dirname -- "$crane")" && pwd)/$(basename -- "$crane")
    license=$(dirname -- "$crane")/LICENSE
    [ -f "$license" ] || fail "crane ships with its license, but $license is missing"
    [ -f "$manifest" ] || fail 'scripts/bundle-images.txt is missing'
    [ -f "$installer" ] || fail 'scripts/install-offline.sh is missing; the bundle ships it as install.sh'
    for tool in git helm tar; do
        command -v "$tool" > /dev/null 2>&1 || fail "$tool is not installed"
    done
}

choose_checksum() {
    if command -v sha256sum > /dev/null 2>&1; then
        checksum() {
            sha256sum -- "$@"
        }
    elif command -v shasum > /dev/null 2>&1; then
        checksum() {
            shasum -a 256 -- "$@"
        }
    else
        fail 'neither sha256sum nor shasum is installed'
    fi
}

prepare_output() {
    top=zone-bundle-$version-$architecture
    mkdir -p -- "$output"
    output=$(CDPATH='' cd -- "$output" && pwd)
    archive=$output/$top.tar
    for existing in "$archive" "$archive.sha256" "$output/$top.partial"; do
        if [ -e "$existing" ] || [ -L "$existing" ]; then
            fail "$existing already exists; refusing to overwrite it"
        fi
    done
    mkdir -- "$output/$top.partial" || fail "$output/$top.partial already exists; refusing to overwrite it"
    work=$output/$top.partial
    staging=$work/$top
    tarball=$work/$top.tar
    checksums=$work/SHA256SUMS
    mkdir -- "$staging"
    : > "$checksums"
}

problem() {
    printf 'line %s: %s\n' "$number" "$1" >> "$work/problems"
}

listed() {
    grep -Fxq -- "$1" "$work/$2"
}

claim() {
    if listed "$1" "$2"; then
        problem "$3 $1 is listed twice"
    fi
    printf '%s\n' "$1" >> "$work/$2"
}

check_exclusion() {
    if ! matches "$1" "$exclusion_pattern"; then
        problem 'an exclusion is #excluded <compose service> <reason words>'
        return
    fi
    service=$(printf '%s\n' "$1" | sed 's/^#excluded[[:blank:]]*//; s/[[:blank:]].*$//')
    claim "$service" exclusions 'excluded service'
}

check_entry() {
    if matches "$1" "$name_pattern"; then
        claim "$1" names name
    else
        problem "name $1 does not match $name_pattern"
    fi
    claim "$2" sources source
    case $2 in
        zone:*)
            matches "$2" "$zone_pattern" || problem "source $2 is not zone:server, zone:manager or zone:litellm"
            ;;
        *)
            matches "$2" "$reference_pattern" || problem "source $2 is not <repository>:<tag>@sha256:<64 hex>"
            ;;
    esac
    matches "$3" "$target_pattern" || problem "target $3 does not match $target_pattern"
    if ! matches "$4" "$consumers_pattern"; then
        problem "consumers $4 are not a comma-joined list of compose:<service>, helm-split:<chart>:<path> or helm-string:<chart>:<path>"
        return
    fi
    items=$(printf '%s' "$4" | tr ',' '\n')
    while IFS= read -r consumer; do
        claim "$consumer" consumers consumer
    done <<ITEMS
$items
ITEMS
    printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" >> "$work/entries"
}

read_manifest() {
    for list in entries problems names sources consumers exclusions; do
        : > "$work/$list"
    done
    number=0
    while IFS= read -r line || [ -n "$line" ]; do
        number=$((number + 1))
        IFS=$blank read -r name source target consumers extra <<LINE
$line
LINE
        [ -n "$name" ] || continue
        case $line in
            [[:blank:]]*) problem 'starts with whitespace' ;;
            '#excluded'*) check_exclusion "$line" ;;
            '#'*) ;;
            *)
                if [ -z "$consumers" ] || [ -n "$extra" ]; then
                    problem 'does not have the four fields <name> <source> <target> <consumers>'
                else
                    check_entry "$name" "$source" "$target" "$consumers"
                fi
                ;;
        esac
    done < "$manifest"
    if [ -s "$work/problems" ]; then
        printf 'bundle: scripts/bundle-images.txt is invalid:\n' >&2
        sed 's/^/  /' "$work/problems" >&2
        exit 1
    fi
    [ -s "$work/entries" ] || fail 'scripts/bundle-images.txt lists no images'
}

resolve_digests() {
    printf 'Resolving linux/%s digests\n' "$architecture"
    : > "$work/images.txt"
    : > "$work/pulls"
    : > "$work/unresolved"
    while IFS=$tab read -r name source target consumers; do
        case $source in
            zone:*)
                repository=$registry/zone/${source#zone:}
                tag=$version
                reference=$repository:$tag
                ;;
            *)
                reference=$source
                repository=${source%@*}
                tag=${repository##*:}
                repository=${repository%:*}
                ;;
        esac
        digest=$("$crane" digest --platform "linux/$architecture" "$reference" < /dev/null) || digest=''
        if ! matches "$digest" "$digest_pattern"; then
            printf '  %s (%s)\n' "$name" "$reference" >> "$work/unresolved"
            continue
        fi
        printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$target" "$tag" "$digest" "images/$name" "$consumers" >> "$work/images.txt"
        printf '%s\t%s\n' "$name" "$repository@$digest" >> "$work/pulls"
    done < "$work/entries"
    if [ -s "$work/unresolved" ]; then
        printf 'bundle: no linux/%s digest for:\n' "$architecture" >&2
        cat "$work/unresolved" >&2
        exit 1
    fi
}

write_override() {
    printf 'services:\n'
    while IFS=$tab read -r name target tag digest _ consumers; do
        items=$(printf '%s' "$consumers" | tr ',' '\n')
        while IFS= read -r consumer; do
            case $consumer in
                compose:*) ;;
                *) continue ;;
            esac
            service=${consumer#compose:}
            printf '  %s:\n' "$service"
            printf '    image: %s/%s@%s\n' "$airgap" "$target" "$digest"
            case $built in
                *" $service "*) printf '    build: !reset null\n' ;;
            esac
            printf '    pull_policy: missing\n'
        done <<ITEMS
$items
ITEMS
    done < "$work/images.txt"
}

append() {
    printf 'Appending %s\n' "$1"
    (
        cd -- "$staging"
        find "$1" -type f | LC_ALL=C sort | while IFS= read -r file; do
            checksum "$file"
        done
    ) >> "$checksums"
    if [ -f "$tarball" ]; then
        tar -rf "$tarball" -C "$work" "$top/$1"
    else
        tar -cf "$tarball" -C "$work" "$top/$1"
    fi
    rm -rf -- "${staging:?}/$1"
}

package_charts() {
    mkdir -- "$staging/helm"
    charts=0
    for chart in "$root"/helm/*/Chart.yaml; do
        [ -f "$chart" ] || fail 'no helm/*/Chart.yaml to package'
        helm package "$(dirname -- "$chart")" --destination "$staging/helm" < /dev/null
        charts=$((charts + 1))
    done
    packages=0
    for package in "$staging"/helm/*.tgz; do
        [ -f "$package" ] || continue
        packages=$((packages + 1))
    done
    [ "$packages" -eq "$charts" ] || fail "helm packaged $packages archives from $charts charts"
    for package in "$staging"/helm/*.tgz; do
        append "helm/$(basename -- "$package")"
    done
}

pull_images() {
    while IFS=$tab read -r name reference; do
        "$crane" pull --platform "linux/$architecture" --format=oci "$reference" "$staging/images/$name" < /dev/null \
            || fail "crane could not pull $reference"
        append "images/$name"
    done < "$work/pulls"
}

assemble() {
    printf '%s\n' "$version" > "$staging/VERSION"
    append VERSION

    cp -- "$installer" "$staging/install.sh"
    chmod 0755 "$staging/install.sh"
    append install.sh

    mkdir -- "$staging/bin"
    cp -- "$crane" "$staging/bin/crane"
    chmod 0755 "$staging/bin/crane"
    append bin/crane

    mkdir -- "$staging/LICENSES"
    cp -- "$license" "$staging/LICENSES/crane"
    append LICENSES/crane

    git -C "$root" archive --format=tar -o "$staging/source.tar" HEAD < /dev/null
    append source.tar

    mkdir -- "$staging/compose"
    write_override > "$staging/compose/docker-compose.airgap.yml"
    append compose/docker-compose.airgap.yml

    package_charts

    mkdir -- "$staging/images"
    "$crane" pull --platform "linux/$architecture" --format=tarball "$helper" "$staging/images/registry.tar" < /dev/null \
        || fail "crane could not pull $helper"
    append images/registry.tar

    pull_images

    cp -- "$work/images.txt" "$staging/images.txt"
    append images.txt

    cp -- "$checksums" "$staging/SHA256SUMS"
    tar -rf "$tarball" -C "$work" "$top/SHA256SUMS"
}

publish() {
    ln -- "$tarball" "$archive" || fail "$archive appeared while bundling; refusing to overwrite it"
    (cd -- "$output" && checksum "$top.tar") > "$work/$top.tar.sha256"
    ln -- "$work/$top.tar.sha256" "$archive.sha256" || fail "$archive.sha256 appeared while bundling; refusing to overwrite it"
    printf 'Wrote %s and %s\n' "$archive" "$archive.sha256"
}

trap cleanup EXIT
trap 'exit 1' HUP INT TERM

parse_arguments "$@"
validate
choose_checksum
prepare_output
read_manifest
resolve_digests
assemble
publish
