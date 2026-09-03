# =============================================================================
# Stage 1: Build shared UI package
# =============================================================================
FROM oven/bun:1.4.0-alpine@sha256:07235578f79ef8c6f97d94aee7938e76f5cdba5f21ae5dbfdd3d3d38058437eb AS ui-builder

WORKDIR /ui

COPY --chown=bun:bun packages/ui/package.json ./
RUN bun install

COPY --chown=bun:bun packages/ui/ ./
RUN bun run build

# =============================================================================
# Stage 2: Install frontend dependencies
# =============================================================================
FROM oven/bun:1.4.0-alpine@sha256:07235578f79ef8c6f97d94aee7938e76f5cdba5f21ae5dbfdd3d3d38058437eb AS frontend-deps

WORKDIR /frontend

COPY --chown=bun:bun installer/frontend/package.json ./
RUN sed -i 's/"@zone\/ui": "\*",//' package.json && bun install

# =============================================================================
# Stage 3: Build React frontend
# =============================================================================
FROM oven/bun:1.4.0-alpine@sha256:07235578f79ef8c6f97d94aee7938e76f5cdba5f21ae5dbfdd3d3d38058437eb AS frontend-builder

WORKDIR /frontend

COPY --from=frontend-deps --chown=bun:bun /frontend/node_modules ./node_modules
COPY --from=frontend-deps --chown=bun:bun /frontend/package.json ./

COPY --from=ui-builder --chown=bun:bun /ui/dist /ui/dist
COPY --from=ui-builder --chown=bun:bun /ui/package.json /ui/package.json
COPY --from=ui-builder --chown=bun:bun /ui/node_modules /ui/node_modules
RUN bun add /ui

COPY installer/frontend/tsconfig.json installer/frontend/vite.config.ts installer/frontend/index.html installer/frontend/tailwind.config.ts ./

COPY --chown=bun:bun installer/frontend/src ./src
COPY --chown=bun:bun packages/ui/src /packages/ui/src

RUN bun run build

# =============================================================================
# Stage 4: Build Rust backend with dependency caching
# =============================================================================
FROM rust:1.97.1-alpine3.23@sha256:c4a364ddbf684fe038e6fa6a4f25b30c8dc85247423e0e660676ece0d17be4a2 AS builder

WORKDIR /build

RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig

COPY runner/Cargo.toml runner/Cargo.lock ./

COPY runner/zone_installer/Cargo.toml ./zone_installer/
COPY runner/zone_core/Cargo.toml ./zone_core/
COPY runner/zone_server/Cargo.toml ./zone_server/
COPY runner/zone_cli/Cargo.toml ./zone_cli/
COPY runner/zone_runner/Cargo.toml ./zone_runner/
COPY runner/tool_runner/Cargo.toml ./tool_runner/
COPY runner/zone_context/Cargo.toml ./zone_context/

RUN mkdir -p zone_installer/src zone_core/src zone_server/src zone_cli/src \
             zone_runner/src tool_runner/src zone_context/src && \
    echo "fn main() {}" > zone_installer/src/main.rs && \
    echo "fn main() {}" > zone_core/src/lib.rs && \
    echo "fn main() {}" > zone_server/src/main.rs && \
    echo "fn main() {}" > zone_cli/src/main.rs && \
    echo "fn main() {}" > zone_runner/src/main.rs && \
    echo "fn main() {}" > tool_runner/src/lib.rs && \
    echo "fn main() {}" > zone_context/src/lib.rs

RUN cargo build --release --package zone_installer 2>/dev/null || true

COPY runner/zone_installer ./zone_installer/
COPY runner/zone_core ./zone_core/
COPY runner/zone_server ./zone_server/
COPY runner/zone_cli ./zone_cli/
COPY runner/zone_runner ./zone_runner/
COPY runner/tool_runner ./tool_runner/
COPY runner/zone_context ./zone_context/

# Touch all source files to ensure Cargo detects changes from dummy files
RUN find . -name "*.rs" -exec touch {} +

RUN cargo build --release --package zone_installer

# =============================================================================
# Stage 5: Runtime
# =============================================================================
FROM alpine:3.23@sha256:fd791d74b68913cbb027c6546007b3f0d3bc45125f797758156952bc2d6daf40 AS runtime

RUN apk add --no-cache apache2-utils ca-certificates docker-cli docker-cli-compose

RUN addgroup -S appuser && adduser -S -G appuser -s /sbin/nologin appuser

WORKDIR /app

COPY --from=builder /build/target/release/zone-installer /app/zone-installer
COPY --from=frontend-builder /frontend/build /app/frontend/build

RUN chown -R appuser:appuser /app

USER appuser

EXPOSE 8000

CMD ["/app/zone-installer"]
