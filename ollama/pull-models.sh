#!/bin/sh
set -e

readonly MAX_RETRIES=30
readonly RETRY_INTERVAL=5

GREEN=''
YELLOW=''
OUTPUT_RESET=''
if [ -t 1 ]; then
    GREEN=$(printf '\033[0;32m')
    YELLOW=$(printf '\033[1;33m')
    OUTPUT_RESET=$(printf '\033[0m')
fi

RED=''
ERROR_RESET=''
if [ -t 2 ]; then
    RED=$(printf '\033[0;31m')
    ERROR_RESET=$(printf '\033[0m')
fi

readonly GREEN YELLOW OUTPUT_RESET RED ERROR_RESET

log_info() {
    printf '%s[ollama-init]%s %s\n' "${GREEN}" "${OUTPUT_RESET}" "$1"
}

log_warn() {
    printf '%s[ollama-init]%s %s\n' "${YELLOW}" "${OUTPUT_RESET}" "$1"
}

log_error() {
    printf '%s[ollama-init ERROR]%s %s\n' "${RED}" "${ERROR_RESET}" "$1" >&2
}

validate_env() (
    missing=0

    if [ -z "${OLLAMA_HOST}" ]; then
        log_error "OLLAMA_HOST is not set"
        missing=1
    fi

    if [ -z "${OLLAMA_MODEL_FAST}" ]; then
        log_error "OLLAMA_MODEL_FAST is not set"
        missing=1
    fi

    if [ -z "${OLLAMA_MODEL_REASON}" ]; then
        log_error "OLLAMA_MODEL_REASON is not set"
        missing=1
    fi

    if [ -z "${OLLAMA_MODEL_EMBED}" ]; then
        log_error "OLLAMA_MODEL_EMBED is not set"
        missing=1
    fi

    if [ $missing -eq 1 ]; then
        log_error "Missing required environment variables. Exiting."
        exit 1
    fi
)

readonly BUNDLED_OLLAMA_HOST='http://ollama:11434'

# Whether OLLAMA_HOST names the Ollama this compose stack runs itself. Pulls
# into anything else happen on that host's own store, so an unreachable
# external Ollama is not this container's failure.
targets_bundled_ollama() {
    [ "${OLLAMA_HOST%/}" = "${BUNDLED_OLLAMA_HOST}" ]
}

wait_for_ollama() (
    if targets_bundled_ollama; then
        log_info "Pulling into the bundled Ollama at ${OLLAMA_HOST}"
    else
        log_info "OLLAMA_BASE_URL points outside the stack: pulling into ${OLLAMA_HOST}"
    fi
    log_info "Waiting for Ollama API at ${OLLAMA_HOST}..."

    retries=0
    while [ $retries -lt $MAX_RETRIES ]; do
        if ollama list >/dev/null 2>&1; then
            log_info "Ollama API is ready!"
            return 0
        fi

        retries=$((retries + 1))
        log_warn "Ollama not ready yet (attempt $retries/$MAX_RETRIES)..."
        sleep "${RETRY_INTERVAL}"
    done

    return 1
)

model_exists() {
    ollama list | grep -qF "$1"
}

pull_model() (
    model_name="$1"
    model_type="$2"

    log_info "Checking ${model_type} model: ${model_name}"

    if model_exists "${model_name}"; then
        log_info "✓ ${model_name} already pulled, skipping"
        return 0
    fi

    log_info "Pulling ${model_name}..."

    if ollama pull "${model_name}"; then
        log_info "✓ Successfully pulled ${model_name}"
        return 0
    else
        log_error "✗ Failed to pull ${model_name}"
        return 1
    fi
)

main() (
    log_info "===== Ollama Model Initialization ====="

    validate_env

    if ! wait_for_ollama; then
        if targets_bundled_ollama; then
            log_error "Ollama API failed to become ready after $MAX_RETRIES attempts"
            exit 1
        fi

        log_warn "OLLAMA_HOST ${OLLAMA_HOST} is not reachable from this container after $MAX_RETRIES attempts; nothing pulled."
        log_warn "Set OLLAMA_BASE_URL=${BUNDLED_OLLAMA_HOST} to pull into the bundled Ollama, or run 'ollama pull' on the host that serves ${OLLAMA_HOST}."
        exit 0
    fi

    log_info "Model Configuration:"
    log_info "  Fast Model:      ${OLLAMA_MODEL_FAST}"
    log_info "  Reasoning Model: ${OLLAMA_MODEL_REASON}"
    log_info "  Embedding Model: ${OLLAMA_MODEL_EMBED}"
    if [ -n "${OLLAMA_MODEL_VISION:-}" ]; then
        log_info "  Vision Model:    ${OLLAMA_MODEL_VISION}"
    fi
    printf '\n'

    failed=0

    pull_model "${OLLAMA_MODEL_FAST}" "FAST" || failed=1
    pull_model "${OLLAMA_MODEL_REASON}" "REASONING" || failed=1
    pull_model "${OLLAMA_MODEL_EMBED}" "EMBEDDING" || failed=1
    if [ -n "${OLLAMA_MODEL_VISION:-}" ]; then
        pull_model "${OLLAMA_MODEL_VISION}" "VISION" || failed=1
    fi

    printf '\n'

    if [ $failed -eq 0 ]; then
        log_info "===== Model initialization complete! ====="
        exit 0
    else
        log_error "===== Model initialization failed! ====="
        log_error "Some models failed to pull. Check logs above for details."
        exit 1
    fi
)

main
