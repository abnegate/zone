#!/bin/sh
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

ZONE_ENV_FILE=${ZONE_ENV_FILE:-$root/.env}
ZONE_COMPOSE=${ZONE_COMPOSE:-./scripts/compose.sh}
TIMEOUT_SECONDS=10
LOOPBACK=127.0.0.1

if [ -t 1 ]; then
    BLUE=$(printf '\033[0;34m')
    GREEN=$(printf '\033[0;32m')
    RED=$(printf '\033[0;31m')
    RESET=$(printf '\033[0m')
else
    BLUE=''
    GREEN=''
    RED=''
    RESET=''
fi

failures=0

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

setting() {
    name=$1
    fallback=$2
    eval "value=\${$name:-}"
    if [ -z "$value" ]; then
        value=$(read_env_value "$name" "$ZONE_ENV_FILE")
    fi
    printf '%s\n' "${value:-$fallback}"
}

pass() {
    printf '%s\n' "${GREEN}✓ $1${RESET}"
}

fail() {
    printf '%s\n' "${RED}✗ $1${RESET}"
    failures=$((failures + 1))
}

probe() {
    curl -skf --max-time "$TIMEOUT_SECONDS" -o /dev/null "$@"
}

check() {
    label=$1
    shift
    if probe "$@"; then
        pass "$label"
    else
        fail "$label"
    fi
}

container_state() {
    docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$1" 2>/dev/null || printf '%s\n' missing
}

check_container() {
    label=$1
    container=$2
    state=$(container_state "$container")
    case "$state" in
        healthy|running) pass "$label container ($state)" ;;
        *) fail "$label container (${state:-unknown})" ;;
    esac
}

container_listed() {
    docker ps --format '{{.Names}}' 2>/dev/null | grep -qx "$1"
}

is_dev_mode() {
    profiles=$("$ZONE_COMPOSE" print-profiles) || {
        printf '%s\n' "${RED}Could not read the active Compose profiles from $ZONE_COMPOSE${RESET}" >&2
        exit 1
    }
    printf '%s\n' "$profiles" | tr ', ' '\n' | grep -qx dev
}

check_manager_and_console() {
    if [ "$1" = dev ]; then
        check 'Manager (http://localhost:8000/health)' http://localhost:8000/health
        check 'Console (http://localhost:3001/)' http://localhost:3001/
    else
        resolve="manager.localhost:443:$LOOPBACK"
        check 'Manager (https://manager.localhost/health)' --resolve "$resolve" https://manager.localhost/health
        check 'Console (https://manager.localhost/)' --resolve "$resolve" https://manager.localhost/
    fi
}

check_litellm() {
    host="litellm.$1"
    url="https://$host/health/readiness"
    status=$(curl -sk --max-time "$TIMEOUT_SECONDS" -o /dev/null -w '%{http_code}' \
        --resolve "$host:443:$LOOPBACK" "$url") || status=${status:-000}
    case "$status" in
        200|401) pass "LiteLLM ($url, HTTP $status)" ;;
        *) fail "LiteLLM ($url, HTTP $status)" ;;
    esac
    check_container LiteLLM litellm
}

check_ollama() {
    if container_listed ollama; then
        check_container Ollama ollama
        return
    fi
    base=$(printf '%s\n' "$1" | sed -e "s#host\.docker\.internal#$LOOPBACK#" -e 's#/*$##')
    check "Ollama ($base/api/tags)" "$base/api/tags"
}

domain=$(setting DOMAIN_HOST_WEBUI webui.localhost)
case "$domain" in
    *[!A-Za-z0-9.-]*|'')
        printf '%s\n' "${RED}DOMAIN_HOST_WEBUI is not a host name: $domain${RESET}" >&2
        exit 1
        ;;
esac
ollama_base_url=$(setting OLLAMA_BASE_URL http://host.docker.internal:11434)

if is_dev_mode; then
    mode=dev
else
    mode=install
fi

printf '%s\n' "${BLUE}Running smoke tests against the $mode stack...${RESET}"
check_manager_and_console "$mode"
check_litellm "$domain"
check_ollama "$ollama_base_url"

if [ "$failures" -gt 0 ]; then
    printf '%s\n' "${RED}$failures smoke test(s) failed.${RESET}"
    exit 1
fi
printf '%s\n' "${GREEN}All smoke tests passed.${RESET}"
