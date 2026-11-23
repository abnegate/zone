#!/bin/sh
set -eu

bundle=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
crane=$bundle/bin/crane
tab=$(printf '\t')
override=docker-compose.airgap.yml
minimum_compose_major=2
minimum_compose_minor=24
default_port=5000
container_port=5000
readiness_attempts=30

scratch=''
registry_container=''
registry_volume=''

usage() {
    cat <<'USAGE'
Usage: install.sh <command> [options]

Run it from the extracted bundle directory.

Commands:
  verify
      Check every file in the bundle against SHA256SUMS.
  compose [--port 5000] [--directory DIR]
      Load the images into the local Docker engine through a temporary
      registry on 127.0.0.1:<port>, extract the Zone source into DIR
      (default ./zone), keep its .env and add the air-gapped settings.
  push --registry HOST[/PREFIX] [--insecure]
      Push every image to your registry, keeping its digest.
  helm-values --registry HOST[/PREFIX] --output DIR
      Write zone-apps.yaml, zone-infra.yaml and zone-ai.yaml for images
      pushed to that registry.

Every command runs verify first.
USAGE
}

fail() {
    printf 'install.sh: %s\n' "$1" >&2
    exit 1
}

refuse_usage() {
    printf 'install.sh: %s\n\n' "$1" >&2
    usage >&2
    exit 2
}

cleanup() {
    if [ -n "$registry_container" ]; then
        docker rm --force --volumes "$registry_container" >/dev/null 2>&1 || true
        registry_container=''
    fi
    if [ -n "$registry_volume" ]; then
        docker volume rm --force "$registry_volume" >/dev/null 2>&1 || true
        registry_volume=''
    fi
    if [ -n "$scratch" ]; then
        rm -rf "$scratch"
        scratch=''
    fi
}

trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

checksum() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$@"
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$@"
    else
        printf '%s\n' 'Neither sha256sum nor shasum is installed' >&2
        return 1
    fi
}

