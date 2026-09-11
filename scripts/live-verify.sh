#!/bin/sh
# Drive the console against a real server, headless.
#
# The mocked Playwright suite forges its own JWT and answers for the API, so it
# can pass while nothing works. This brings up a real `zone-server` against real
# Postgres, Valkey and Ollama, registers two real tenants, and runs
# `manager/frontend/live` against the console the browser actually loads.
#
#   ./scripts/live-verify.sh                 # bring the rig up and run the suite
#   ./scripts/live-verify.sh live/train      # a subset, passed to Playwright
#   ZONE_LIVE_KEEP=1 ./scripts/live-verify.sh   # leave the rig running
#
# With ZONE_LIVE_REAL_MODELS=1 the stand-in and the placeholder weights are
# not used: the server talks to a real ComfyUI (ZONE_LIVE_COMFYUI_URL,
# default 127.0.0.1:8188) holding the weights `setup-comfyui-macos.sh` installs,
# trains through it rather than the stand-in trainer, and waits as long as a
# real render takes. Start that ComfyUI before the rig.
#
#   ZONE_LIVE_REAL_MODELS=1 ZONE_TRAIN_CLIP=subject.mp4 \
#     ./scripts/live-verify.sh live/real-media.live.ts live/real-train.live.ts
#
# Two checks drive a real agent end to end and are skipped unless
# ZONE_LIVE_AGENT_MODEL names a model that reliably calls tools, because whether
# a model reaches for one is the model's decision and not a property of this
# console. The rendering they exercise is pinned by unit tests either way.
#
# Required: DATABASE_URL, REDIS_URL, JWT_SECRET, ENCRYPTION_KEY. A migrated
# database is not required — the server migrates on startup.
#
# Subject-aware training crops need the U2-Net weights `make vision-model`
# fetches; without them the server says so and centres the crop instead.
#
# ComfyUI is replaced by a stand-in that speaks the real /prompt, /history,
# /view protocol and returns real PNG, WebM and FLAC bytes. It runs no weights,
# so nothing here claims anything about a model; every line of the server's own
# ComfyUI path does run, and each lane is asserted byte-exact against the
# fixture it should have collected.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
work=${ZONE_LIVE_WORK:-${TMPDIR:-/tmp}/zone-live-verify}
port=${ZONE_LIVE_PORT:-4179}
api_port=${ZONE_LIVE_API_PORT:-8010}
stub_port=${ZONE_LIVE_STUB_PORT:-8188}
api="http://127.0.0.1:$api_port"
real=${ZONE_LIVE_REAL_MODELS:-}
models="$work/models"
comfy_url="http://127.0.0.1:$stub_port"
train_command="$root/scripts/live-verify/stand-in-trainer.sh"
generation_timeout=300
video_timeout=600
audio_timeout=600
upscale_timeout=600
if [ "$real" = 1 ]; then
  models=${COMFYUI_MODELS_DIR:-"$HOME/Library/Application Support/Zone/ComfyUI/models"}
  comfy_url=${ZONE_LIVE_COMFYUI_URL:-$comfy_url}
  train_command=
  generation_timeout=900
  video_timeout=3600
  audio_timeout=3600
  upscale_timeout=1800
fi

: "${DATABASE_URL:?set DATABASE_URL to a Postgres the server may migrate}"
: "${REDIS_URL:?set REDIS_URL}"
: "${JWT_SECRET:?set JWT_SECRET}"
: "${ENCRYPTION_KEY:?set ENCRYPTION_KEY}"

mkdir -p "$work"
pids=""

stop() {
  status=$?
  if [ -n "${ZONE_LIVE_KEEP:-}" ]; then
    echo "rig left running: console $port, api $api_port, comfy stand-in $stub_port"
    exit "$status"
  fi
  for pid in $pids; do
    kill "$pid" 2>/dev/null || true
  done
  exit "$status"
}
trap stop EXIT INT TERM

wait_for() {
  name=$1
  url=$2
  tries=0
  while [ "$tries" -lt 120 ]; do
    if curl -sf -o /dev/null --max-time 2 "$url"; then
      return 0
    fi
    tries=$((tries + 1))
    sleep 1
  done
  echo "$name never became ready at $url" >&2
  return 1
}

echo "==> fixtures"
"$root/scripts/live-verify/fixtures.sh" "$work/fixtures" >/dev/null

if [ "$real" = 1 ]; then
  echo "==> real models in $models"
else
  # A placeholder weight per required file is enough for the recipe catalog to
  # report a trainable base: readiness is a file that exists, and the Train tab
  # needs a base to offer.
  echo "==> models"
  mkdir -p "$models/checkpoints" "$models/diffusion_models" "$models/loras" \
    "$models/text_encoders" "$models/vae" "$models/upscale_models"
  printf placeholder >"$models/checkpoints/flux1-schnell-fp8.safetensors"
  printf placeholder >"$models/diffusion_models/qwen_image_edit_2511_fp8mixed.safetensors"
  printf placeholder >"$models/text_encoders/qwen_2.5_vl_7b_fp8_scaled.safetensors"
  printf placeholder >"$models/vae/qwen_image_vae.safetensors"
  printf placeholder >"$models/upscale_models/RealESRGAN_x4plus.safetensors"
