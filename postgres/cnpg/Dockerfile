# Optional CNPG operand with a newer pgvector than the official 16.11 image.
# The Helm chart defaults to ghcr.io/cloudnative-pg/postgresql:16.11, which
# already includes pgvector. Build this only if you need to pin a newer build:
#   docker build -t zone/cnpg-pgvector:16.11 -f postgres/cnpg/Dockerfile postgres/cnpg
FROM ghcr.io/cloudnative-pg/postgresql:16.11

USER root

RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends postgresql-16-pgvector; \
    rm -rf /var/lib/apt/lists/*

# CNPG images run as the postgres UID.
USER 26
