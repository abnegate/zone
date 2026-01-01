FROM ghcr.io/gleam-lang/gleam:v1.14.0-erlang AS builder

WORKDIR /build

COPY gleam.toml manifest.toml ./
COPY src src
COPY migrations migrations

RUN gleam deps download
RUN gleam build

FROM ghcr.io/gleam-lang/gleam:v1.14.0-erlang AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends curl && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd -r appuser && useradd -r -g appuser -s /bin/false appuser

WORKDIR /app

COPY --from=builder /build /app

RUN chown -R appuser:appuser /app

USER appuser

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8000/api/health || exit 1

CMD ["gleam", "run"]
