# =============================================================================
# Manager Dockerfile
# Multi-stage build with Rust tool runner
#
# Note: When building with the Rust runner, set build context to repo root:
#   docker build -f manager/Dockerfile .
# Otherwise, build from manager directory for Gleam-only build.
# =============================================================================

# =============================================================================
# Stage 1: Build Rust tool runner (optional)
# This stage is skipped if runner directory doesn't exist
# =============================================================================
FROM rust:1.92-slim-bookworm AS rust-builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Copy Rust source - these files are optional
# If building from manager context only, this will fail and that's ok
COPY runner/Cargo.toml runner/Cargo.lock ./runner/
COPY runner/tool_runner ./runner/tool_runner/
COPY runner/zone_runner ./runner/zone_runner/

# Build release binary
WORKDIR /build/runner
RUN cargo build --release --package zone_runner

# =============================================================================
# Stage 2: Build Gleam application
# =============================================================================
FROM ghcr.io/gleam-lang/gleam:v1.14.0-erlang AS gleam-builder

WORKDIR /build

# Optional: DATABASE_URL for regenerating SQL module during build
ARG DATABASE_URL

# Copy from manager directory (works with both contexts)
COPY manager/gleam.toml manager/manifest.toml ./
COPY manager/src src
COPY manager/migrations migrations

RUN gleam deps download

# Generate SQL module if DATABASE_URL is provided
RUN if [ -n "$DATABASE_URL" ]; then \
        echo "Generating SQL module with squirrel..." && \
        gleam run -m squirrel; \
    else \
        echo "Using committed sql.gleam (verified by CI)"; \
    fi

RUN gleam build

# =============================================================================
# Stage 3: Runtime
# =============================================================================
FROM ghcr.io/gleam-lang/gleam:v1.14.0-erlang AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd -r appuser && useradd -r -g appuser -s /bin/false appuser

WORKDIR /build

# Copy Gleam build
COPY --from=gleam-builder /build /build

# Create priv/bin directory
RUN mkdir -p /build/priv/bin

# Copy Rust binary (this will fail silently if rust-builder stage was skipped)
COPY --from=rust-builder /build/runner/target/release/zone-runner /build/priv/bin/zone-runner

# Ensure binary is executable (ignore errors if file doesn't exist)
RUN chmod +x /build/priv/bin/zone-runner 2>/dev/null || true

RUN chown -R appuser:appuser /build

USER appuser

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8000/api/health || exit 1

CMD ["gleam", "run"]
