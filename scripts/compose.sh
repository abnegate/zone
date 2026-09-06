#!/bin/sh
# Profile-aware Compose driver.
#
# Optional services are Compose profiles. Config variants that cannot be
# profile-gated (hot reload, Gluetun network_mode) live in overlay files that
# this script selects from the active profile list.
#
#   ./scripts/compose.sh --profile dev --profile vpn --profile monitoring up
#   make up PROFILES=dev,vpn,monitoring
#
# Profiles:
#   dev                     docker-compose.dev.yml (hot reload)
#   vpn                     docker-compose.vpn.yml (full tunnel)
#   monitoring              Prometheus / Grafana (in docker-compose.yml)
#   bundled-ollama          bundled Ollama engine
#   bundled-comfyui         bundled NVIDIA ComfyUI
#   comfyui-model-setup     one-shot model download
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

ZONE_ENV_FILE=${ZONE_ENV_FILE:-$root/.env}
KNOWN_PROFILES='dev vpn monitoring bundled-ollama bundled-comfyui comfyui-model-setup'
ALL_OVERLAY_PROFILES='dev,vpn,monitoring,bundled-ollama,bundled-comfyui,comfyui-model-setup'

is_on() {
    value=$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')
    case "$value" in
        1|true|yes|on) return 0 ;;
        *) return 1 ;;
    esac
}

read_env_value() {
    name=$1
    file=$2
    if [ ! -f "$file" ]; then
        return 0
    fi
    awk -F= -v name="$name" '
        $0 ~ "^[[:space:]]*(export[[:space:]]+)?" name "[[:space:]]*=" {
            value = $0
            sub(/^[[:space:]]*(export[[:space:]]+)?[^=]+=/, "", value)
            gsub(/^[[:space:]]+|[[:space:]]+$/, "", value)
            gsub(/^["'\'']|["'\'']$/, "", value)
            found = value
        }
        END { if (found != "") print found }
    ' "$file"
}

zone_vpn_enabled() {
    value=${ZONE_VPN:-}
    if [ -z "$value" ]; then
        value=$(read_env_value ZONE_VPN "$ZONE_ENV_FILE")
    fi
    is_on "$value"
}

tokenize_profiles() {
    printf '%s' "$1" | tr ', ' '\n' | tr -d '\r' | sed '/^$/d'
}

has_profile() {
    needle=$1
    haystack=$2
    tokenize_profiles "$haystack" | grep -qx "$needle" || return 1
}

normalize_profiles() {
    requested=$1
    result=''
    for known in $KNOWN_PROFILES; do
        if has_profile "$known" "$requested"; then
            result=${result:+$result,}$known
        fi
    done
    for profile in $(tokenize_profiles "$requested"); do
        known_match=0
        for known in $KNOWN_PROFILES; do
            if [ "$profile" = "$known" ]; then
                known_match=1
                break
            fi
        done
        if [ "$known_match" -eq 0 ]; then
            result=${result:+$result,}$profile
        fi
    done
    printf '%s\n' "$result"
}

ensure_profile() {
    extra=$1
    list=$2
    if [ -z "$extra" ]; then
        printf '%s\n' "$list"
        return
    fi
    if has_profile "$extra" "$list"; then
        normalize_profiles "$list"
    else
        normalize_profiles "$extra${list:+,$list}"
    fi
}

persisted_profiles() {
    value=${COMPOSE_PROFILES:-}
    if [ -z "$value" ]; then
        value=$(read_env_value COMPOSE_PROFILES "$ZONE_ENV_FILE")
    fi
    if [ -z "$value" ] && zone_vpn_enabled; then
        value=vpn
    elif [ -n "$value" ] && ! has_profile vpn "$value" && zone_vpn_enabled; then
        value=$value,vpn
    fi
    normalize_profiles "$value"
}

compose_files() {
    profiles=$1
    printf '%s\n' docker-compose.yml
    if has_profile dev "$profiles"; then
        printf '%s\n' docker-compose.dev.yml
    fi
    if has_profile vpn "$profiles"; then
        printf '%s\n' docker-compose.vpn.yml
    fi
    if [ -f docker-compose.override.yml ]; then
        printf '%s\n' docker-compose.override.yml
    fi
}

compose_file_env() {
    profiles=$1
    separator=${COMPOSE_PATH_SEPARATOR:-:}
    files=''
    for file in $(compose_files "$profiles"); do
        files=${files:+$files$separator}$file
    done
    printf '%s\n' "$files"
}

print_flags() {
    profiles=$(normalize_profiles "$1")
    for file in $(compose_files "$profiles"); do
        printf -- '-f %s ' "$file"
    done
    if [ -n "$profiles" ]; then
        old_ifs=$IFS
        IFS=,
        # Intentional split on commas.
        for profile in $profiles; do
            printf -- '--profile %s ' "$profile"
        done
        IFS=$old_ifs
    fi
    printf '\n'
}

upsert_env() {
    file=$1
    name=$2
    value=$3
    temporary=$(mktemp "${file}.XXXXXX")
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
    ' "$file" > "$temporary"
    mv "$temporary" "$file"
}

delete_env() {
    file=$1
    name=$2
    temporary=$(mktemp "${file}.XXXXXX")
    awk -v name="$name" '
        $0 ~ "^[[:space:]]*(export[[:space:]]+)?" name "[[:space:]]*=" { next }
        { print }
    ' "$file" > "$temporary"
    mv "$temporary" "$file"
}

