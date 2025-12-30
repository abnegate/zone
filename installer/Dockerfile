FROM ghcr.io/gleam-lang/gleam:v1.14.0-erlang AS builder
WORKDIR /build
COPY gleam.toml .
COPY src src
RUN gleam deps download
RUN gleam build

FROM ghcr.io/gleam-lang/gleam:v1.14.0-erlang AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends apache2-utils && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd -r appuser && useradd -r -g appuser -s /bin/false appuser

WORKDIR /build

COPY --from=builder /build /build
COPY static static
COPY templates templates

RUN chown -R appuser:appuser /build

USER appuser

CMD ["sh", "-c", "cd /build && gleam run"]
