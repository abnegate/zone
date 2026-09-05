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

yaml_single_quote() {
  # YAML single-quoted scalars: escape ' as ''.
  printf "%s" "${1-}" | sed "s/'/''/g"
}

write_alert_contact_points() {
  local dest="$1"
  local email_addr discord_url
  email_addr="$(yaml_single_quote "${ALERT_EMAIL_RECIPIENTS:-admin@example.com}")"
  discord_url="$(yaml_single_quote "${ALERT_DISCORD_WEBHOOK_URL:-}")"

  {
    cat <<EOF
apiVersion: 1
contactPoints:
  - orgId: 1
    name: email-notifications
    receivers:
      - uid: email-receiver
        type: email
        settings:
          addresses: '${email_addr}'
          singleEmail: false
        disableResolveMessage: false
EOF
    if [ -n "${ALERT_DISCORD_WEBHOOK_URL:-}" ]; then
      cat <<EOF
      - uid: discord-receiver
        type: discord
        disableResolveMessage: false
        settings:
          use_discord_username: true
          message: |
            {{ template "default.message" . }}
        secureSettings:
          url: '${discord_url}'
EOF
      echo "[grafana] Discord alert contact point enabled" >&2
    else
      echo "[grafana] ALERT_DISCORD_WEBHOOK_URL is empty; Discord alerts disabled" >&2
    fi
    cat <<'EOF'
policies:
  - orgId: 1
    receiver: email-notifications
    group_by:
      - grafana_folder
      - alertname
    group_wait: 30s
    group_interval: 5m
    repeat_interval: 4h
EOF
  } >"${dest}"
}

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

# Provisioning is a read-only mount. Copy it and inject Discord/email
# receivers so an empty webhook does not fail Grafana startup.
RUNTIME_PROVISION="${DATA_PATH}/provisioning-runtime"
rm -rf "${RUNTIME_PROVISION}"
mkdir -p "${RUNTIME_PROVISION}"
cp -a /etc/grafana/provisioning/. "${RUNTIME_PROVISION}/"
mkdir -p "${RUNTIME_PROVISION}/plugins"
write_alert_contact_points "${RUNTIME_PROVISION}/alerting/alerting.yml"
export GF_PATHS_PROVISIONING="${RUNTIME_PROVISION}"

exec /run.sh "$@"