persist_profiles() {
    list=$1
    if [ ! -f "$ZONE_ENV_FILE" ]; then
        printf '%s\n' 'Missing .env file. Run make setup first.' >&2
        exit 1
    fi
    profiles=$(normalize_profiles "$list")
    if has_profile vpn "$profiles"; then
        sh "$root/scripts/configure-model-proxy.sh" "$ZONE_ENV_FILE" vpn
    else
        sh "$root/scripts/configure-model-proxy.sh" "$ZONE_ENV_FILE" direct
    fi
    upsert_env "$ZONE_ENV_FILE" COMPOSE_PROFILES "$profiles"
    if [ -n "$profiles" ]; then
        upsert_env "$ZONE_ENV_FILE" COMPOSE_FILE "$(compose_file_env "$profiles")"
    else
        delete_env "$ZONE_ENV_FILE" COMPOSE_FILE
    fi
    printf '%s\n' "$profiles"
}

find_compose() {
    if docker compose version >/dev/null 2>&1; then
        printf '%s\n' 'docker compose'
        return
    fi
    if command -v docker-compose >/dev/null 2>&1; then
        printf '%s\n' 'docker-compose'
        return
    fi
    printf '%s\n' 'docker compose not found' >&2
    exit 1
}

run_compose() {
    profiles=$1
    shift

    reversed_files=''
    for file in $(compose_files "$profiles"); do
        reversed_files="$file $reversed_files"
    done
    for file in $reversed_files; do
        set -- -f "$file" "$@"
    done

    if [ -n "$profiles" ]; then
        reversed_profiles=''
        old_ifs=$IFS
        IFS=,
        for profile in $profiles; do
            reversed_profiles="$profile $reversed_profiles"
        done
        IFS=$old_ifs
        for profile in $reversed_profiles; do
            set -- --profile "$profile" "$@"
        done
    fi

    compose=$(find_compose)
    # $compose is `docker compose` or `docker-compose`.
    # shellcheck disable=SC2086
    COMPOSE_FILE='' COMPOSE_PROFILES='' exec $compose "$@"
}

case "${1:-}" in
    normalize)
        shift
        normalize_profiles "${1:-}"
        exit 0
        ;;
    flags)
        shift
        print_flags "${1:-}"
        exit 0
        ;;
    files)
        shift
        compose_files "$(normalize_profiles "${1:-}")"
        exit 0
        ;;
    print-profiles)
        persisted_profiles
        exit 0
        ;;
    persist)
        shift
        list=''
        list_set=0
        ensure_arg=''
        while [ $# -gt 0 ]; do
            case "$1" in
                --ensure)
                    ensure_arg=$2
                    shift 2
                    ;;
                --ensure=*)
                    ensure_arg=${1#*=}
                    shift
                    ;;
                --env-file)
                    ZONE_ENV_FILE=$2
                    shift 2
                    ;;
                --env-file=*)
                    ZONE_ENV_FILE=${1#*=}
                    shift
                    ;;
                --)
                    shift
                    break
                    ;;
                -*)
                    printf '%s\n' "Unknown persist option: $1" >&2
                    exit 1
                    ;;
                *)
                    list=$1
                    list_set=1
                    shift
                    break
                    ;;
            esac
        done
        if [ "$list_set" -eq 0 ]; then
            if [ -n "$ensure_arg" ]; then
                list=$(ensure_profile "$ensure_arg" "$(persisted_profiles)")
            else
                list=''
            fi
        elif [ -n "$ensure_arg" ]; then
            list=$(ensure_profile "$ensure_arg" "$list")
        fi
        persist_profiles "$list"
        exit 0
        ;;
esac

ensure_arg=''
replace=0
all_overlays=0
cli_profiles=''

while [ $# -gt 0 ]; do
    case "$1" in
        --replace-profiles=*)
            replace=1
            cli_profiles=${1#*=}
            shift
            ;;
        --replace-profiles)
            replace=1
            cli_profiles=${2:-}
            shift 2
            ;;
        --all-overlays)
            all_overlays=1
            shift
            ;;
        --ensure-profile)
            ensure_arg=$2
            shift 2
            ;;
        --ensure-profile=*)
            ensure_arg=${1#*=}
            shift
            ;;
        --profile)
            cli_profiles=${cli_profiles:+$cli_profiles,}$2
            shift 2
            ;;
        --profile=*)
            cli_profiles=${cli_profiles:+$cli_profiles,}${1#*=}
            shift
            ;;
        --env-file)
            ZONE_ENV_FILE=$2
            shift 2
            ;;
        --env-file=*)
            ZONE_ENV_FILE=${1#*=}
            shift
            ;;
        --)
            shift
            break
            ;;
        *)
            break
            ;;
    esac
done

if [ "$all_overlays" -eq 1 ]; then
    effective=$ALL_OVERLAY_PROFILES
elif [ "$replace" -eq 1 ]; then
    effective=$(normalize_profiles "$cli_profiles")
elif [ -n "$cli_profiles" ]; then
    effective=$(normalize_profiles "$(persisted_profiles),$cli_profiles")
else
    effective=$(persisted_profiles)
fi

if [ -n "$ensure_arg" ]; then
    effective=$(ensure_profile "$ensure_arg" "$effective")
fi

run_compose "$effective" "$@"