checksum_paths() {
    awk '
        {
            hash = substr($0, 1, 64)
            separator = substr($0, 65, 2)
            path = substr($0, 67)
            if (length(hash) != 64 || hash !~ /^[0-9a-f]+$/ || (separator != "  " && separator != " *") || path == "") {
                printf "SHA256SUMS line %d is not a sha256sum line\n", NR > "/dev/stderr"
                exit 1
            }
            sub(/^\.\//, "", path)
            if (path ~ /^\// || ("/" path "/") ~ /\/\.\.\//) {
                printf "SHA256SUMS line %d names a path outside the bundle: %s\n", NR, path > "/dev/stderr"
                exit 1
            }
            print path
        }
    ' "$bundle/SHA256SUMS"
}

verify() {
    [ -f "$bundle/SHA256SUMS" ] || fail "SHA256SUMS is missing from $bundle"
    paths=$(checksum_paths) || fail 'SHA256SUMS is malformed. Download the bundle again.'
    [ -n "$paths" ] || fail 'SHA256SUMS lists no files. Download the bundle again.'
    if ! report=$(cd "$bundle" && checksum -c SHA256SUMS 2>&1); then
        printf '%s\n' "$report" | grep -v ': OK$' >&2 || true
        fail 'The bundle does not match SHA256SUMS. Download it again and check its signature.'
    fi
    require_listed images.txt
    require_listed install.sh
}

require_listed() {
    printf '%s\n' "$paths" | grep -qxF -- "$1" || fail "SHA256SUMS does not cover $1"
}

read_images() {
    awk -F '\t' '
        function refuse(message) {
            printf "images.txt line %d: %s\n", NR, message > "/dev/stderr"
            failed = 1
            exit 1
        }
        function consumer_valid(item,    fields, count, segments, index_) {
            count = split(item, fields, ":")
            if (fields[1] == "compose") return count == 2 && fields[2] ~ /^[a-z0-9][a-z0-9_.-]*$/
            if (fields[1] != "helm-split" && fields[1] != "helm-string") return 0
            if (count != 3 || fields[2] !~ /^[a-z0-9-]+$/ || fields[3] == "") return 0
            count = split(fields[3], segments, ".")
            for (index_ = 1; index_ <= count; index_++) {
                if (segments[index_] !~ /^[A-Za-z0-9_-]+$/) return 0
            }
            return 1
        }
        {
            if (NF != 6) refuse("expected 6 TAB-separated fields, found " NF)
            if ($1 !~ /^[a-z0-9-]+$/) refuse("the name " $1 " is not [a-z0-9-]+")
            if ($1 in seen) refuse("the name " $1 " is listed twice")
            seen[$1] = 1
            if ($2 !~ /^[a-z0-9._\/-]+$/ || $2 ~ /^\// || $2 ~ /\/$/ || $2 ~ /\/\//) refuse("the target " $2 " is not a repository path")
            if ($3 !~ /^[A-Za-z0-9_][A-Za-z0-9_.-]*$/ || length($3) > 128) refuse("the tag " $3 " is not a tag")
            digest = substr($4, 8)
            if (substr($4, 1, 7) != "sha256:" || length(digest) != 64 || digest !~ /^[0-9a-f]+$/) refuse("the digest " $4 " is not sha256:<64 hex>")
            if ($5 != "images/" $1) refuse("the layout " $5 " is not images/" $1)
            count = split($6, items, ",")
            if (count == 0) refuse("no consumers")
            for (item = 1; item <= count; item++) {
                if (!consumer_valid(items[item])) refuse("the consumer " items[item] " does not follow the grammar")
            }
            print
        }
        END {
            if (failed) exit 1
            if (NR == 0) {
                print "images.txt lists no images" > "/dev/stderr"
                exit 1
            }
        }
    ' "$bundle/images.txt" > "$scratch/images" || fail 'images.txt is malformed. Download the bundle again.'
    cut -f 5 "$scratch/images" > "$scratch/layouts"
    while IFS= read -r layout; do
        require_listed "$layout/index.json"
    done < "$scratch/layouts"
}

normalized_registry() {
    value=$1
    while :; do
        case "$value" in
            */) value=${value%/} ;;
            *) break ;;
        esac
    done
    if ! printf '%s\n' "$value" | grep -Eq '^[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?(:[0-9]+)?(/[a-z0-9]([a-z0-9._-]*[a-z0-9])?)*$'; then
        fail "--registry must be HOST[:PORT][/PREFIX] with no scheme, such as registry.example.com/zone: $1"
    fi
    printf '%s\n' "$value"
}

valid_port() {
    case "$1" in
        ''|*[!0-9]*) return 1 ;;
    esac
    [ "$1" -ge 1 ] && [ "$1" -le 65535 ]
}

crane_push() {
    insecure=$1
    layout=$2
    reference=$3
    if [ "$insecure" -eq 1 ]; then
        "$crane" push --insecure "$bundle/$layout" "$reference"
    else
        "$crane" push "$bundle/$layout" "$reference"
    fi
}

