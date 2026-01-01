FROM ghcr.io/gleam-lang/gleam:v1.14.0-erlang AS builder

WORKDIR /build

# Optional: DATABASE_URL for regenerating SQL module during build
# If provided, squirrel will regenerate the SQL module before building
# Otherwise, the committed sql.gleam is used (verified up-to-date by CI)
ARG DATABASE_URL

COPY gleam.toml manifest.toml ./
COPY src src
COPY migrations migrations

RUN gleam deps download

# Generate SQL module if DATABASE_URL is provided, otherwise use committed version
RUN if [ -n "$DATABASE_URL" ]; then \
        echo "Generating SQL module with squirrel..." && \
        gleam run -m squirrel; \
    else \
        echo "Using committed sql.gleam (verified by CI)"; \
    fi

RUN gleam build

FROM ghcr.io/gleam-lang/gleam:v1.14.0-erlang AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends curl && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd -r appuser && useradd -r -g appuser -s /bin/false appuser

WORKDIR /build

COPY --from=builder /build /build

RUN chown -R appuser:appuser /build

USER appuser

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8000/api/health || exit 1

CMD ["gleam", "run"]
