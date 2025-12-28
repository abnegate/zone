#!/bin/sh
set -e

# Traefik Dynamic Configuration Script
# Adds HTTP->HTTPS redirect based on SECURITY_HTTP_REDIRECT env var

ARGS=""

# Base Traefik configuration
ARGS="$ARGS --api.dashboard=true"
ARGS="$ARGS --providers.docker=true"
ARGS="$ARGS --providers.docker.exposedbydefault=false"
ARGS="$ARGS --providers.docker.network=voiz_edge"
ARGS="$ARGS --entrypoints.web.address=:80"
ARGS="$ARGS --entrypoints.websecure.address=:443"
ARGS="$ARGS --entrypoints.ping.address=:8082"
ARGS="$ARGS --ping=true"

# HTTP->HTTPS redirect (if enabled)
if [ "${SECURITY_HTTP_REDIRECT}" = "true" ]; then
    echo "[traefik] HTTP->HTTPS redirect ENABLED"
    ARGS="$ARGS --entrypoints.web.http.redirections.entrypoint.to=websecure"
    ARGS="$ARGS --entrypoints.web.http.redirections.entrypoint.scheme=https"
else
    echo "[traefik] HTTP->HTTPS redirect DISABLED"
fi

# TLS/ACME certificate generation (independent of redirect)
if [ "${SECURITY_GENERATE_CERTIFICATE}" = "true" ]; then
    ARGS="$ARGS --certificatesresolvers.letsencrypt.acme.tlschallenge=true"
    ARGS="$ARGS --certificatesresolvers.letsencrypt.acme.email=${ADVANCED_ACME_EMAIL}"
    ARGS="$ARGS --certificatesresolvers.letsencrypt.acme.storage=/letsencrypt/acme.json"
    echo "[traefik] Let's Encrypt certificate generation ENABLED"
else
    echo "[traefik] Let's Encrypt certificate generation DISABLED"
fi

# Logging
ARGS="$ARGS --log.level=INFO"
ARGS="$ARGS --accesslog=true"

echo "[traefik] Starting with configuration:"
echo "$ARGS" | tr ' ' '\n' | grep "^--"

# Execute Traefik with generated arguments
exec /entrypoint.sh $ARGS