push_images() {
    registry=$1
    insecure=$2
    require_listed bin/crane
    [ -x "$crane" ] || fail "$crane is not executable"
    while IFS="$tab" read -r name target tag digest layout _; do
        for reference in "$registry/$target@$digest" "$registry/$target:$tag"; do
            output=$(crane_push "$insecure" "$layout" "$reference") || fail "crane could not push $name to $reference"
            pushed=$(printf '%s\n' "$output" | tail -n 1)
            pushed=${pushed##*@}
            [ "$pushed" = "$digest" ] || fail "crane pushed $name as $pushed, but images.txt pins $digest"
        done
        printf 'Pushed %s as %s/%s:%s@%s\n' "$name" "$registry" "$target" "$tag" "$digest"
    done < "$scratch/images"
}

compose_services() {
    awk -F '\t' '
        {
            count = split($6, items, ",")
            for (item = 1; item <= count; item++) {
                if (items[item] ~ /^compose:/) print substr(items[item], 9)
            }
        }
    ' "$scratch/images"
}

require_compose() {
    version=$(docker compose version --short 2>/dev/null) || fail "Docker Compose is not installed. This bundle needs Compose $minimum_compose_major.$minimum_compose_minor or later."
    version=${version#v}
    major=${version%%.*}
    rest=${version#*.}
    minor=${rest%%.*}
    case "$major.$minor" in
        *[!0-9.]*|.*|*.) fail "Could not read the Docker Compose version: $version" ;;
    esac
    if [ "$major" -lt "$minimum_compose_major" ] || { [ "$major" -eq "$minimum_compose_major" ] && [ "$minor" -lt "$minimum_compose_minor" ]; }; then
        fail "Docker Compose $version is too old. This bundle needs $minimum_compose_major.$minimum_compose_minor or later, for the !reset tag in $override."
    fi
}

extract_source() {
    target=$1
    if [ -f "$target/.env" ]; then
        cp -p "$target/.env" "$scratch/env"
    fi
    tar -xf "$bundle/source.tar" -C "$target" || fail "Could not extract source.tar into $target"
    if [ -f "$scratch/env" ] && ! cmp -s "$scratch/env" "$target/.env"; then
        rm -f "$target/.env"
        cp -p "$scratch/env" "$target/.env"
    fi
}

upsert_env() {
    file=$1
    name=$2
    value=$3
    awk -v name="$name" -v value="$value" '
        $0 ~ "^[[:space:]]*(export[[:space:]]+)?" name "[[:space:]]*=" {
            if (!written) print name "=" value
            written = 1
            next
        }
        { print }
        END {
            if (!written) print name "=" value
        }
    ' "$file" > "$scratch/env.next"
    cat "$scratch/env.next" > "$file"
}

add_env_default() {
    file=$1
    name=$2
    value=$3
    if [ -n "$(read_env_value "$name" "$file")" ]; then
        printf 'Kept %s from %s\n' "$name" "$file"
        return 0
    fi
    upsert_env "$file" "$name" "$value"
    printf 'Added %s=%s to %s\n' "$name" "$value" "$file"
}

start_registry() {
    port=$1
    require_listed images/registry.tar
    loaded=$(docker load --input "$bundle/images/registry.tar") || fail 'docker load could not load images/registry.tar'
    image=$(printf '%s\n' "$loaded" | sed -n 's/^Loaded image\( ID\)\{0,1\}: //p' | tail -n 1)
    [ -n "$image" ] || fail "docker load did not name the image it loaded from images/registry.tar"
    name=zone-airgap-registry-$$
    registry_volume=$name
    docker volume create "$name" >/dev/null || fail "Could not create the temporary volume $name"
    registry_container=$name
    docker run --detach --name "$name" \
        --publish "127.0.0.1:$port:$container_port" \
        --volume "$name:/var/lib/registry" \
        "$image" >/dev/null || fail "Could not start the temporary registry on 127.0.0.1:$port. Is the port free? Choose another with --port."
    attempt=1
    until "$crane" catalog --insecure "127.0.0.1:$port" >/dev/null 2>&1; do
        [ "$attempt" -lt "$readiness_attempts" ] || fail "The temporary registry on 127.0.0.1:$port did not answer"
        attempt=$((attempt + 1))
        sleep 1
    done
    printf 'Started the temporary registry %s on 127.0.0.1:%s\n' "$name" "$port"
}

pull_images() {
    directory=$1
    port=$2
    profiles=$(cd "$directory" && docker compose -f docker-compose.yml -f "$override" config --profiles) \
        || fail "docker compose could not read the configuration in $directory"
    set -- -f docker-compose.yml -f "$override"
    for profile in $profiles; do
        set -- "$@" --profile "$profile"
    done
    set -- "$@" pull
    for service in $(compose_services); do
        set -- "$@" "$service"
    done
    (cd "$directory" && ZONE_AIRGAP_REGISTRY="127.0.0.1:$port" docker compose "$@") \
        || fail 'docker compose pull failed'
}

command_verify() {
    [ $# -eq 0 ] || refuse_usage "verify takes no options: $1"
    verify
    printf 'Verified %s files against SHA256SUMS\n' "$(printf '%s\n' "$paths" | wc -l | tr -d ' ')"
}

command_compose() {
    port=$default_port
    port_given=0
    directory=$PWD/zone
    while [ $# -gt 0 ]; do
        case "$1" in
            --port)
                [ $# -ge 2 ] || refuse_usage '--port needs a value'
                port=$2
                port_given=1
                shift 2
                ;;
            --port=*)
                port=${1#*=}
                port_given=1
                shift
                ;;
            --directory)
                [ $# -ge 2 ] || refuse_usage '--directory needs a value'
                directory=$2
                shift 2
                ;;
            --directory=*)
                directory=${1#*=}
                shift
                ;;
            *) refuse_usage "Unknown compose option: $1" ;;
        esac
    done
    valid_port "$port" || fail "--port must be a number from 1 to 65535: $port"
    [ -n "$directory" ] || fail '--directory must not be empty'

    verify
    read_images
    require_listed source.tar
    require_listed "compose/$override"
    require_compose

    mkdir -p "$directory" || fail "Could not create $directory"
    directory=$(CDPATH='' cd -- "$directory" && pwd)
    extract_source "$directory"
    environment=$directory/.env
    if [ ! -f "$environment" ]; then
        fail "$environment does not exist yet. Create it with 'cd $directory && ./scripts/setup.sh --skip-models', then run install.sh compose again."
    fi
    [ -f "$directory/scripts/env-file.sh" ] || fail "source.tar has no scripts/env-file.sh"
    # shellcheck source=/dev/null
    . "$directory/scripts/env-file.sh"

    configured=$(read_env_value ZONE_AIRGAP_REGISTRY "$environment")
    case "$configured" in
        '') ;;
        127.0.0.1:*)
            configured_port=${configured#127.0.0.1:}
            valid_port "$configured_port" || fail "ZONE_AIRGAP_REGISTRY in $environment is not 127.0.0.1:<port>: $configured"
            if [ "$port_given" -eq 1 ] && [ "$configured_port" != "$port" ]; then
                fail "ZONE_AIRGAP_REGISTRY in $environment is $configured. Run with --port $configured_port, or change it to 127.0.0.1:$port."
            fi
            port=$configured_port
            ;;
        *)
            fail "ZONE_AIRGAP_REGISTRY in $environment is $configured, but compose loads images through 127.0.0.1. Fill that registry with 'install.sh push --registry $configured' instead, or remove the setting."
            ;;
    esac

    start_registry "$port"
    push_images "127.0.0.1:$port" 1
    cp "$bundle/compose/$override" "$directory/$override"
    add_env_default "$environment" ZONE_AIRGAP_REGISTRY "127.0.0.1:$port"
    add_env_default "$environment" ZONE_LICENSE_RENEWAL false
    pull_images "$directory" "$port"
    cleanup
    printf 'Removed the temporary registry\n\n'

    version=$(cat "$bundle/VERSION" 2>/dev/null || printf 'unknown')
    cat <<NEXT
Zone $version is loaded. Start it from $directory with one of:

  make up
  ./scripts/compose.sh up -d
  docker compose -f docker-compose.yml -f $override up -d
NEXT
}

command_push() {
    registry=''
    insecure=0
    while [ $# -gt 0 ]; do
        case "$1" in
            --registry)
                [ $# -ge 2 ] || refuse_usage '--registry needs a value'
                registry=$2
                shift 2
                ;;
            --registry=*)
                registry=${1#*=}
                shift
                ;;
            --insecure)
                insecure=1
                shift
                ;;
            *) refuse_usage "Unknown push option: $1" ;;
        esac
    done
    [ -n "$registry" ] || refuse_usage 'push needs --registry HOST[/PREFIX]'
    registry=$(normalized_registry "$registry")

    verify
    read_images
    push_images "$registry" "$insecure"
    printf '\nEvery image is in %s under its original digest.\n' "$registry"
}

