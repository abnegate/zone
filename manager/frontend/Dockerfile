# =============================================================================
# Stage 1: Build shared UI package
# =============================================================================
FROM oven/bun:1-alpine AS ui-builder

WORKDIR /ui

COPY packages/ui/package.json ./
RUN bun install --frozen-lockfile || bun install

COPY packages/ui/ ./
RUN bun run build

# =============================================================================
# Stage 2: Install dependencies (cached layer)
# =============================================================================
FROM oven/bun:1-alpine AS deps

WORKDIR /app

COPY manager/frontend/package.json ./package.json.orig
RUN sed 's/"@zone\/ui": "\*",//' package.json.orig > package.json && \
    bun install --frozen-lockfile || bun install

# =============================================================================
# Stage 3: Build frontend application
# =============================================================================
FROM oven/bun:1-alpine AS builder

WORKDIR /app

COPY --from=deps /app/node_modules ./node_modules
COPY --from=deps /app/package.json ./

COPY --from=ui-builder /ui/dist /ui/dist
COPY --from=ui-builder /ui/package.json /ui/package.json

RUN bun add /ui

COPY manager/frontend/tsconfig.json manager/frontend/vite.config.ts manager/frontend/index.html ./

COPY manager/frontend/src ./src

RUN bun run build

# =============================================================================
# Stage 4: Production runtime with bun
# =============================================================================
FROM oven/bun:1-alpine AS runtime

WORKDIR /app

COPY --from=builder /app/build ./build

RUN chown -R bun:bun /app

USER bun

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://127.0.0.1:3000/ || exit 1

CMD ["bun", "x", "serve", "-s", "build", "-l", "3000"]
