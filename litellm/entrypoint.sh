#!/bin/sh
set -e

# Cleanup function for temporary files
cleanup() {
    [ -n "${CONFIG_YAML}" ] && [ -f "${CONFIG_YAML}" ] && rm -f "${CONFIG_YAML}"
    [ -n "${ROUTER_JSON}" ] && [ -f "${ROUTER_JSON}" ] && rm -f "${ROUTER_JSON}"
}
trap cleanup EXIT

# URL encode function for DATABASE_URL password
url_encode() {
    local string="${1}"
    local strlen=${#string}
    local encoded=""
    local pos c o

    for pos in $(seq 0 "$((strlen - 1))"); do
        c=$(printf '%s' "$string" | cut -c"$((pos + 1))")
        case "$c" in
            [-_.~a-zA-Z0-9])
                o="${c}"
                ;;
            *)
                # Handle single quote specially - use octal escape
                if [ "$c" = "'" ]; then
                    o=$(printf '%%%02X' 39)
                else
                    o=$(printf '%%%02X' "'$c")
                fi
                ;;
        esac
        encoded="${encoded}${o}"
    done
    printf '%s' "${encoded}"
}

# Construct DATABASE_URL with URL-encoded password
# Always construct it here to avoid race condition with special characters in compose
if [ -n "${POSTGRES_PASSWORD}" ]; then
    POSTGRES_USER_VAL="${POSTGRES_USER:-litellm}"
    POSTGRES_DB_VAL="${POSTGRES_DB:-litellm}"

    # Check if password contains special URL characters that need encoding
    case "${POSTGRES_PASSWORD}" in
        *@* | *:* | */* | *%* | *?* | *#* | *\&* | *=*)
            echo "[litellm-entrypoint] Encoding DATABASE_URL password for special characters..."
            ENCODED_PASSWORD=$(url_encode "${POSTGRES_PASSWORD}")
            export DATABASE_URL="postgresql://${POSTGRES_USER_VAL}:${ENCODED_PASSWORD}@postgres:5432/${POSTGRES_DB_VAL}"
            echo "[litellm-entrypoint] ✓ DATABASE_URL password encoded"
            ;;
        *)
            # No special characters, use password as-is
            export DATABASE_URL="postgresql://${POSTGRES_USER_VAL}:${POSTGRES_PASSWORD}@postgres:5432/${POSTGRES_DB_VAL}"
            ;;
    esac
fi

# Validate model names contain only safe characters (alphanumeric, dash, underscore, colon, dot)
validate_model_name() {
    local name="$1"
    local var="$2"
    if ! echo "${name}" | grep -qE '^[a-zA-Z0-9:._-]+$'; then
        echo "[litellm-entrypoint] ERROR: Invalid characters in ${var}: ${name}"
        exit 1
    fi
}

validate_model_name "${OLLAMA_MODEL_FAST}" "OLLAMA_MODEL_FAST"
validate_model_name "${OLLAMA_MODEL_REASON}" "OLLAMA_MODEL_REASON"
validate_model_name "${OLLAMA_MODEL_EMBED}" "OLLAMA_MODEL_EMBED"

# Generate config.yaml from template with model names
if [ -f /app/config.yaml.template ]; then
    echo "[litellm-entrypoint] Generating config.yaml from template..."

    CONFIG_YAML=$(mktemp /tmp/config.yaml.XXXXXX)

    sed -e "s|{{OLLAMA_MODEL_FAST}}|${OLLAMA_MODEL_FAST}|g" \
        -e "s|{{OLLAMA_MODEL_REASON}}|${OLLAMA_MODEL_REASON}|g" \
        -e "s|{{OLLAMA_MODEL_EMBED}}|${OLLAMA_MODEL_EMBED}|g" \
        /app/config.yaml.template > "${CONFIG_YAML}"

    echo "[litellm-entrypoint] ✓ config.yaml generated"
    echo "[litellm-entrypoint]   fast:   ${OLLAMA_MODEL_FAST}"
    echo "[litellm-entrypoint]   reason: ${OLLAMA_MODEL_REASON}"
    echo "[litellm-entrypoint]   embed:  ${OLLAMA_MODEL_EMBED}"
else
    echo "[litellm-entrypoint] Warning: config.yaml.template not found"
    exit 1
fi

# Generate router.json from template with environment variable substitution
if [ -f /app/router.json.template ]; then
    echo "[litellm-entrypoint] Generating router.json from template..."

    ROUTER_JSON=$(mktemp /tmp/router.json.XXXXXX)

    sed "s|{{OLLAMA_EMBED_MODEL}}|${OLLAMA_MODEL_EMBED}|g" /app/router.json.template > "${ROUTER_JSON}"

    echo "[litellm-entrypoint] ✓ router.json generated"
    export ROUTER_JSON
else
    echo "[litellm-entrypoint] Warning: router.json.template not found"
    exit 1
fi

# Execute litellm with the generated config
echo "[litellm-entrypoint] Starting LiteLLM proxy..."
exec litellm --config "${CONFIG_YAML}" "$@"
