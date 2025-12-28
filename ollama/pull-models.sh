#!/bin/sh
set -e

# =============================================================================
# Ollama Model Initialization Script
# =============================================================================
# This script pulls the required models into the shared Ollama volume.
# It only runs once during initial setup (restart: "no" in docker-compose).
# =============================================================================

readonly MAX_RETRIES=30
readonly RETRY_INTERVAL=5

# Color output for better readability
readonly RED='\033[0;31m'
readonly GREEN='\033[0;32m'
readonly YELLOW='\033[1;33m'
readonly NC='\033[0m' # No Color

log_info() {
    echo "${GREEN}[ollama-init]${NC} $1"
}

log_warn() {
    echo "${YELLOW}[ollama-init]${NC} $1"
}

log_error() {
    echo "${RED}[ollama-init ERROR]${NC} $1" >&2
}

# Validate environment variables
validate_env() {
    local missing=0

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
}

# Wait for Ollama API to be ready
wait_for_ollama() {
    log_info "Waiting for Ollama API at ${OLLAMA_HOST}..."

    local retries=0
    while [ $retries -lt $MAX_RETRIES ]; do
        if ollama list >/dev/null 2>&1; then
            log_info "Ollama API is ready!"
            return 0
        fi

        retries=$((retries + 1))
        log_warn "Ollama not ready yet (attempt $retries/$MAX_RETRIES)..."
        sleep $RETRY_INTERVAL
    done

    log_error "Ollama API failed to become ready after $MAX_RETRIES attempts"
    exit 1
}

# Check if a model is already pulled
model_exists() {
    local model_name="$1"

    if ollama list | grep -q "^${model_name}[[:space:]]"; then
        return 0  # Model exists
    else
        return 1  # Model doesn't exist
    fi
}

# Pull a single model with error handling
pull_model() {
    local model_name="$1"
    local model_type="$2"

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
}

# Main execution
main() {
    log_info "===== Ollama Model Initialization ====="

    # Validate environment
    validate_env

    # Wait for Ollama to be ready
    wait_for_ollama

    # Display model configuration
    log_info "Model Configuration:"
    log_info "  Fast Model:      ${OLLAMA_MODEL_FAST}"
    log_info "  Reasoning Model: ${OLLAMA_MODEL_REASON}"
    log_info "  Embedding Model: ${OLLAMA_MODEL_EMBED}"
    echo ""

    # Pull models
    local failed=0

    pull_model "${OLLAMA_MODEL_FAST}" "FAST" || failed=1
    pull_model "${OLLAMA_MODEL_REASON}" "REASONING" || failed=1
    pull_model "${OLLAMA_MODEL_EMBED}" "EMBEDDING" || failed=1

    echo ""

    if [ $failed -eq 0 ]; then
        log_info "===== Model initialization complete! ====="
        exit 0
    else
        log_error "===== Model initialization failed! ====="
        log_error "Some models failed to pull. Check logs above for details."
        exit 1
    fi
}

main
