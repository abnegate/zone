# ComfyUI and FLUX.1 Schnell

Zone supports a pinned ComfyUI runtime with the single-file FLUX.1 Schnell FP8
checkpoint. The runtime is native on Apple Silicon and an optional NVIDIA
Compose profile on Linux.

The checkpoint is **not** downloaded during a build or normal startup. Model
setup is an explicit operation and verifies both the exact byte count and
SHA-256 before the file is accepted.

## Pinned artifacts

- ComfyUI commit: `30bdda1ef13a3a34fce2cd2fec633f15d832122a`
- Model repository: `Comfy-Org/flux1-schnell`
- Model revision: `1b9a9220e849bf08835aaef57e1ce6723db32bf0`
- File: `flux1-schnell-fp8.safetensors`
- Size: `17,236,328,572` bytes (approximately 16.05 GiB / 17.24 GB)
- SHA-256: `ead426278b49030e9da5df862994f25ce94ab2ee4df38b556ddddb3db093bf72`
- Model license: Apache-2.0
- CUDA base image manifest:
  `sha256:14d94b039cb94bbd5da559f303b46bc4b0d5d6c24ab1a9d7b186e566ed3400dc`

The machine-readable source of truth is `comfyui/model-manifest.json`.
Third-party attribution is in `comfyui/NOTICE.md`.
Python dependencies are fully resolved and hash-verified in the platform
specific `comfyui/requirements*.lock` files.

## Apple Silicon macOS

### Requirements

- Apple Silicon (arm64); Intel Macs are not supported by this installer
- Python 3.11 through 3.13, running as arm64
- Git / Xcode Command Line Tools
- At least 25 GB free disk space
- 32 GB unified memory recommended; 24 GB may work with memory pressure and
  substantially lower resolutions

Install the pinned runtime without downloading model weights:

```bash
make setup-comfyui-macos
```

Download the checkpoint only when ready:

```bash
./scripts/setup-comfyui-macos.sh --download-model
```

An interrupted download is retained as a `.part` file and resumes on the next
run. Verify an existing checkpoint without network access:

```bash
./scripts/setup-comfyui-macos.sh --verify-model
```

Start the pinned runtime:

```bash
cd "$HOME/Library/Application Support/Zone/ComfyUI"
.venv/bin/python main.py \
  --listen 127.0.0.1 \
  --port 8188 \
  --disable-auto-launch \
  --force-fp16
```

`--force-fp16` keeps compute on the broadly supported MPS path while the FP8
checkpoint retains its smaller storage and memory footprint. ComfyUI has no
built-in authentication, so it must stay bound to loopback.

First test whether Docker Desktop can reach the loopback listener:

```bash
docker run --rm curlimages/curl:8.16.0 \
  -fsS http://host.docker.internal:8188/system_stats
```

If that succeeds, use `http://host.docker.internal:8188` directly. If it does
not, run Zone's authenticated bridge in a separate terminal:

```bash
export COMFYUI_BRIDGE_TOKEN="$(openssl rand -hex 32)"
python3 scripts/comfyui-loopback-bridge.py
```

The bridge is the only process that listens beyond loopback, accepts only
GET/POST, forwards solely to `127.0.0.1:8188`, limits request bodies, and
requires a constant-time checked token. Put the same token in `.env`:

```dotenv
COMFYUI_BASE_URL=http://host.docker.internal:8189
COMFYUI_API_TOKEN=<value of COMFYUI_BRIDGE_TOKEN>
```

After the runtime and checkpoint are ready, enable routing in `.env`:

```dotenv
COMFYUI_ENABLED=true
COMFYUI_BASE_URL=http://host.docker.internal:8188
```

Override installation paths when necessary:

```bash
COMFYUI_INSTALL_DIR="$HOME/Applications/ComfyUI-Zone" \
COMFYUI_MODELS_DIR="/Volumes/Models/ComfyUI/models" \
./scripts/setup-comfyui-macos.sh --download-model
```

Use the same overrides for later verification and startup.

## Bundled NVIDIA Compose profile

### Requirements

- Linux host with a CUDA-capable NVIDIA GPU
- Current NVIDIA driver compatible with CUDA 13
- NVIDIA Container Toolkit configured for Docker
- At least 24 GB VRAM recommended
- At least 25 GB free Docker volume storage

Set the manager's internal endpoint in `.env`:

```dotenv
COMFYUI_ENABLED=true
COMFYUI_BASE_URL=http://comfyui:8188
```

Explicitly download and verify the model into the persistent
`zone_comfyui_models` volume:

```bash
make setup-comfyui-model
make verify-comfyui-model
```

If verification reports that an existing final file is invalid, inspect the
storage first. To deliberately replace it:

```bash
make setup-comfyui-model FORCE=1
```

Build and start the GPU runtime:

```bash
make up-comfyui
make up
```

The `comfyui` service joins only the private `zone_internal` network and
publishes no host port. It is therefore reachable by the manager as
`http://comfyui:8188`, but not through Traefik or the host. The one-shot model
setup container is separately gated behind the `comfyui-model-setup` profile
because it requires internet access.

Check readiness from inside the private network:

```bash
docker inspect --format '{{.State.Health.Status}}' comfyui
docker compose --profile bundled-comfyui logs comfyui
```

## Workflow contract

`comfyui/workflows/flux1-schnell-fp8-api.json` is the text-to-image API-format
workflow. It uses only built-in ComfyUI nodes. Integration code may replace
only these inputs:

- node `6`: positive prompt text
- node `4`: checkpoint filename from trusted server configuration
- node `5`: width, height, and batch size
- node `3`: seed, steps, CFG, sampler, scheduler, and denoise
- node `9`: temporary `PreviewImage` output (persistent `SaveImage` is rejected)

`comfyui/workflows/flux1-schnell-fp8-img2img-api.json` is the image-to-image
sibling. Chat uses it when the current message includes an image attachment
(a data URL or a same-chat `/api/artifacts/...` URL). The manager uploads
that image to ComfyUI's input folder, then may replace only:

- node `6`: positive prompt text
- node `4`: checkpoint filename from trusted server configuration
- node `3`: seed (steps, CFG, sampler, scheduler, and denoise stay packaged)
- node `10`: uploaded source filename
- node `9`: temporary `PreviewImage` output (persistent `SaveImage` is rejected)

Node `11` scales the source to 1024×1024 with a centered crop. Node `12`
VAE-encodes it. Packaged denoise is `0.75` so Schnell's four Euler/simple
steps still transform the source instead of ignoring it.

The packaged defaults are Schnell-appropriate: four Euler/simple steps and CFG
1. Node `4` defaults to the manifest filename and is never changed from
untrusted request data. Zone copies successful temporary output into its
protected artifact store and clears the ComfyUI history entry; cancelled
running jobs remain only in ComfyUI's temporary lifecycle. Remote `http(s)`
attachment URLs are never fetched as img2img sources.

## Troubleshooting

- **Model verification fails immediately:** the named volume or macOS model
  directory is empty. Run the explicit model setup command.
- **CUDA device unavailable:** confirm `nvidia-smi` works on the host and
  `docker run --rm --gpus all nvidia/cuda:13.0.2-base-ubuntu24.04 nvidia-smi`
  works before starting the profile.
- **macOS manager cannot connect:** confirm ComfyUI responds at
  `http://127.0.0.1:8188/system_stats`, then use the direct Docker Desktop test
  and authenticated bridge procedure above. Do not bind ComfyUI itself to
  `0.0.0.0`.
- **MPS out of memory:** close GPU-heavy applications and reduce workflow width
  and height. Do not add `--cpu` unless very slow CPU generation is acceptable.
