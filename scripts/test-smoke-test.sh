#!/bin/sh
set -eu

unset DOMAIN_HOST_WEBUI OLLAMA_BASE_URL COMPOSE_PROFILES ZONE_COMPOSE ZONE_ENV_FILE \
    LITELLM_STATUS LITELLM_HEALTH OLLAMA_HEALTH UNREACHABLE CONTAINERS PROFILES \
    MANAGER_CONFIG_FILES CONSOLE_CONFIG_FILES

directory=$(mktemp -d)
trap 'rm -rf "$directory"' EXIT HUP INT TERM
source=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)/smoke-test.sh
sandbox=$directory/zone
script=$sandbox/scripts/smoke-test.sh

mkdir -p "$directory/bin" "$sandbox/scripts"
cp "$source" "$script"

cat > "$directory/bin/curl" <<'CURL'
#!/bin/sh
printf '%s\n' "$*" >> "$CURL_LOG"
url=''
writes_status=0
for argument do
    case "$argument" in
        http://*|https://*) url=$argument ;;
        -w) writes_status=1 ;;
    esac
done
if [ -n "${UNREACHABLE:-}" ]; then
    case "$url" in
        *"$UNREACHABLE"*)
            [ "$writes_status" -eq 0 ] || printf '000'
            exit 7
            ;;
    esac
fi
case "$url" in
    *litellm.*) printf '%s' "${LITELLM_STATUS:-401}" ;;
esac
exit 0
CURL

cat > "$directory/bin/docker" <<'DOCKER'
#!/bin/sh
printf '%s\n' "$*" >> "$DOCKER_LOG"
case "$1" in
    ps)
        for container in ${CONTAINERS:-litellm}; do
            printf '%s\n' "$container"
        done
        ;;
    inspect)
        for argument do
            container=$argument
        done
        case "$*" in
            *com.docker.compose.project.config_files*)
                case "$container" in
                    manager) files=${MANAGER_CONFIG_FILES:-} ;;
                    console) files=${CONSOLE_CONFIG_FILES:-} ;;
                    *) files='' ;;
                esac
                [ -n "$files" ] || exit 1
                printf '%s\n' "$files"
                ;;
            *)
                case "$container" in
                    litellm) printf '%s\n' "${LITELLM_HEALTH:-healthy}" ;;
                    ollama) printf '%s\n' "${OLLAMA_HEALTH:-healthy}" ;;
                    *) exit 1 ;;
                esac
                ;;
        esac
        ;;
    *) exit 1 ;;
esac
DOCKER

cat > "$directory/bin/fake-compose" <<'COMPOSE'
#!/bin/sh
printf '%s\n' "$*" >> "$COMPOSE_LOG"
[ "$1" = print-profiles ] || exit 1
printf '%s\n' "${PROFILES:-}"
COMPOSE
chmod +x "$directory/bin/curl" "$directory/bin/docker" "$directory/bin/fake-compose"

fail() {
    printf '%s\n' "$1" >&2
    if [ -f "$directory/output" ]; then
        sed 's/^/  output: /' "$directory/output" >&2
    fi
    if [ -f "$directory/curl.log" ]; then
        sed 's/^/  curl: /' "$directory/curl.log" >&2
    fi
    exit 1
}

write_env() {
    printf '%s\n' "$@" > "$sandbox/.env"
}

smoke() {
    : > "$directory/curl.log"
    : > "$directory/docker.log"
    : > "$directory/compose.log"
    status=0
    env \
        PATH="$directory/bin:$PATH" \
        CURL_LOG="$directory/curl.log" \
        DOCKER_LOG="$directory/docker.log" \
        COMPOSE_LOG="$directory/compose.log" \
        ZONE_COMPOSE=fake-compose \
        "$@" \
        sh "$script" > "$directory/output" 2>&1 || status=$?
    rm -f "$sandbox/.env"
    return "$status"
}

