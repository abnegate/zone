# =============================================================================
# Stage 1: Build Rust application
# =============================================================================
# glibc, not musl: zone_server's `local-embeddings` feature links ONNX Runtime,
# which publishes no musl prebuilts.
FROM rust:1.92-slim-trixie AS builder

WORKDIR /build

# g++ is for libstdc++-dev, not for compiling C++: the statically linked ONNX
# Runtime needs -lstdc++ and rust:slim ships only gcc.
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev g++ \
    && rm -rf /var/lib/apt/lists/*

COPY runner/Cargo.toml runner/Cargo.lock ./runner/

COPY runner/tool_runner/Cargo.toml ./runner/tool_runner/
COPY runner/zone_runner/Cargo.toml ./runner/zone_runner/
COPY runner/zone_core/Cargo.toml ./runner/zone_core/
COPY runner/zone_context/Cargo.toml ./runner/zone_context/
COPY runner/zone_server/Cargo.toml ./runner/zone_server/
COPY runner/zone_cli/Cargo.toml ./runner/zone_cli/
COPY runner/zone_installer/Cargo.toml ./runner/zone_installer/

RUN mkdir -p runner/tool_runner/src runner/zone_runner/src runner/zone_core/src \
    runner/zone_context/src runner/zone_server/src runner/zone_cli/src runner/zone_installer/src && \
    echo "fn main() {}" > runner/tool_runner/src/lib.rs && \
    echo "fn main() {}" > runner/zone_runner/src/main.rs && \
    echo "fn main() {}" > runner/zone_core/src/lib.rs && \
    echo "fn main() {}" > runner/zone_context/src/lib.rs && \
    echo "fn main() {}" > runner/zone_server/src/lib.rs && \
    echo "fn main() {}" > runner/zone_server/src/main.rs && \
    echo "fn main() {}" > runner/zone_cli/src/main.rs && \
    echo "fn main() {}" > runner/zone_installer/src/main.rs

WORKDIR /build/runner
RUN cargo build --release --package zone_server --package zone_runner 2>/dev/null || true

COPY runner/tool_runner ./tool_runner/
COPY runner/zone_runner ./zone_runner/
COPY runner/zone_core ./zone_core/
COPY runner/zone_context ./zone_context/
COPY runner/zone_server ./zone_server/
COPY runner/zone_cli ./zone_cli/
COPY runner/zone_installer ./zone_installer/

COPY runner/zone_server/migrations ./zone_server/migrations/

# Touch all source files to ensure Cargo detects changes from dummy files
RUN find . -name "*.rs" -exec touch {} +

ENV SQLX_OFFLINE=true
RUN cargo build --release --package zone_server --package zone_runner

# =============================================================================
# Stage 2: Runtime
# =============================================================================
# Must match the builder's Debian release so the glibc the binaries were
# linked against is present. libstdc++6 backs the statically linked ONNX Runtime.
FROM debian:trixie-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
        curl ca-certificates libssl3t64 libstdc++6 \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --system zone \
    && useradd --system --gid zone --shell /usr/sbin/nologin --no-create-home zone

WORKDIR /app

COPY --from=builder /build/runner/target/release/zone-server /app/zone-server
COPY --from=builder /build/runner/target/release/zone-runner /app/zone-runner
COPY --from=builder /build/runner/zone_server/migrations /app/migrations

RUN chmod +x /app/zone-server /app/zone-runner

# Docker seeds a new named volume from the image directory, so create the
# embedding model cache up front to give the volume zone's ownership.
RUN mkdir -p /app/embed-cache

RUN chown -R zone:zone /app

USER zone

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8000/health || exit 1

CMD ["/app/zone-server"]
