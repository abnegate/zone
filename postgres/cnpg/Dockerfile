# Optional CNPG operand with a newer pgvector than the official 16.15 image.
# The Helm chart defaults to ghcr.io/cloudnative-pg/postgresql:16.15, which
# already includes pgvector. Build this only if you need to pin a newer build:
#   docker build -t zone/cnpg-pgvector:16.15 -f postgres/cnpg/Dockerfile postgres/cnpg
FROM ghcr.io/cloudnative-pg/postgresql:16.15@sha256:34cd4159d07b3410a1b29da35072e2e927a0905e717a61afa3814624a2e8859a

USER root

RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends postgresql-16-pgvector; \
    rm -rf /var/lib/apt/lists/*

# CNPG images run as the postgres UID.
USER 26