fi

if [ "$real" = 1 ]; then
  echo "==> real comfyui at $comfy_url"
  wait_for comfyui "$comfy_url/system_stats"
else
  echo "==> comfyui stand-in on $stub_port"
  ZONE_COMFY_FIXTURES="$work/fixtures/media" COMFY_STUB_PORT="$stub_port" \
    python3 "$root/scripts/live-verify/comfy-stub.py" >"$work/comfy-stub.log" 2>&1 &
  pids="$pids $!"
fi

echo "==> server on $api_port"
(cd "$root/runner" && SQLX_OFFLINE=true cargo build --quiet -p zone_server)
HOST=127.0.0.1 \
PORT="$api_port" \
CORS_ORIGINS="http://localhost:$port,http://127.0.0.1:$port" \
APP_BASE_URL="http://localhost:$port" \
ARTIFACT_ROOT="$work/artifacts" \
LITELLM_HOST="${LITELLM_HOST:-http://127.0.0.1:11434/v1}" \
LITELLM_KEY="${LITELLM_KEY:-live-verify}" \
OLLAMA_HOST="${OLLAMA_HOST:-http://127.0.0.1:11434}" \
OLLAMA_MODEL_FAST="${OLLAMA_MODEL_FAST:-llama3.2:3b}" \
OLLAMA_MODEL_REASON="${OLLAMA_MODEL_REASON:-llama3.2:3b}" \
OLLAMA_MODEL_EMBED="${OLLAMA_MODEL_EMBED:-nomic-embed-text}" \
OLLAMA_MODEL_VISION="${OLLAMA_MODEL_VISION:-llava:7b}" \
COMFYUI_ENABLED=true \
COMFYUI_BASE_URL="$comfy_url" \
COMFYUI_WORKFLOW_PATH="$root/comfyui/workflows/flux1-schnell-fp8-api.json" \
COMFYUI_CHECKPOINT=flux1-schnell-fp8.safetensors \
COMFYUI_MODELS_DIR="$models" \
COMFYUI_GENERATION_TIMEOUT_SECS="$generation_timeout" \
COMFYUI_VIDEO_GENERATION_TIMEOUT_SECS="$video_timeout" \
COMFYUI_AUDIO_GENERATION_TIMEOUT_SECS="$audio_timeout" \
COMFYUI_UPSCALE_GENERATION_TIMEOUT_SECS="$upscale_timeout" \
COMFYUI_CLASSIFIER_MODEL="${OLLAMA_MODEL_FAST:-llama3.2:3b}" \
COMFYUI_CAPTION_MODEL="${OLLAMA_MODEL_VISION:-llava:7b}" \
COMFYUI_CLASSIFIER_TIMEOUT_SECS=20 \
COMFYUI_POLL_INTERVAL_MS=200 \
COMFYUI_UPSCALE_MODEL=RealESRGAN_x4plus.safetensors \
COMFYUI_TRAIN_COMMAND="$train_command" \
ZONE_VISION_MODEL="${ZONE_VISION_MODEL:-$root/runner/zone_vision/models/u2net.onnx}" \
MONITORING_ENABLED=false \
ZONE_MCP_ENABLED=false \
SQLX_OFFLINE=true \
RUST_LOG="${RUST_LOG:-zone_server=info}" \
  "$root/runner/target/debug/zone-server" >"$work/server.log" 2>&1 &
pids="$pids $!"
wait_for server "$api/health"

echo "==> tenants"
state="$work/state.json"
ZONE_API="$api" ZONE_LIVE_STATE="$state" \
  python3 "$root/scripts/live-verify/seed-tenants.py" >"$work/tenants.json"

echo "==> console on $port"
(cd "$root/manager/frontend" && VITE_PROXY_TARGET="$api" \
  exec bun run vite --port "$port" --strictPort >"$work/console.log" 2>&1) &
pids="$pids $!"
wait_for console "http://localhost:$port/"

echo "==> live suite"
cd "$root/manager/frontend"
ZONE_LIVE_STATE="$state" \
ZONE_LIVE_PORT="$port" \
ZONE_LIVE_AGENT_MODEL="${ZONE_LIVE_AGENT_MODEL:-}" \
ZONE_COMFY_STUB="http://127.0.0.1:$stub_port" \
ZONE_COMFY_FIXTURES="$work/fixtures/media" \
ZONE_TRAIN_FIXTURES="$work/fixtures/training" \
  bunx playwright test --config playwright.live.config.ts "$@"
