# =============================================================================
# Zone Installer Dockerfile
# Multi-stage build: UI package -> React frontend -> Rust backend
#
# Build context must be repository root:
#   docker build -f installer/Dockerfile .
# =============================================================================

# =============================================================================
# Stage 1: Build shared UI package
# =============================================================================
FROM node:20-slim AS ui-builder

WORKDIR /ui

# Copy UI package files (no lock file in workspace packages)
COPY packages/ui/package.json ./
RUN npm install

COPY packages/ui/ ./
RUN npm run build

# =============================================================================
# Stage 2: Build React frontend
# =============================================================================
FROM node:20-slim AS frontend-builder

WORKDIR /frontend

# Copy package files (without @zone/ui dependency for initial install)
COPY installer/frontend/package.json installer/frontend/package-lock.json ./

# Create a temporary package.json without the workspace dependency and install
RUN sed -i 's/"@zone\/ui": "\*",//' package.json && npm install

# Copy the built UI package and install it
COPY --from=ui-builder /ui/dist /ui/dist
COPY --from=ui-builder /ui/package.json /ui/package.json
RUN npm install /ui --legacy-peer-deps

# Copy source and build
COPY installer/frontend/ ./
RUN npm run build

# =============================================================================
# Stage 3: Build Rust backend
# =============================================================================
FROM rust:1.85-slim AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY runner/Cargo.toml runner/Cargo.lock ./
COPY runner/zone_installer zone_installer
COPY runner/zone_core zone_core
COPY runner/zone_server zone_server
COPY runner/zone_cli zone_cli
COPY runner/zone_runner zone_runner
COPY runner/tool_runner tool_runner

# Build only the installer binary
RUN cargo build --release --package zone_installer

# =============================================================================
# Stage 4: Runtime
# =============================================================================
FROM debian:bookworm-slim AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends apache2-utils ca-certificates && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd -r appuser && useradd -r -g appuser -s /bin/false appuser

WORKDIR /app

# Copy the binary
COPY --from=builder /build/target/release/zone-installer /app/zone-installer

# Copy frontend build
COPY --from=frontend-builder /frontend/build /app/frontend/build

RUN chown -R appuser:appuser /app

USER appuser

EXPOSE 8000

CMD ["/app/zone-installer"]
