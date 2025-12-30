FROM ghcr.io/gleam-lang/gleam:v1.14.0-erlang AS builder

WORKDIR /build

COPY gleam.toml .
COPY src src

RUN gleam deps download
RUN gleam build

FROM ghcr.io/gleam-lang/gleam:v1.14.0-erlang AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends curl && \
    rm -rf /var/lib/apt/lists/*

# Create non-root user for security
# Note: Gleam/Erlang applications don't require root privileges
RUN groupadd -r appuser && useradd -r -g appuser -s /bin/false appuser

WORKDIR /app

COPY --from=builder /build /app
COPY static static
COPY templates templates

# Change ownership to non-root user
RUN chown -R appuser:appuser /app

# Run as non-root user
USER appuser

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8000/ || exit 1

CMD ["gleam", "run"]