requested() {
    grep -qF -- "$1" "$directory/curl.log"
}

assert_requested() {
    requested "$1" || fail "Expected a request containing: $1"
}

assert_not_requested() {
    if requested "$1"; then
        fail "Expected no request containing: $1"
    fi
}

assert_output() {
    grep -qF -- "$1" "$directory/output" || fail "Expected output containing: $1"
}

assert_never_legacy_probes() {
    assert_not_requested ':3000'
    assert_not_requested ':4000'
    assert_not_requested '/api/health'
}

assert_dev_probes() {
    assert_output 'against the dev stack'
    assert_requested 'http://localhost:3001/'
    assert_requested 'http://localhost:8000/health'
    assert_not_requested 'manager.localhost'
}

assert_install_probes() {
    assert_output 'against the install stack'
    assert_requested 'https://manager.localhost/health'
    assert_not_requested 'localhost:3001'
    assert_not_requested 'localhost:8000'
}

smoke || fail 'Expected an install with LiteLLM answering 401 behind basic auth to pass'
assert_requested '--resolve litellm.webui.localhost:443:127.0.0.1'
assert_requested 'https://litellm.webui.localhost/health/readiness'
assert_requested '--resolve manager.localhost:443:127.0.0.1'
assert_requested 'https://manager.localhost/health'
assert_requested 'https://manager.localhost/'
assert_requested 'http://127.0.0.1:11434/api/tags'
assert_install_probes
assert_never_legacy_probes
grep -qF 'inspect' "$directory/docker.log" || fail 'Expected the LiteLLM container health to be inspected'
assert_output '✓ LiteLLM'
assert_output '✓ Manager'
assert_output '✓ Console'
assert_output '✓ Ollama'

smoke LITELLM_STATUS=200 || fail 'Expected LiteLLM answering 200 to pass'

if smoke LITELLM_STATUS=502; then
    fail 'Expected LiteLLM answering 502 to fail'
fi
assert_output '✗ LiteLLM'

if smoke LITELLM_HEALTH=unhealthy; then
    fail 'Expected an unhealthy LiteLLM container to fail'
fi
assert_output '✗ LiteLLM container (unhealthy)'

write_env 'DOMAIN_HOST_WEBUI=from-file.example'
smoke || fail 'Expected DOMAIN_HOST_WEBUI from the repository .env to pass'
assert_requested '--resolve litellm.from-file.example:443:127.0.0.1'
assert_requested 'https://litellm.from-file.example/health/readiness'

printf '%s\n' 'DOMAIN_HOST_WEBUI=override.example' > "$directory/custom.env"
write_env 'DOMAIN_HOST_WEBUI=from-file.example'
smoke ZONE_ENV_FILE="$directory/custom.env" || fail 'Expected DOMAIN_HOST_WEBUI from ZONE_ENV_FILE to pass'
assert_requested 'https://litellm.override.example/health/readiness'
assert_not_requested 'from-file.example'

write_env 'DOMAIN_HOST_WEBUI=from-file.example'
smoke DOMAIN_HOST_WEBUI=zone.example || fail 'Expected DOMAIN_HOST_WEBUI from the environment to pass'
assert_requested '--resolve litellm.zone.example:443:127.0.0.1'
assert_requested 'https://litellm.zone.example/health/readiness'
assert_not_requested 'from-file.example'
assert_not_requested 'webui.localhost'

write_env \
    '# DOMAIN_HOST_WEBUI=commented.example' \
    'DOMAIN_HOST_WEBUI=first.example' \
    'DOMAIN_HOST_WEBUI=zone.example # the public host' \
    'OLLAMA_BASE_URL="http://models.example:11434/" # quoted with a comment'
smoke || fail 'Expected .env values with trailing comments and double quotes to pass'
assert_requested 'https://litellm.zone.example/health/readiness'
assert_requested 'http://models.example:11434/api/tags'
assert_not_requested 'commented.example'
assert_not_requested 'first.example'
assert_not_requested '#'

