#!/bin/sh
set -eu

echo "[ollama-init] Waiting for Ollama API..."
until wget -qO- http://ollama:11434/api/tags >/dev/null 2>&1; do
  sleep 2
done

echo "[ollama-init] Pulling models into shared volume..."
echo "  FAST:       ${OLLAMA_FAST_MODEL}"
echo "  REASONING:  ${OLLAMA_REASON_MODEL}"
echo "  EMBEDDING:  ${OLLAMA_EMBED_MODEL}"

ollama pull "${OLLAMA_FAST_MODEL}"
ollama pull "${OLLAMA_REASON_MODEL}"
ollama pull "${OLLAMA_EMBED_MODEL}"

echo "[ollama-init] Done."