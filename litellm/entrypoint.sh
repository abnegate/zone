#!/bin/sh
set -e

# Generate router.json from template with environment variable substitution
# Write to /tmp since /app is read-only
if [ -f /app/router.json.template ]; then
    echo "[litellm-entrypoint] Generating router.json from template..."
    sed "s|{{OLLAMA_EMBED_MODEL}}|${OLLAMA_EMBED_MODEL}|g" /app/router.json.template > /tmp/router.json
    echo "[litellm-entrypoint] ✓ router.json generated with embedding model: ${OLLAMA_EMBED_MODEL}"
    echo "[litellm-entrypoint] ✓ router.json location: /tmp/router.json"
else
    echo "[litellm-entrypoint] Warning: router.json.template not found"
    exit 1
fi

# Execute the original litellm command
echo "[litellm-entrypoint] Starting LiteLLM proxy..."
echo "[litellm-entrypoint] Command: $@"
exec litellm "$@"