write_env \
    "export DOMAIN_HOST_WEBUI='single.example'   # single quoted" \
    "OLLAMA_BASE_URL='http://models.example:11434' "
smoke || fail 'Expected single-quoted .env values to pass'
assert_requested 'https://litellm.single.example/health/readiness'
assert_requested 'http://models.example:11434/api/tags'

write_env 'DOMAIN_HOST_WEBUI=zone.example#not-a-comment'
if smoke; then
    fail 'Expected a # without leading whitespace to stay part of the value, as Compose reads it'
fi
assert_output 'DOMAIN_HOST_WEBUI is not a host name: zone.example#not-a-comment'

if smoke DOMAIN_HOST_WEBUI='bad host'; then
    fail 'Expected a DOMAIN_HOST_WEBUI that is not a host name to be rejected'
fi
assert_output 'DOMAIN_HOST_WEBUI is not a host name: bad host'
[ ! -s "$directory/curl.log" ] || fail 'Expected no probes once DOMAIN_HOST_WEBUI is rejected'

smoke PROFILES=dev,vpn || fail 'Expected the dev stack to pass'
assert_dev_probes
assert_requested '--resolve litellm.webui.localhost:443:127.0.0.1'
assert_never_legacy_probes

smoke PROFILES=monitoring || fail 'Expected a stack without the dev profile to pass as an install'
assert_install_probes

smoke MANAGER_CONFIG_FILES=/srv/zone/docker-compose.yml,/srv/zone/docker-compose.dev.yml \
    || fail 'Expected a running manager started with the dev overlay to pass as the dev stack'
assert_dev_probes
[ ! -s "$directory/compose.log" ] || fail 'Expected the running stack to decide the mode without reading persisted profiles'

smoke PROFILES=dev MANAGER_CONFIG_FILES=/srv/zone/docker-compose.yml \
    || fail 'Expected a running manager without the dev overlay to pass as an install'
assert_install_probes

smoke CONSOLE_CONFIG_FILES=docker-compose.yml,docker-compose.dev.yml,docker-compose.override.yml \
    || fail 'Expected a running console started with the dev overlay to pass as the dev stack'
assert_dev_probes

smoke MANAGER_CONFIG_FILES=/srv/zone/docker-compose.yml,/srv/zone/old-docker-compose.dev.yml \
    || fail 'Expected only the dev overlay itself to mark the dev stack'
assert_install_probes

smoke PROFILES=dev || fail 'Expected the persisted dev profile to decide when no stack is running'
assert_dev_probes
grep -qx 'print-profiles' "$directory/compose.log" || fail 'Expected persisted profiles to be read when no stack is running'

if smoke UNREACHABLE='manager.localhost/health'; then
    fail 'Expected an unreachable manager to fail'
fi
assert_output '✗ Manager'
assert_output '✓ Console'

if smoke PROFILES=dev UNREACHABLE='localhost:8000'; then
    fail 'Expected an unreachable dev manager to fail'
fi
assert_output '✗ Manager'

if smoke UNREACHABLE='11434'; then
    fail 'Expected an unreachable host Ollama to fail'
fi
assert_output '✗ Ollama'

write_env 'OLLAMA_BASE_URL=http://models.example:11434/'
smoke || fail 'Expected an external Ollama from .env to pass'
assert_requested 'http://models.example:11434/api/tags'

smoke CONTAINERS='litellm ollama' || fail 'Expected a healthy bundled Ollama to pass'
assert_not_requested '11434'
grep -qF 'ollama' "$directory/docker.log" || fail 'Expected the bundled Ollama container health to be inspected'

if smoke CONTAINERS='litellm ollama' OLLAMA_HEALTH=starting; then
    fail 'Expected a bundled Ollama that is not healthy to fail'
fi
assert_output '✗ Ollama container (starting)'

printf '%s\n' 'Smoke test checks passed'
