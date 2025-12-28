#!/bin/sh
set -e

# Traefik Entrypoint Script
# Adds conditional TLS/ACME settings based on environment variables

ARGS=""

# HTTP->HTTPS redirect (if enabled)
if [ "${SECURITY_HTTP_REDIRECT}" = "true" ]; then
    echo "[traefik] HTTP->HTTPS redirect ENABLED"
    ARGS="$ARGS --entrypoints.web.http.redirections.entrypoint.to=websecure"
    ARGS="$ARGS --entrypoints.web.http.redirections.entrypoint.scheme=https"
fi

# TLS/ACME certificate generation (if enabled)
if [ "${SECURITY_GENERATE_CERTIFICATE}" = "true" ]; then
    if [ -z "${ADVANCED_ACME_EMAIL}" ]; then
        echo "[traefik] ERROR: SECURITY_GENERATE_CERTIFICATE=true but ADVANCED_ACME_EMAIL is not set"
        exit 1
    fi
    ARGS="$ARGS --certificatesresolvers.letsencrypt.acme.tlschallenge=true"
    ARGS="$ARGS --certificatesresolvers.letsencrypt.acme.email=${ADVANCED_ACME_EMAIL}"
    ARGS="$ARGS --certificatesresolvers.letsencrypt.acme.storage=/letsencrypt/acme.json"
    echo "[traefik] Let's Encrypt certificate generation ENABLED"
fi

echo "[traefik] Starting Traefik..."

# Execute Traefik with command args + conditional args
# shellcheck disable=SC2086
exec /entrypoint.sh "$@" $ARGS
