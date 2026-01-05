# =============================================================================
# Manager Frontend (Console) Dockerfile
# Multi-stage build with shared UI package
#
# Build context must be repository root:
#   docker build -f manager/frontend/Dockerfile .
# =============================================================================

# =============================================================================
# Stage 1: Build shared UI package
# =============================================================================
FROM node:20-alpine AS ui-builder

WORKDIR /ui

# Copy UI package files (no lock file in workspace packages)
COPY packages/ui/package.json ./
RUN npm install

COPY packages/ui/ ./
RUN npm run build

# =============================================================================
# Stage 2: Build frontend application
# =============================================================================
FROM node:20-alpine AS builder

WORKDIR /app

# Copy package files (without @zone/ui dependency for initial install)
COPY manager/frontend/package.json manager/frontend/package-lock.json ./

# Create a temporary package.json without the workspace dependency and install
RUN sed -i 's/"@zone\/ui": "\*",//' package.json && npm install

# Now copy the built UI package and link it
COPY --from=ui-builder /ui/dist /ui/dist
COPY --from=ui-builder /ui/package.json /ui/package.json

# Install @zone/ui from local path
RUN npm install /ui --legacy-peer-deps

# Copy source and build
COPY manager/frontend/ ./
RUN npm run build

# =============================================================================
# Stage 3: Production runtime with nginx
# =============================================================================
FROM nginx:alpine AS runtime

# Copy custom nginx config
COPY manager/frontend/nginx.conf /etc/nginx/conf.d/default.conf

# Copy built assets from builder
COPY --from=builder /app/build /usr/share/nginx/html

# Create non-root user
RUN adduser -D -u 1000 appuser && \
    chown -R appuser:appuser /usr/share/nginx/html && \
    chown -R appuser:appuser /var/cache/nginx && \
    chown -R appuser:appuser /var/log/nginx && \
    touch /var/run/nginx.pid && \
    chown -R appuser:appuser /var/run/nginx.pid

USER appuser

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:3000/ || exit 1

CMD ["nginx", "-g", "daemon off;"]