helm_leaves() {
    awk -F '\t' -v registry="$1" '
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
    ' "$scratch/images"
}

helm_yaml() {
    awk -F '\t' -v chart="$1" '
        function refuse(message) {
            printf "%s: %s\n", chart, message > "/dev/stderr"
            failed = 1
            exit 1
        }
        function emit(node, depth,    list, count, position, child, indent, level) {
            indent = ""
            for (level = 0; level < depth; level++) indent = indent "  "
            count = split(children[node], list, SUBSEP)
            for (position = 2; position <= count; position++) {
                child = list[position]
                if (child in value) {
                    print indent label[child] ": \"" value[child] "\""
                } else {
                    print indent label[child] ":"
                    emit(child, depth + 1)
                }
            }
        }
        $1 == chart {
            count = split($2, segments, ".")
            parent = ""
            for (position = 1; position <= count; position++) {
                node = parent == "" ? segments[position] : parent "." segments[position]
                if (parent in value) refuse(parent " is set as a value and as a map")
                if (!(node in label)) {
                    label[node] = segments[position]
                    children[parent] = children[parent] SUBSEP node
                }
                parent = node
            }
            if (children[parent] != "") refuse(parent " is set as a value and as a map")
            if ((parent in value) && value[parent] != $3) refuse(parent " is set to two different values")
            value[parent] = $3
            found = 1
        }
        END {
            if (failed) exit 1
            if (!found) print "{}"
            else emit("", 0)
        }
    ' "$scratch/leaves"
}

