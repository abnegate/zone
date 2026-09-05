#!/bin/sh
set -eu

COMFYUI_COMMIT="30bdda1ef13a3a34fce2cd2fec633f15d832122a"
PIP_VERSION="25.3"

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
PROJECT_DIR=$(dirname "$SCRIPT_DIR")
INSTALL_DIR=${COMFYUI_INSTALL_DIR:-"$HOME/Library/Application Support/Zone/ComfyUI"}
MODELS_DIR=${COMFYUI_MODELS_DIR:-"$INSTALL_DIR/models"}
PYTHON=${PYTHON_BIN:-python3}
MODEL_ACTION=none
MODEL_BUNDLE=image

usage() {
    cat <<EOF
Usage: $0 [--download-model | --download-video-model | --verify-model | --verify-video-model] [--force-model]

Install the pinned native Apple Silicon ComfyUI runtime. Image weights are
downloaded only when --download-model is supplied. Video weights are a
separate explicit download.

Environment:
  COMFYUI_INSTALL_DIR  Runtime directory (default: $INSTALL_DIR)
  COMFYUI_MODELS_DIR   Model directory (default: <runtime>/models)
  PYTHON_BIN           Python 3.11-3.13 executable (default: python3)
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --download-model) MODEL_ACTION=download; MODEL_BUNDLE=image ;;
        --download-video-model) MODEL_ACTION=download; MODEL_BUNDLE=video ;;
        --verify-model) MODEL_ACTION=verify; MODEL_BUNDLE=image ;;
        --verify-video-model) MODEL_ACTION=verify; MODEL_BUNDLE=video ;;
        --force-model) MODEL_FORCE=1 ;;
        --help|-h) usage; exit 0 ;;
        *) echo "Unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
    shift
done

if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
    echo "This installer supports Apple Silicon macOS only." >&2
    exit 1
fi

if ! command -v git >/dev/null 2>&1; then
    echo "git is required. Install the Xcode Command Line Tools first." >&2
    exit 1
fi
if ! command -v "$PYTHON" >/dev/null 2>&1; then
    echo "$PYTHON was not found. Install Python 3.11-3.13." >&2
    exit 1
fi

"$PYTHON" - <<'PY'
import platform
import sys

if not ((3, 11) <= sys.version_info[:2] < (3, 14)):
    raise SystemExit("Python 3.11-3.13 is required")
if platform.machine() != "arm64":
    raise SystemExit("Python must be an arm64 build, not a Rosetta/x86_64 build")
PY

if [ "$MODEL_ACTION" = "verify" ]; then
    exec "$PYTHON" "$PROJECT_DIR/comfyui/download-models.py" \
        --models-dir "$MODELS_DIR" --bundle "$MODEL_BUNDLE" --verify-only
fi

mkdir -p "$(dirname "$INSTALL_DIR")"
if [ ! -d "$INSTALL_DIR/.git" ]; then
    if [ -e "$INSTALL_DIR" ] && [ -n "$(ls -A "$INSTALL_DIR" 2>/dev/null)" ]; then
        echo "Install directory exists and is not a ComfyUI checkout: $INSTALL_DIR" >&2
        exit 1
    fi
    git init "$INSTALL_DIR"
    git -C "$INSTALL_DIR" remote add origin https://github.com/comfyanonymous/ComfyUI.git
fi

git -C "$INSTALL_DIR" fetch --depth 1 origin "$COMFYUI_COMMIT"
git -C "$INSTALL_DIR" checkout --detach "$COMFYUI_COMMIT"
if [ "$(git -C "$INSTALL_DIR" rev-parse HEAD)" != "$COMFYUI_COMMIT" ]; then
    echo "ComfyUI checkout verification failed." >&2
    exit 1
fi

if [ ! -x "$INSTALL_DIR/.venv/bin/python" ]; then
    "$PYTHON" -m venv "$INSTALL_DIR/.venv"
fi

VENV_PYTHON="$INSTALL_DIR/.venv/bin/python"
"$VENV_PYTHON" -m pip install --disable-pip-version-check --upgrade "pip==$PIP_VERSION"
"$VENV_PYTHON" -m pip install --disable-pip-version-check \
    --require-hashes -r "$PROJECT_DIR/comfyui/requirements-macos.lock"

mkdir -p \
    "$MODELS_DIR/checkpoints" \
    "$MODELS_DIR/diffusion_models" \
    "$MODELS_DIR/text_encoders" \
    "$MODELS_DIR/vae" \
    "$INSTALL_DIR/models" \
    "$INSTALL_DIR/output"
if [ "$MODELS_DIR" != "$INSTALL_DIR/models" ]; then
    for folder in checkpoints diffusion_models text_encoders vae; do
        LINK="$INSTALL_DIR/models/$folder"
        if [ -d "$LINK" ] && [ ! -L "$LINK" ] \
            && [ -n "$(ls -A "$LINK" 2>/dev/null)" ]; then
            echo "Default $folder directory is not empty: $LINK" >&2
            exit 1
        fi
        rm -rf "$LINK"
        ln -s "$MODELS_DIR/$folder" "$LINK"
    done
fi

echo "Installed ComfyUI $COMFYUI_COMMIT at: $INSTALL_DIR"
echo "Model directory: $MODELS_DIR"

if [ "$MODEL_ACTION" = "download" ]; then
    set -- "$VENV_PYTHON" "$PROJECT_DIR/comfyui/download-models.py" \
        --models-dir "$MODELS_DIR" --bundle "$MODEL_BUNDLE"
    if [ "${MODEL_FORCE:-0}" = "1" ]; then
        set -- "$@" --force
    fi
    exec "$@"
fi

echo "Model download skipped. Run with --download-model when ready."
