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

COPY runner/Cargo.toml runner/Cargo.lock ./
COPY runner/tool_runner ./tool_runner/
COPY runner/zone_runner ./zone_runner/
COPY runner/zone_core ./zone_core/
COPY runner/zone_context ./zone_context/
COPY runner/zone_server ./zone_server/
COPY runner/zone_cli ./zone_cli/
COPY runner/zone_installer ./zone_installer/
COPY runner/zone_desktop/Cargo.toml ./zone_desktop/
COPY runner/zone_desktop/src ./zone_desktop/src/
COPY runner/.sqlx ./.sqlx

# Packaged recipes and workflows are compiled into zone_server via include_str!
COPY comfyui/workflows /build/comfyui/workflows
COPY comfyui/recipes /build/comfyui/recipes

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
FROM debian:trixie-slim@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132 AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
        curl ca-certificates libssl3t64 libstdc++6 \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --system zone \
    && useradd --system --gid zone --shell /usr/sbin/nologin --no-create-home zone

WORKDIR /app

COPY --from=builder /build/bin/zone-server /app/zone-server
COPY --from=builder /build/bin/zone-runner /app/zone-runner
COPY --from=builder /build/runner/zone_server/migrations /app/migrations
COPY --from=builder /build/comfyui/workflows /app/comfyui/workflows
COPY --from=builder /build/comfyui/recipes /app/comfyui/recipes

RUN chmod +x /app/zone-server /app/zone-runner

# Docker seeds a new named volume from the image directory, so create these
# paths up front to give first-time volumes zone's ownership.
RUN mkdir -p /app/embed-cache /app/artifacts

COPY manager/docker-entrypoint.sh /usr/local/bin/manager-entrypoint.sh
RUN chmod 0755 /usr/local/bin/manager-entrypoint.sh \
    && chown -R zone:zone /app

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8000/health || exit 1

ENTRYPOINT ["/usr/local/bin/manager-entrypoint.sh"]
CMD ["/app/zone-server"]
