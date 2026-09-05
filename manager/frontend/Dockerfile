# =============================================================================
# Stage 1: Install the locked workspace and build the frontend
# =============================================================================
FROM oven/bun:1.4.0-alpine@sha256:07235578f79ef8c6f97d94aee7938e76f5cdba5f21ae5dbfdd3d3d38058437eb AS builder

WORKDIR /workspace

COPY --chown=bun:bun package.json bun.lock ./
COPY --chown=bun:bun packages/ui/package.json packages/ui/package.json
COPY --chown=bun:bun manager/frontend/package.json manager/frontend/package.json
RUN bun install --frozen-lockfile

COPY --chown=bun:bun packages/ui packages/ui
COPY --chown=bun:bun manager/frontend manager/frontend
RUN bun run --filter @zone/ui build && bun run --filter zone build

# =============================================================================
# Stage 2: Production runtime with bun
# =============================================================================
FROM oven/bun:1.4.0-alpine@sha256:07235578f79ef8c6f97d94aee7938e76f5cdba5f21ae5dbfdd3d3d38058437eb AS runtime

WORKDIR /app

COPY --from=builder /workspace/manager/frontend/build ./build

RUN chown -R bun:bun /app

USER bun

EXPOSE 3001

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://127.0.0.1:3001/ || exit 1

CMD ["bun", "x", "serve@14.2.6", "-s", "build", "-l", "3001"]
