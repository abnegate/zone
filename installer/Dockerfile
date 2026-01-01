# Build React frontend
FROM node:20-slim AS frontend-builder
WORKDIR /frontend
COPY frontend/package*.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# Build Gleam backend
FROM ghcr.io/gleam-lang/gleam:v1.14.0-erlang AS builder
WORKDIR /build
COPY gleam.toml .
COPY src src
RUN gleam deps download
RUN gleam build

# Runtime
FROM ghcr.io/gleam-lang/gleam:v1.14.0-erlang AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends apache2-utils && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd -r appuser && useradd -r -g appuser -s /bin/false appuser

WORKDIR /build

COPY --from=builder /build /build
COPY --from=frontend-builder /frontend/build /build/frontend/build

RUN chown -R appuser:appuser /build

USER appuser

CMD ["sh", "-c", "cd /build && gleam run"]
