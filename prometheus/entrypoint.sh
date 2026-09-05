#!/bin/sh
# Write LiteLLM's master key so the scrape job can send
# `Authorization: Bearer …`. LiteLLM's /metrics is auth-gated; without this
# the job stays down with HTTP 401.
set -eu

TOKEN_FILE="${PROMETHEUS_LITELLM_BEARER_FILE:-/tmp/litellm_bearer_token}"
TOKEN="${SECURITY_LITELLM_MASTER_KEY:-}"

if [ -z "${TOKEN}" ]; then
  echo "[prometheus] SECURITY_LITELLM_MASTER_KEY is empty; LiteLLM scrape will 401" >&2
fi

# Prometheus reads this as the Bearer token. No trailing newline.
printf '%s' "${TOKEN}" > "${TOKEN_FILE}"
chmod 600 "${TOKEN_FILE}" || true

exec /bin/prometheus "$@"
