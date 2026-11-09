#!/bin/sh
set -e

SETTINGS_DIR="/etc/searxng"
SETTINGS_FILE="${SETTINGS_DIR}/settings.yml"
TEMPLATE_FILE="/searxng/settings.yml.template"

# Generate settings.yml from template
if [ -f "${TEMPLATE_FILE}" ]; then
    echo "[searxng-entrypoint] Generating settings.yml from template..."

    # Validate secret key is set and not a known insecure default
    if [ -z "${SEARXNG_SECRET_KEY}" ]; then
        echo "[searxng-entrypoint] ERROR: SEARXNG_SECRET_KEY is not set"
        exit 1
    fi

    # Check against common insecure values
    case "${SEARXNG_SECRET_KEY}" in
        "ultrasecretkey"|"secret"|"password"|"dev-insecure-key-change-for-production"|"changeme"|"insecure")
            echo "[searxng-entrypoint] ERROR: SEARXNG_SECRET_KEY is set to a known insecure default"
            echo "[searxng-entrypoint] Generate a secure key with: openssl rand -base64 32"
            exit 1
            ;;
    esac

    # Warn if key is too short (less than 16 characters)
    if [ "${#SEARXNG_SECRET_KEY}" -lt 16 ]; then
        echo "[searxng-entrypoint] WARNING: SEARXNG_SECRET_KEY is too short (< 16 chars)"
        echo "[searxng-entrypoint] Continuing, but consider using: openssl rand -base64 32"
    fi

    # Escape special characters for sed
    ESCAPED_KEY=$(printf '%s\n' "${SEARXNG_SECRET_KEY}" | sed 's/[&/\]/\\&/g')

    # Perform substitution
    sed "s|{{SEARXNG_SECRET_KEY}}|${ESCAPED_KEY}|g" "${TEMPLATE_FILE}" > "${SETTINGS_FILE}"

    echo "[searxng-entrypoint] ✓ settings.yml generated"
else
    echo "[searxng-entrypoint] ERROR: Template not found: ${TEMPLATE_FILE}"
    exit 1
fi

# Execute the original entrypoint
exec /usr/local/searxng/entrypoint.sh
