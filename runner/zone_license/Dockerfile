# syntax=docker/dockerfile:1
# Build from the repository root: docker build -f runner/zone_license/Dockerfile .
FROM rust:1.97.1-slim-trixie@sha256:8e8cf8f7fd54a2d23d5a743b3a03f56e26b6c774276c33fa0595111704ebb15c AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build/runner
ARG TARGETARCH
ARG TARGETVARIANT

# Cargo reads every workspace member's manifest before it builds one package.
COPY runner/ ./

RUN --mount=type=cache,id=zone-license-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=zone-license-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=zone-license-target-${TARGETARCH}${TARGETVARIANT},target=/build/runner/target,sharing=locked \
    find . -path './target' -prune -o -name '*.rs' -exec touch {} + \
    && cargo build --locked --release --package zone_license \
    && mkdir -p /build/bin \
    && cp target/release/zone-license /build/bin/

# Must match the builder's Debian release so the glibc and OpenSSL the binary
# was linked against are present.
FROM debian:trixie-slim@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3t64 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 zone-license \
    && useradd --system --uid 10001 --gid zone-license --no-create-home --shell /usr/sbin/nologin zone-license

COPY --from=builder /build/bin/zone-license /usr/local/bin/zone-license

USER 10001:10001

EXPOSE 8080

ENTRYPOINT ["zone-license"]
CMD ["serve"]
