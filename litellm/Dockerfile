# LiteLLM converts images with Pillow before handing them to Ollama, but the
# published image does not ship Pillow and its venv has no pip or uv to add it
# at runtime. Without this, any message carrying an image fails with
# "ollama image conversion failed please run `pip install Pillow`".
#
# The wheel is built against the same Python and platform as the base image and
# copied into its site-packages, so vision requests route through the same
# gateway as text rather than needing a second path to Ollama.
ARG LITELLM_VERSION=v1.99.1
ARG LITELLM_DIGEST=sha256:a53a7d3ffebede1925bd3ee8a21e4a7b9b63e2e68ec883af136edcccb6eeb82c

FROM python:3.13.15-slim@sha256:9d2e5553305c7c7b0097999bb17187c69b921ccd6bc9d40e4bb5ebe652c00285 AS pillow
ARG PILLOW_VERSION=12.3.0
RUN pip install --no-cache-dir --target=/pillow "Pillow==${PILLOW_VERSION}"

FROM ghcr.io/berriai/litellm:${LITELLM_VERSION}@${LITELLM_DIGEST}
COPY --from=pillow /pillow/ /app/.venv/lib/python3.13/site-packages/