command_helm_values() {
    registry=''
    output=''
    while [ $# -gt 0 ]; do
        case "$1" in
            --registry)
                [ $# -ge 2 ] || refuse_usage '--registry needs a value'
                registry=$2
                shift 2
                ;;
            --registry=*)
                registry=${1#*=}
                shift
                ;;
            --output)
                [ $# -ge 2 ] || refuse_usage '--output needs a value'
                output=$2
                shift 2
                ;;
            --output=*)
                output=${1#*=}
                shift
                ;;
            *) refuse_usage "Unknown helm-values option: $1" ;;
        esac
    done
    [ -n "$registry" ] || refuse_usage 'helm-values needs --registry HOST[/PREFIX]'
    [ -n "$output" ] || refuse_usage 'helm-values needs --output DIR'
    registry=$(normalized_registry "$registry")

    verify
    read_images
    helm_leaves "$registry" > "$scratch/leaves"
    mkdir -p "$output" || fail "Could not create $output"
    cut -f 1 "$scratch/leaves" | awk '!seen[$0]++' > "$scratch/charts"
    while IFS= read -r chart; do
        helm_yaml "$chart" > "$scratch/values.yaml" || fail "Could not write the values for $chart"
        cp "$scratch/values.yaml" "$output/$chart.yaml"
        printf 'Wrote %s\n' "$output/$chart.yaml"
    done < "$scratch/charts"
}

[ $# -gt 0 ] || refuse_usage 'Name a command.'
command=$1
shift
case "$command" in
    help|-h|--help)
        usage
        exit 0
        ;;
esac
scratch=$(mktemp -d)
case "$command" in
    verify) command_verify "$@" ;;
    compose) command_compose "$@" ;;
    push) command_push "$@" ;;
    helm-values) command_helm_values "$@" ;;
    *) refuse_usage "Unknown command: $command" ;;
esac
