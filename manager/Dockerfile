# =============================================================================
# Stage 1: Build Rust application
# =============================================================================
FROM rust:1.92-alpine3.23 AS builder

WORKDIR /build

RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig

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
COPY runner/.sqlx ./.sqlx/

# Touch all source files to ensure Cargo detects changes from dummy files
RUN find . -name "*.rs" -exec touch {} +

ENV SQLX_OFFLINE=true
RUN cargo build --release --package zone_server --package zone_runner

# =============================================================================
# Stage 2: Runtime
# =============================================================================
FROM alpine:3.21 AS runtime

RUN apk add --no-cache curl ca-certificates libssl3

RUN addgroup -S zone && adduser -S -G zone -s /sbin/nologin zone

WORKDIR /app

COPY --from=builder /build/runner/target/release/zone-server /app/zone-server
COPY --from=builder /build/runner/target/release/zone-runner /app/zone-runner
COPY --from=builder /build/runner/zone_server/migrations /app/migrations

RUN chmod +x /app/zone-server /app/zone-runner

RUN chown -R zone:zone /app

USER zone

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8000/api/health || exit 1

CMD ["/app/zone-server"]
