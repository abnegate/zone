ARG CUDA_IMAGE=nvidia/cuda:13.0.2-cudnn-runtime-ubuntu24.04

FROM ${CUDA_IMAGE} AS builder

ARG COMFYUI_COMMIT=30bdda1ef13a3a34fce2cd2fec633f15d832122a
ARG TORCH_VERSION=2.9.1
ARG TORCHVISION_VERSION=0.24.1
ARG TORCHAUDIO_VERSION=2.9.1

ENV DEBIAN_FRONTEND=noninteractive \
    PIP_DISABLE_PIP_VERSION_CHECK=1

RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        ca-certificates \
        git \
        python3 \
        python3-venv \
    && rm -rf /var/lib/apt/lists/*

RUN git init /opt/ComfyUI \
    && git -C /opt/ComfyUI remote add origin https://github.com/comfyanonymous/ComfyUI.git \
    && git -C /opt/ComfyUI fetch --depth 1 origin "${COMFYUI_COMMIT}" \
    && git -C /opt/ComfyUI checkout --detach FETCH_HEAD \
    && test "$(git -C /opt/ComfyUI rev-parse HEAD)" = "${COMFYUI_COMMIT}" \
    && rm -rf /opt/ComfyUI/.git

RUN python3 -m venv /opt/venv \
    && /opt/venv/bin/pip install --no-cache-dir --upgrade pip \
    && /opt/venv/bin/pip install --no-cache-dir \
        --index-url https://download.pytorch.org/whl/cu130 \
        "torch==${TORCH_VERSION}" \
        "torchvision==${TORCHVISION_VERSION}" \
        "torchaudio==${TORCHAUDIO_VERSION}" \
    && /opt/venv/bin/pip install --no-cache-dir -r /opt/ComfyUI/requirements.txt

FROM ${CUDA_IMAGE}

ARG COMFYUI_COMMIT=30bdda1ef13a3a34fce2cd2fec633f15d832122a

LABEL org.opencontainers.image.title="Zone ComfyUI runtime" \
      org.opencontainers.image.description="Pinned ComfyUI runtime for FLUX.1 Schnell FP8" \
      org.opencontainers.image.source="https://github.com/comfyanonymous/ComfyUI" \
      org.opencontainers.image.revision="${COMFYUI_COMMIT}" \
      org.opencontainers.image.licenses="GPL-3.0"

ENV DEBIAN_FRONTEND=noninteractive \
    PATH=/opt/venv/bin:${PATH} \
    PYTHONUNBUFFERED=1 \
    NVIDIA_VISIBLE_DEVICES=all \
    NVIDIA_DRIVER_CAPABILITIES=compute,utility

RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        ca-certificates \
        libgl1 \
        libglib2.0-0 \
        python3 \
        tini \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 --shell /usr/sbin/nologin comfy

COPY --from=builder /opt/venv /opt/venv
COPY --from=builder --chown=comfy:comfy /opt/ComfyUI /workspace/ComfyUI
COPY --chown=comfy:comfy model-manifest.json download-models.py /opt/zone/

RUN mkdir -p \
        /workspace/ComfyUI/models/checkpoints \
        /workspace/ComfyUI/output \
        /workspace/ComfyUI/temp \
        /workspace/ComfyUI/user \
    && chown -R comfy:comfy /workspace/ComfyUI /opt/zone

USER comfy
WORKDIR /workspace/ComfyUI
EXPOSE 8188

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["python", "main.py", "--listen", "0.0.0.0", "--port", "8188", "--disable-auto-launch"]
