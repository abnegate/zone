# =============================================================================
# Zone Server Dockerfile
# Multi-stage build for Rust backend
#
# Build context should be repo root:
#   docker build -f manager/Dockerfile .
# =============================================================================

# =============================================================================
# Stage 1: Build Rust application
# =============================================================================
FROM rust:1.85-slim-bookworm AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy Cargo workspace files first for better caching
COPY runner/Cargo.toml runner/Cargo.lock ./runner/

# Copy all crate manifests
COPY runner/tool_runner/Cargo.toml ./runner/tool_runner/
COPY runner/zone_runner/Cargo.toml ./runner/zone_runner/
COPY runner/zone_core/Cargo.toml ./runner/zone_core/
COPY runner/zone_server/Cargo.toml ./runner/zone_server/
COPY runner/zone_cli/Cargo.toml ./runner/zone_cli/
COPY runner/zone_installer/Cargo.toml ./runner/zone_installer/

# Create stub files to satisfy cargo's dependency resolution
RUN mkdir -p runner/tool_runner/src runner/zone_runner/src runner/zone_core/src \
    runner/zone_server/src runner/zone_cli/src runner/zone_installer/src && \
    echo "fn main() {}" > runner/tool_runner/src/lib.rs && \
    echo "fn main() {}" > runner/zone_runner/src/main.rs && \
    echo "fn main() {}" > runner/zone_core/src/lib.rs && \
    echo "fn main() {}" > runner/zone_server/src/main.rs && \
    echo "fn main() {}" > runner/zone_cli/src/main.rs && \
    echo "fn main() {}" > runner/zone_installer/src/main.rs

# Download dependencies (cached unless Cargo.toml changes)
WORKDIR /build/runner
RUN cargo fetch

# Now copy actual source code
COPY runner/tool_runner ./tool_runner/
COPY runner/zone_runner ./zone_runner/
COPY runner/zone_core ./zone_core/
COPY runner/zone_server ./zone_server/
COPY runner/zone_cli ./zone_cli/
COPY runner/zone_installer ./zone_installer/

# Copy migrations and sqlx offline data
COPY runner/zone_server/migrations ./zone_server/migrations/
COPY runner/zone_server/.sqlx ./zone_server/.sqlx/

# Build release binaries
ENV SQLX_OFFLINE=true
RUN cargo build --release --package zone_server --package zone_runner

# =============================================================================
# Stage 2: Runtime
# =============================================================================
FROM debian:bookworm-slim AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    curl \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r zone && useradd -r -g zone -s /bin/false zone

WORKDIR /app

# Copy binaries from builder
COPY --from=builder /build/runner/target/release/zone-server /app/zone-server
COPY --from=builder /build/runner/target/release/zone-runner /app/zone-runner

# Copy migrations for runtime migration support
COPY --from=builder /build/runner/zone_server/migrations /app/migrations

# Ensure binaries are executable
RUN chmod +x /app/zone-server /app/zone-runner

# Change ownership
RUN chown -R zone:zone /app

USER zone

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8000/api/health || exit 1

CMD ["/app/zone-server"]
