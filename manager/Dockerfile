# syntax=docker/dockerfile:1
# glibc, not musl: zone_server's `local-embeddings` feature links ONNX Runtime,
# which publishes no musl prebuilts.
FROM rust:1.97.1-slim-trixie@sha256:8e8cf8f7fd54a2d23d5a743b3a03f56e26b6c774276c33fa0595111704ebb15c AS builder

WORKDIR /build

# g++ is for libstdc++-dev, not for compiling C++: the statically linked ONNX
# Runtime needs -lstdc++ and rust:slim ships only gcc.
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev g++ \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build/runner
ARG TARGETARCH
ARG TARGETVARIANT

# The whole workspace, because Cargo reads every member's manifest before it
# builds any one package. Copying members by name drifted from
# runner/Cargo.toml once already; .dockerignore keeps the build tree and the
# desktop bundle assets out, and scripts/test-dockerfile-members.sh checks
# every member stays covered.
COPY runner/ ./

# Packaged recipes, workflows, and the setup catalog are compiled into
# zone_server via include_str!
COPY comfyui/workflows /build/comfyui/workflows
COPY comfyui/recipes /build/comfyui/recipes
COPY comfyui/custom_nodes /build/comfyui/custom_nodes
COPY comfyui/model-manifest.json /build/comfyui/model-manifest.json
COPY scripts/setup-features.json /build/scripts/setup-features.json

ENV SQLX_OFFLINE=true
# Source mtimes can precede cached artifacts after a checkout or file restore.
# Refresh workspace sources so Cargo cannot reuse a stale application binary.
RUN --mount=type=cache,id=zone-manager-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=zone-manager-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=zone-manager-target-${TARGETARCH}${TARGETVARIANT},target=/build/runner/target,sharing=locked \
    find . -path './target' -prune -o -name '*.rs' -exec touch {} + \
    && cargo build --locked --release --package zone_server --package zone_runner \
    && mkdir -p /build/bin \
    && cp target/release/zone-server target/release/zone-runner /build/bin/

# Must match the builder's Debian release so the glibc the binaries were
# linked against is present. libstdc++6 backs the statically linked ONNX Runtime.
FROM debian:trixie-slim@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132 AS runtime-base

