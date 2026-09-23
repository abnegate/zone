# syntax=docker/dockerfile:1
# glibc, not musl: zone_server's `local-embeddings` feature links ONNX Runtime,
# which publishes no musl prebuilts.
FROM rust:1.97.1-slim-trixie@sha256:8e8cf8f7fd54a2d23d5a743b3a03f56e26b6c774276c33fa0595111704ebb15c AS builder

WORKDIR /build

# g++ is for libstdc++-dev, not for compiling C++: the statically linked ONNX
# Runtime needs -lstdc++ and rust:slim ships only gcc.
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev g++ \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build/runner
ARG TARGETARCH
ARG TARGETVARIANT

# The whole workspace, because Cargo reads every member's manifest before it
# builds any one package. Copying members by name drifted from
# runner/Cargo.toml once already; .dockerignore keeps the build tree and the
# desktop bundle assets out, and scripts/test-dockerfile-members.sh checks
# every member stays covered.
COPY runner/ ./

# Packaged recipes and workflows are compiled into zone_server via include_str!
COPY comfyui/workflows /build/comfyui/workflows
COPY comfyui/recipes /build/comfyui/recipes
COPY comfyui/custom_nodes /build/comfyui/custom_nodes

ENV SQLX_OFFLINE=true
# Source mtimes can precede cached artifacts after a checkout or file restore.
# Refresh workspace sources so Cargo cannot reuse a stale application binary.
RUN --mount=type=cache,id=zone-manager-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=zone-manager-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=zone-manager-target-${TARGETARCH}${TARGETVARIANT},target=/build/runner/target,sharing=locked \
    find . -path './target' -prune -o -name '*.rs' -exec touch {} + \
    && cargo build --locked --release --package zone_server --package zone_runner \
    && mkdir -p /build/bin \
    && cp target/release/zone-server target/release/zone-runner /build/bin/

# Must match the builder's Debian release so the glibc the binaries were
# linked against is present. libstdc++6 backs the statically linked ONNX Runtime.
FROM debian:trixie-slim@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132 AS runtime-base

# ffmpeg (with ffprobe) decodes the clips submitted for LoRA training. Without
# it images still train; videos are refused with a message saying why.
RUN apt-get update && apt-get install -y --no-install-recommends \
        curl ca-certificates git libssl3t64 libstdc++6 ffmpeg \
    && rm -rf /var/lib/apt/lists/*

FROM runtime-base AS agents

ARG TARGETARCH
ARG CLAUDE_VERSION=2.1.278
ARG CLAUDE_SHA256_AMD64=5c4735937844e84f8a93306e841a5b0e12252909b07870f789b190468da147ab
ARG CLAUDE_SHA256_ARM64=7de6cab134e48321148e30182c98614118e8f4666819412bead45865190b34ed
ARG CODEX_VERSION=0.156.1
ARG CODEX_SHA256_AMD64=aff46539a83aff86e3c62c592bce2c50d95391f9df289afaf03a50c01d14533d
ARG CODEX_SHA256_ARM64=558e12aaa6dacb335ec47240bf9721db8a54746806d64f01185a403f44f79b72

WORKDIR /agents
RUN set -eu; \
    case "$TARGETARCH" in \
        amd64) platform=linux-x64; triple=x86_64-unknown-linux-musl; \
            claude_sha256=$CLAUDE_SHA256_AMD64; codex_sha256=$CODEX_SHA256_AMD64 ;; \
        arm64) platform=linux-arm64; triple=aarch64-unknown-linux-musl; \
            claude_sha256=$CLAUDE_SHA256_ARM64; codex_sha256=$CODEX_SHA256_ARM64 ;; \
        *) echo "No claude or codex release for TARGETARCH=$TARGETARCH" >&2; exit 1 ;; \
    esac; \
    curl -fsSL --proto '=https' --retry 3 -o claude \
        "https://downloads.claude.ai/claude-code-releases/$CLAUDE_VERSION/$platform/claude"; \
    echo "$claude_sha256  claude" | sha256sum -c -; \
    curl -fsSL --proto '=https' --retry 3 -o codex.tar.gz \
        "https://github.com/openai/codex/releases/download/rust-v$CODEX_VERSION/codex-$triple.tar.gz"; \
    echo "$codex_sha256  codex.tar.gz" | sha256sum -c -; \
    test "$(tar -tzf codex.tar.gz)" = "codex-$triple"; \
    tar --no-same-owner -xzf codex.tar.gz; \
    rm codex.tar.gz; \
    chmod 0755 claude "codex-$triple"; \
    mv claude /usr/local/bin/claude; \
    mv "codex-$triple" /usr/local/bin/codex

FROM runtime-base AS runtime

RUN groupadd --system zone \
    && useradd --system --gid zone --create-home --home-dir /home/zone --shell /bin/bash zone

ENV SHELL=/bin/bash

COPY --from=agents /usr/local/bin/claude /usr/local/bin/codex /usr/local/bin/

WORKDIR /app

COPY --from=builder /build/bin/zone-server /app/zone-server
COPY --from=builder /build/bin/zone-runner /app/zone-runner
COPY --from=builder /build/runner/zone_server/migrations /app/migrations
COPY --from=builder /build/comfyui/workflows /app/comfyui/workflows
COPY --from=builder /build/comfyui/recipes /app/comfyui/recipes
COPY comfyui/train_lora.py /app/comfyui/train_lora.py
COPY comfyui/custom_nodes/zone_lora/train_config.json /app/comfyui/custom_nodes/zone_lora/train_config.json

RUN chmod +x /app/zone-server /app/zone-runner

# Docker seeds a new named volume from the image directory, so create these
# paths up front to give first-time volumes zone's ownership. zone owns nothing
# else under /app, so neither the server nor an agent it runs can rewrite it.
RUN mkdir -p /app/workspace /app/agent-state /app/artifacts /app/embed-cache \
    && chown -R zone:zone /app/workspace /app/agent-state /app/artifacts /app/embed-cache \
    && chmod 0700 /app/agent-state

COPY manager/docker-entrypoint.sh /usr/local/bin/manager-entrypoint.sh
RUN chmod 0755 /usr/local/bin/manager-entrypoint.sh

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8000/health || exit 1

ENTRYPOINT ["/usr/local/bin/manager-entrypoint.sh"]
CMD ["/app/zone-server"]
