# LiteLLM converts images with Pillow before handing them to Ollama, but the
# published image does not ship Pillow and its venv has no pip or uv to add it
# at runtime. Without this, any message carrying an image fails with
# "ollama image conversion failed please run `pip install Pillow`".
#
# The wheel is built against the same Python and platform as the base image and
# copied into its site-packages, so vision requests route through the same
# gateway as text rather than needing a second path to Ollama.
ARG LITELLM_VERSION=v1.99.1

FROM python:3.13-slim AS pillow
ARG PILLOW_VERSION=12.3.0
RUN pip install --no-cache-dir --target=/pillow "Pillow==${PILLOW_VERSION}"

FROM ghcr.io/berriai/litellm:${LITELLM_VERSION}
COPY --from=pillow /pillow/ /app/.venv/lib/python3.13/site-packages/