# ffmpeg (with ffprobe) decodes the clips submitted for LoRA training. Without
# it images still train; videos are refused with a message saying why.
RUN apt-get update && apt-get install -y --no-install-recommends \
        curl ca-certificates git libssl3t64 libstdc++6 ffmpeg \
    && rm -rf /var/lib/apt/lists/*

FROM runtime-base AS agents

ARG TARGETARCH
ARG CLAUDE_VERSION=2.1.278
ARG CLAUDE_SHA256_AMD64=5c4735937844e84f8a93306e841a5b0e12252909b07870f789b190468da147ab
ARG CLAUDE_SHA256_ARM64=7de6cab134e48321148e30182c98614118e8f4666819412bead45865190b34ed
ARG CODEX_VERSION=0.156.1
ARG CODEX_SHA256_AMD64=aff46539a83aff86e3c62c592bce2c50d95391f9df289afaf03a50c01d14533d
ARG CODEX_SHA256_ARM64=558e12aaa6dacb335ec47240bf9721db8a54746806d64f01185a403f44f79b72
ARG BWRAP_SHA256_AMD64=74bb868f29e845da36ab287b88459a5fbb1fa4e16ff59837660312eaf0b33f3a
ARG BWRAP_SHA256_ARM64=42803e6a0f5557bb32fbef636aa9efa805eb5a108c28a670e83fd1a0e1c672aa
ARG CODE_MODE_HOST_SHA256_AMD64=a929daa9f6a0bddc00c0c9e6402df117b125acd96f9d554f6c99c32c7e66c608
ARG CODE_MODE_HOST_SHA256_ARM64=40198138b03798ffa8c0da4c827a8ca5896774ea104b7110c2a2c0c7560cbe94

WORKDIR /agents
RUN set -eu; \
    case "$TARGETARCH" in \
        amd64) platform=linux-x64; triple=x86_64-unknown-linux-musl; \
            claude_sha256=$CLAUDE_SHA256_AMD64; codex_sha256=$CODEX_SHA256_AMD64; \
            bwrap_sha256=$BWRAP_SHA256_AMD64; host_sha256=$CODE_MODE_HOST_SHA256_AMD64 ;; \
        arm64) platform=linux-arm64; triple=aarch64-unknown-linux-musl; \
            claude_sha256=$CLAUDE_SHA256_ARM64; codex_sha256=$CODEX_SHA256_ARM64; \
            bwrap_sha256=$BWRAP_SHA256_ARM64; host_sha256=$CODE_MODE_HOST_SHA256_ARM64 ;; \
        *) echo "No claude or codex release for TARGETARCH=$TARGETARCH" >&2; exit 1 ;; \
    esac; \
    fetch() { \
        curl -fsSL --proto '=https' --retry 3 -o "$1" "$2"; \
        echo "$3  $1" | sha256sum -c -; \
    }; \
    unpack() { \
        test "$(tar -tzf "$1.tar.gz")" = "$1-$triple"; \
        tar --no-same-owner -xzf "$1.tar.gz"; \
        rm "$1.tar.gz"; \
        mv "$1-$triple" "$1"; \
    }; \
    releases="https://github.com/openai/codex/releases/download/rust-v$CODEX_VERSION"; \
    fetch claude "https://downloads.claude.ai/claude-code-releases/$CLAUDE_VERSION/$platform/claude" "$claude_sha256"; \
    fetch codex.tar.gz "$releases/codex-$triple.tar.gz" "$codex_sha256"; \
    fetch bwrap.tar.gz "$releases/bwrap-$triple.tar.gz" "$bwrap_sha256"; \
    fetch codex-code-mode-host.tar.gz "$releases/codex-code-mode-host-$triple.tar.gz" "$host_sha256"; \
    unpack codex; \
    unpack bwrap; \
    unpack codex-code-mode-host; \
    chmod 0755 claude codex bwrap codex-code-mode-host; \
    mkdir /usr/local/bin/codex-resources; \
    mv claude codex codex-code-mode-host /usr/local/bin/; \
    mv bwrap /usr/local/bin/codex-resources/bwrap

FROM oven/bun:1.4.0-slim@sha256:e0ee68d16ccb9927bf02aa7dd8fd4bf3369ee6d46da04faa72b05ce8bfd135f6 AS bun

FROM runtime-base AS fetch

RUN apt-get update && apt-get install -y --no-install-recommends unzip xz-utils \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /fetch

# Agents start with only HOME and PATH, so RUSTUP_HOME cannot reach them and
# rustup's proxies would look for a toolchain in each agent's own HOME. The
# standalone toolchain needs no rustup and goes on PATH directly.
FROM fetch AS rust-toolchain

ARG TARGETARCH
ARG RUST_VERSION=1.99.1
ARG RUST_SHA256_AMD64=231fcae47ee950621374725b5fc3b5441d2fbda2955ac45dd7dc816d8876cfe7
ARG RUST_SHA256_ARM64=a48fb1e853be56ce9b8e2751f5464dbd5e16440cd81e237920f3a5e7e806333e

RUN set -eu; \
    case "$TARGETARCH" in \
        amd64) triple=x86_64-unknown-linux-gnu; sha256=$RUST_SHA256_AMD64 ;; \
        arm64) triple=aarch64-unknown-linux-gnu; sha256=$RUST_SHA256_ARM64 ;; \
        *) echo "No Rust toolchain for TARGETARCH=$TARGETARCH" >&2; exit 1 ;; \
    esac; \
    curl -fsSL --proto '=https' --retry 3 -o rust.tar.xz \
        "https://static.rust-lang.org/dist/rust-$RUST_VERSION-$triple.tar.xz"; \
    echo "$sha256  rust.tar.xz" | sha256sum -c -; \
    tar --no-same-owner -xJf rust.tar.xz; \
    "./rust-$RUST_VERSION-$triple/install.sh" --prefix=/opt/rust --disable-ldconfig \
        --components="rustc,cargo,rust-std-$triple,rustfmt-preview,clippy-preview"

FROM fetch AS go-toolchain

ARG TARGETARCH
ARG GO_VERSION=1.27.3
ARG GO_SHA256_AMD64=65efbc80fdc1fa2cd2631cad5b481777a68e8725f34e89f50fc6ac714e379b40
ARG GO_SHA256_ARM64=a06e99309dbcbdca1a8ccd2282c3ee146e55930886e26b4f1193269587165f49

RUN set -eu; \
    case "$TARGETARCH" in \
        amd64) sha256=$GO_SHA256_AMD64 ;; \
        arm64) sha256=$GO_SHA256_ARM64 ;; \
        *) echo "No Go release for TARGETARCH=$TARGETARCH" >&2; exit 1 ;; \
    esac; \
    curl -fsSL --proto '=https' --retry 3 -o go.tar.gz \
        "https://go.dev/dl/go$GO_VERSION.linux-$TARGETARCH.tar.gz"; \
    echo "$sha256  go.tar.gz" | sha256sum -c -; \
    tar --no-same-owner -C /usr/local -xzf go.tar.gz; \
    rm go.tar.gz

# Maven Central publishes only a SHA-512 for Maven.
FROM fetch AS jvm

ARG GRADLE_VERSION=9.8.2
ARG GRADLE_SHA256=21bd5da2de49b642904077f38c0efdc7778e92b3ed0889392dca61b97e389607
ARG MAVEN_VERSION=3.9.16
ARG MAVEN_SHA512=831a8591fe20c8243b1dbe7d71e3244f31d1665b0804b2e825e38cbbe5ce0cafb8338851f90780735568773e0a6cd07bbec107cda0b896b008b861075358b6f6
ARG KOTLIN_VERSION=2.4.21
ARG KOTLIN_SHA256=7cc140e76daf416a0424a5557f99afd7ca89ebd463ea1101261e3e01c33e75cd

RUN set -eu; \
    fetch() { \
        curl -fsSL --proto '=https' --retry 3 -o "$1" "$2"; \
        echo "$4  $1" | "$3" -c -; \
    }; \
    fetch gradle.zip "https://services.gradle.org/distributions/gradle-$GRADLE_VERSION-bin.zip" \
        sha256sum "$GRADLE_SHA256"; \
    fetch maven.tar.gz "https://repo1.maven.org/maven2/org/apache/maven/apache-maven/$MAVEN_VERSION/apache-maven-$MAVEN_VERSION-bin.tar.gz" \
        sha512sum "$MAVEN_SHA512"; \
    fetch kotlin.zip "https://github.com/JetBrains/kotlin/releases/download/v$KOTLIN_VERSION/kotlin-compiler-$KOTLIN_VERSION.zip" \
        sha256sum "$KOTLIN_SHA256"; \
    mkdir -p /opt/jvm/maven; \
    unzip -q gradle.zip -d /opt/jvm; \
    mv "/opt/jvm/gradle-$GRADLE_VERSION" /opt/jvm/gradle; \
    tar --no-same-owner --strip-components=1 -C /opt/jvm/maven -xzf maven.tar.gz; \
    unzip -q kotlin.zip -d /opt/jvm; \
    rm gradle.zip maven.tar.gz kotlin.zip

FROM fetch AS binaries

ARG TARGETARCH
ARG COMPOSER_VERSION=2.11.1
ARG COMPOSER_SHA256=6f1b891e1d4927d62740276c540fb490e47689e19e102c49e8da849794882f8c
ARG DENO_VERSION=2.9.8
ARG DENO_SHA256_AMD64=e890975bc2549807bbcc0b584ffafda72cc7c93349dd99c80483dd784f6c4609
ARG DENO_SHA256_ARM64=d49596c20b9788cf8f5d8ddf644103d4bde56ed239753f0bdb92ba9361d51905
ARG UV_VERSION=0.13.7
ARG UV_SHA256_AMD64=c5221ff41b7892bfc223543883600125f6a47fe845093e0f5bdff4f359196d83
ARG UV_SHA256_ARM64=71279f8d3c67241e807b9d8db872d1bf541579fb9f08b7d1eb1682196d1e57dc

RUN set -eu; \
    case "$TARGETARCH" in \
        amd64) triple=x86_64-unknown-linux-gnu; \
            deno_sha256=$DENO_SHA256_AMD64; uv_sha256=$UV_SHA256_AMD64 ;; \
        arm64) triple=aarch64-unknown-linux-gnu; \
            deno_sha256=$DENO_SHA256_ARM64; uv_sha256=$UV_SHA256_ARM64 ;; \
        *) echo "No deno or uv release for TARGETARCH=$TARGETARCH" >&2; exit 1 ;; \
    esac; \
    fetch() { \
        curl -fsSL --proto '=https' --retry 3 -o "$1" "$2"; \
        echo "$3  $1" | sha256sum -c -; \
    }; \
    fetch composer "https://getcomposer.org/download/$COMPOSER_VERSION/composer.phar" "$COMPOSER_SHA256"; \
    fetch deno.zip "https://github.com/denoland/deno/releases/download/v$DENO_VERSION/deno-$triple.zip" "$deno_sha256"; \
    fetch uv.tar.gz "https://github.com/astral-sh/uv/releases/download/$UV_VERSION/uv-$triple.tar.gz" "$uv_sha256"; \
    mkdir /opt/bin; \
    unzip -q deno.zip -d /opt/bin; \
    tar --no-same-owner --strip-components=1 -C /opt/bin -xzf uv.tar.gz "uv-$triple/uv" "uv-$triple/uvx"; \
    mv composer /opt/bin/composer; \
    chmod 0755 /opt/bin/*; \
    rm deno.zip uv.tar.gz

# Microsoft publishes only a SHA-512 for the .NET SDK. The userlocal marker
# installs workloads under each user's HOME instead of the root-owned SDK.
FROM fetch AS dotnet-sdk

ARG TARGETARCH
ARG DOTNET_VERSION=10.0.403
ARG DOTNET_SHA512_AMD64=b7b3d8461cf98b07d2552ce9199702fa3ad010ac2671a5f8cd5136f7534e50f6855ba7bbb02b11494563f1d04156276a31cda1103b900e5eecd0d9473dc7b9c5
ARG DOTNET_SHA512_ARM64=0966aa2b70f4677b8319be9df447a78a0dcd6164836425b55a68fd714f4023feb87b4cac21b9a996b79472ad54487cd0adcd2a9dfd570e2b2a1d26423a2baa9b

RUN set -eu; \
    case "$TARGETARCH" in \
        amd64) rid=x64; sha512=$DOTNET_SHA512_AMD64 ;; \
        arm64) rid=arm64; sha512=$DOTNET_SHA512_ARM64 ;; \
        *) echo "No .NET SDK for TARGETARCH=$TARGETARCH" >&2; exit 1 ;; \
    esac; \
    curl -fsSL --proto '=https' --retry 3 -o dotnet.tar.gz \
        "https://builds.dotnet.microsoft.com/dotnet/Sdk/$DOTNET_VERSION/dotnet-sdk-$DOTNET_VERSION-linux-$rid.tar.gz"; \
    echo "$sha512  dotnet.tar.gz" | sha512sum -c -; \
    mkdir /opt/dotnet; \
    tar --no-same-owner -C /opt/dotnet -xzf dotnet.tar.gz; \
    mkdir -p "/opt/dotnet/metadata/workloads/${DOTNET_VERSION%??}00"; \
    touch "/opt/dotnet/metadata/workloads/${DOTNET_VERSION%??}00/userlocal"; \
    rm dotnet.tar.gz

FROM fetch AS dart-sdk

ARG TARGETARCH
ARG DART_VERSION=3.13.7
ARG DART_SHA256_AMD64=eb99aed266efbcb615c6ae21c12f52cfe935b9f208bccc5fea7f64c21e867229
ARG DART_SHA256_ARM64=887cbb763bc4a1d6f3d7c34a6ea08f0f05211807abf98c06d6e4263907ccac1a

RUN set -eu; \
    case "$TARGETARCH" in \
        amd64) arch=x64; sha256=$DART_SHA256_AMD64 ;; \
        arm64) arch=arm64; sha256=$DART_SHA256_ARM64 ;; \
        *) echo "No Dart SDK for TARGETARCH=$TARGETARCH" >&2; exit 1 ;; \
    esac; \
    curl -fsSL --proto '=https' --retry 3 -o dart.zip \
        "https://storage.googleapis.com/dart-archive/channels/stable/release/$DART_VERSION/sdk/dartsdk-linux-$arch-release.zip"; \
    echo "$sha256  dart.zip" | sha256sum -c -; \
    unzip -q dart.zip -d /opt; \
    rm dart.zip

# Flutter publishes no arm64 Linux archive, so both architectures check out the
# release commit. Its Dart SDK is the checksummed one above, which the runtime
# stage checks is the revision Flutter's DEPS pins, so Flutter never downloads
# an unverified SDK of its own.
FROM fetch AS flutter-sdk

ARG FLUTTER_VERSION=3.47.9
ARG FLUTTER_COMMIT=c197ca5199e9ee5661e865596c9c07294dfab51c

RUN set -eu; \
    git clone -q --depth 1 --branch "$FLUTTER_VERSION" https://github.com/flutter/flutter.git /opt/flutter; \
    test "$(git -C /opt/flutter rev-parse HEAD)" = "$FLUTTER_COMMIT"; \
    git -C /opt/flutter checkout -q -b stable; \
    mkdir /opt/flutter/bin/cache; \
    ln -s /opt/dart-sdk /opt/flutter/bin/cache/dart-sdk; \
    cp /opt/flutter/bin/internal/engine.version /opt/flutter/bin/cache/engine-dart-sdk.stamp

# Swift publishes PGP signatures, not checksums. These SHA-256s are of the
# tarballs whose signatures verify against the Swift 6 release signing key,
# 52BB7E3DE28A71BE22EC05FFEF80A866B47A981F.
FROM fetch AS swift-toolchain

ARG TARGETARCH
ARG SWIFT_VERSION=6.4.2
ARG SWIFT_SHA256_AMD64=8b6fde775b7cfc7a3758739d8abb09bb6fba519c6532d6b1a47faff791646c64
ARG SWIFT_SHA256_ARM64=bcbdd71d21421b9d18fc80f668b2b8dafefc84fa4ceba85edb0657c0c94bf36a

RUN set -eu; \
    case "$TARGETARCH" in \
        amd64) platform=debian13; sha256=$SWIFT_SHA256_AMD64 ;; \
        arm64) platform=debian13-aarch64; sha256=$SWIFT_SHA256_ARM64 ;; \
        *) echo "No Swift toolchain for TARGETARCH=$TARGETARCH" >&2; exit 1 ;; \
    esac; \
    release="swift-$SWIFT_VERSION-RELEASE"; \
    curl -fsSL --proto '=https' --retry 3 -o swift.tar.gz \
        "https://download.swift.org/swift-$SWIFT_VERSION-release/$platform/$release/$release-$platform.tar.gz"; \
    echo "$sha256  swift.tar.gz" | sha256sum -c -; \
    mkdir /opt/swift; \
    tar --no-same-owner --strip-components=1 -C /opt/swift -xzf swift.tar.gz; \
    chmod -R o+rX /opt/swift; \
    rm swift.tar.gz

FROM runtime-base AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config openssh-client gh jq ripgrep fd-find less file \
        procps unzip zip xz-utils bzip2 wget cmake ninja-build \
        python3 python3-pip python3-venv pipx nodejs npm postgresql-client \
    && ln -s /usr/bin/fdfind /usr/local/bin/fd \
    && rm -rf /var/lib/apt/lists/*

RUN apt-get update && apt-get install -y --no-install-recommends \
        php-cli php-mbstring php-xml php-curl php-zip php-intl php-sqlite3 \
        php-pgsql php-mysql php-gd php-bcmath \
    && rm -rf /var/lib/apt/lists/*

RUN apt-get update && apt-get install -y --no-install-recommends \
        ruby ruby-dev ruby-bundler \
    && rm -rf /var/lib/apt/lists/*

RUN apt-get update && apt-get install -y --no-install-recommends \
        openjdk-21-jdk-headless \
    && rm -rf /var/lib/apt/lists/*

RUN apt-get update && apt-get install -y --no-install-recommends \
        erlang-nox erlang-dev elixir \
    && rm -rf /var/lib/apt/lists/*

RUN apt-get update && apt-get install -y --no-install-recommends \
        binutils-gold libicu-dev libcurl4-openssl-dev libedit-dev libsqlite3-dev \
        libncurses-dev libpython3-dev libxml2-dev uuid-dev tzdata \
    && rm -rf /var/lib/apt/lists/*

# Shared libraries Playwright's browsers link against. Without them the
# browsers download and then refuse to launch, so an agent in this image can
# run the unit tests of a web project but never its end-to-end ones.
#
# The package list, the libraries it has to deliver, and the check that it did
# all live beside the script, so the manager-image workflow can run exactly
# this against the same base image on a pull request.
COPY manager/browser-packages.txt manager/browser-libraries.txt /opt/browser/
COPY manager/bin/install-browser-libraries.sh /opt/browser/install.sh
RUN sh /opt/browser/install.sh /opt/browser

COPY --from=rust-toolchain /opt/rust /opt/rust
COPY --from=go-toolchain /usr/local/go /usr/local/go
COPY --from=jvm /opt/jvm /opt/jvm
COPY --from=binaries /opt/bin/ /usr/local/bin/
COPY --from=dotnet-sdk /opt/dotnet /opt/dotnet
COPY --from=dart-sdk /opt/dart-sdk /opt/dart-sdk
COPY --from=swift-toolchain /opt/swift /opt/swift
COPY manager/rubygems/operating_system.rb /usr/local/lib/site_ruby/rubygems/defaults/operating_system.rb
COPY manager/bin/dotnet /usr/local/bin/dotnet
RUN mkdir -p /etc/dotnet && printf '/opt/dotnet\n' > /etc/dotnet/install_location
ENV PATH="$PATH:/opt/rust/bin:/usr/local/go/bin:/opt/jvm/gradle/bin:/opt/jvm/maven/bin:/opt/jvm/kotlinc/bin:/opt/dart-sdk/bin:/opt/swift/usr/bin"

COPY --from=bun /usr/local/bin/bun /usr/local/bin/bun
RUN ln -s /usr/local/bin/bun /usr/local/bin/bunx

RUN groupadd --system zone \
    && useradd --system --gid zone --create-home --home-dir /home/zone --shell /bin/bash zone \
    && mkdir -p /opt/tools/bin \
    && chown -R zone:zone /opt/tools \
    && printf 'prefix=/opt/tools\n' > /etc/npmrc \
    && printf '[global]\nbreak-system-packages = true\n' > /etc/pip.conf \
    && mkdir -p /etc/xdg \
    && printf '[ "$(id -u)" -ne 0 ] && PATH="$PATH:$HOME/.local/bin:$HOME/.bun/bin:$HOME/.cargo/bin:$HOME/go/bin:$HOME/.gem/bin:$HOME/.config/composer/vendor/bin:$HOME/.deno/bin:$HOME/.dotnet/tools:$HOME/.pub-cache/bin:/opt/flutter/bin:/opt/tools/bin"\n' > /etc/profile.d/tools.sh

# Flutter writes into its own SDK directory, so zone owns it and it is on
# ZONE_PATH only. Analytics are switched off for /home/zone, the HOME every
# agent runs with.
COPY --from=flutter-sdk --chown=zone:zone /opt/flutter /opt/flutter
RUN test "$(cat /opt/dart-sdk/revision)" = "$(sed -n "s/^ *'dart_revision': '\([0-9a-f]*\)',\$/\1/p" /opt/flutter/DEPS)" \
    && runuser -u zone -- env -i HOME=/home/zone PATH="$PATH:/opt/flutter/bin" sh -c '\
        dart --disable-analytics \
        && flutter --disable-analytics \
        && flutter config --no-cli-animations \
        && flutter precache --universal --web --linux --no-android --no-ios'

ENV SHELL=/bin/bash
# Docker's default seccomp profile blocks the user namespaces bubblewrap needs.
ENV ZONE_CODEX_SANDBOX=danger-full-access

COPY --from=agents /usr/local/bin/ /usr/local/bin/

WORKDIR /app

COPY --from=builder /build/bin/zone-server /app/zone-server
COPY --from=builder /build/bin/zone-runner /app/zone-runner
COPY --from=builder /build/runner/zone_server/migrations /app/migrations
COPY --from=builder /build/comfyui/workflows /app/comfyui/workflows
COPY --from=builder /build/comfyui/recipes /app/comfyui/recipes
COPY comfyui/poses /app/comfyui/poses
COPY comfyui/train_lora.py /app/comfyui/train_lora.py
COPY comfyui/custom_nodes/zone_lora/train_config.json /app/comfyui/custom_nodes/zone_lora/train_config.json

RUN chmod +x /app/zone-server /app/zone-runner

# Docker seeds a new named volume from the image directory, so create these
# paths up front to give first-time volumes zone's ownership. zone owns nothing
# else under /app, so neither the server nor an agent it runs can rewrite it.
RUN mkdir -p /app/workspace /app/agent-state /app/artifacts /app/embed-cache /tmp/repos \
    && chown -R zone:zone /app/workspace /app/agent-state /app/artifacts /app/embed-cache /tmp/repos \
    && chmod 0700 /app/agent-state /tmp/repos

COPY manager/docker-entrypoint.sh /usr/local/bin/manager-entrypoint.sh
RUN chmod 0755 /usr/local/bin/manager-entrypoint.sh

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD /usr/bin/curl -f http://localhost:8000/health || exit 1

ENTRYPOINT ["/usr/local/bin/manager-entrypoint.sh"]
CMD ["/app/zone-server"]
