#!/bin/bash
# Apply MONITORING_GRAFANA_ADMIN_PASSWORD (GF_SECURITY_ADMIN_PASSWORD) to the
# persisted admin user. Grafana only honors the env var on first boot; after
# that the hash in grafana.db wins unless we reset it against the same data
# path the server uses.
set -euo pipefail

HOME_PATH="${GF_PATHS_HOME:-/usr/share/grafana}"
CONFIG_PATH="${GF_PATHS_CONFIG:-/etc/grafana/grafana.ini}"
DATA_PATH="${GF_PATHS_DATA:-/var/lib/grafana}"
PASSWORD="${GF_SECURITY_ADMIN_PASSWORD:-}"
ADMIN_ID="${GF_SECURITY_ADMIN_USER_ID:-1}"

if [ -z "${PASSWORD}" ]; then
  echo "[grafana] GF_SECURITY_ADMIN_PASSWORD is empty; leaving the admin hash unchanged"
elif [ ! -f "${DATA_PATH}/grafana.db" ]; then
  echo "[grafana] No grafana.db yet; first boot will create admin from GF_SECURITY_ADMIN_PASSWORD"
else
  echo "[grafana] Syncing admin password from GF_SECURITY_ADMIN_PASSWORD"
  # printf (no newline) so the stored hash matches what you type in the login form.
  # --configOverrides is required: grafana-cli otherwise writes to
  # $GF_PATHS_HOME/data instead of the volume at $GF_PATHS_DATA.
  if ! printf '%s' "${PASSWORD}" | grafana cli \
    --homepath="${HOME_PATH}" \
    --config="${CONFIG_PATH}" \
    --configOverrides="cfg:default.paths.data=${DATA_PATH}" \
    admin reset-admin-password --user-id "${ADMIN_ID}" --password-from-stdin
  then
    echo "[grafana] admin password reset failed; Grafana will start with the existing hash" >&2
  fi
fi

exec /run.sh "$@"
