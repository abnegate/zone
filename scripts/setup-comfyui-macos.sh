#!/bin/sh
set -eu

COMFYUI_COMMIT="30bdda1ef13a3a34fce2cd2fec633f15d832122a"
PIP_VERSION="25.3"

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
PROJECT_DIR=$(dirname "$SCRIPT_DIR")
MANIFEST="$PROJECT_DIR/comfyui/model-manifest.json"
INSTALL_DIR=${COMFYUI_INSTALL_DIR:-"$HOME/Library/Application Support/Zone/ComfyUI"}
MODELS_DIR=${COMFYUI_MODELS_DIR:-"$INSTALL_DIR/models"}
PYTHON=${PYTHON_BIN:-python3}
MODEL_ACTION=none
MODEL_BUNDLE=image
MANIFEST_BUNDLES=
APPLY_NODES_ONLY=0
INSTALL_TRAINER=0

require_python() {
    if ! command -v "$PYTHON" >/dev/null 2>&1; then
        echo "$PYTHON was not found. Install Python 3.11-3.13." >&2
        exit 1
    fi
}

load_bundles() {
    if [ -z "$MANIFEST_BUNDLES" ]; then
        require_python
        MANIFEST_BUNDLES=$("$PYTHON" - "$MANIFEST" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    models = json.load(handle)["models"]
print(" ".join(dict.fromkeys(model.get("bundle") or "image" for model in models)))
PY
)
    fi
}

select_bundle() {
    load_bundles
    for bundle in $MANIFEST_BUNDLES; do
        if [ "$bundle" = "$1" ]; then
            MODEL_BUNDLE=$1
            return 0
        fi
    done
    echo "Unknown bundle: $1" >&2
    echo "Valid bundles: $MANIFEST_BUNDLES" >&2
    exit 2
}

usage() {
    load_bundles
    cat <<EOF
Usage: $0 [--download-model | --verify-model] [--bundle NAME] [--force-model]
       $0 --apply-nodes
       $0 --install-trainer

Install the pinned native Apple Silicon ComfyUI runtime. Weights are downloaded
only when --download-model is supplied, and only for the selected bundle.
--apply-nodes copies the packaged Zone LoRA custom node onto an existing
checkout without fetching ComfyUI again. --install-trainer copies the host
SDXL trainer onto an existing checkout, creates its venv, and loads the
LaunchAgent; it does not reinstall ComfyUI.

Options:
  --bundle NAME           Bundle to act on: $MANIFEST_BUNDLES
  --download-model        Download the selected bundle (default: image)
  --verify-model          Verify the selected bundle without downloading
  --download-video-model  Alias for --download-model --bundle video
  --verify-video-model    Alias for --verify-model --bundle video
  --force-model           Replace an installed file that fails verification
  --apply-nodes           Copy the Zone LoRA node onto an existing checkout
  --install-trainer       Install the host SDXL trainer LaunchAgent onto an existing checkout

Bundles are selected left to right, so the last of --bundle and any bundle
alias on the command line wins.

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
        --bundle)
            if [ "$#" -lt 2 ]; then
                echo "--bundle requires a bundle name." >&2
                usage >&2
                exit 2
            fi
            select_bundle "$2"
            shift
            ;;
        --apply-nodes) APPLY_NODES_ONLY=1 ;;
        --install-trainer) INSTALL_TRAINER=1 ;;
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

apply_zone_nodes() {
    mkdir -p "$INSTALL_DIR/custom_nodes"
    if [ -d "$INSTALL_DIR/.git" ]; then
        git -C "$INSTALL_DIR" checkout HEAD -- \
            comfy_extras/nodes_train.py \
            comfy/weight_adapter/bypass.py \
            >/dev/null 2>&1 || true
    fi
    rm -rf "$INSTALL_DIR/custom_nodes/zone_lora"
    cp -R "$PROJECT_DIR/comfyui/custom_nodes/zone_lora" "$INSTALL_DIR/custom_nodes/zone_lora"
    echo "Applied Zone LoRA nodes to $INSTALL_DIR/custom_nodes/zone_lora"
}

escape_xml() {
    printf '%s' "$1" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g'
}

install_trainer() {
    if [ ! -d "$INSTALL_DIR" ]; then
        echo "ComfyUI is not installed at $INSTALL_DIR" >&2
        exit 1
    fi
    if [ ! -x "$INSTALL_DIR/.venv/bin/python" ]; then
        echo "ComfyUI venv is missing at $INSTALL_DIR/.venv" >&2
        exit 1
    fi
    cp "$PROJECT_DIR/comfyui/train_sdxl.py" "$INSTALL_DIR/train_sdxl.py"
    cp "$PROJECT_DIR/comfyui/train_sdxl_config.json" "$INSTALL_DIR/train_sdxl_config.json"
    if [ -f "$PROJECT_DIR/comfyui/sdxl_checkpoint.py" ]; then
        cp "$PROJECT_DIR/comfyui/sdxl_checkpoint.py" "$INSTALL_DIR/sdxl_checkpoint.py"
    fi
    if [ ! -x "$INSTALL_DIR/.venv-train/bin/python" ]; then
        "$INSTALL_DIR/.venv/bin/python" -m venv --system-site-packages "$INSTALL_DIR/.venv-train"
    fi
    TRAIN_PYTHON="$INSTALL_DIR/.venv-train/bin/python"
    if ! "$TRAIN_PYTHON" -c "import torch" >/dev/null 2>&1; then
        rm -rf "$INSTALL_DIR/.venv-train"
        "$INSTALL_DIR/.venv/bin/python" -m venv --system-site-packages "$INSTALL_DIR/.venv-train"
        TRAIN_PYTHON="$INSTALL_DIR/.venv-train/bin/python"
    fi
    if ! "$TRAIN_PYTHON" -c "import torch" >/dev/null 2>&1; then
        echo "ComfyUI torch is not importable from $INSTALL_DIR/.venv-train" >&2
        exit 1
    fi
    "$TRAIN_PYTHON" -m pip install --disable-pip-version-check \
        diffusers peft accelerate transformers safetensors pillow
    mkdir -p "$HOME/Library/LaunchAgents" "$HOME/Library/Logs"
    PLIST="$HOME/Library/LaunchAgents/ai.zone.train.plist"
    LOG="$HOME/Library/Logs/zone-train.log"
    PYTHON_XML=$(escape_xml "$INSTALL_DIR/.venv-train/bin/python")
    SCRIPT_XML=$(escape_xml "$INSTALL_DIR/train_sdxl.py")
    MODELS_XML=$(escape_xml "$MODELS_DIR")
    WORKDIR_XML=$(escape_xml "$INSTALL_DIR")
    LOG_XML=$(escape_xml "$LOG")
    cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>ai.zone.train</string>
    <key>KeepAlive</key>
    <true/>
    <key>RunAtLoad</key>
    <true/>
    <key>WorkingDirectory</key>
    <string>$WORKDIR_XML</string>
    <key>ProgramArguments</key>
    <array>
        <string>$PYTHON_XML</string>
        <string>$SCRIPT_XML</string>
        <string>--models-dir</string>
        <string>$MODELS_XML</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PYTHONUNBUFFERED</key>
        <string>1</string>
    </dict>
    <key>StandardOutPath</key>
    <string>$LOG_XML</string>
    <key>StandardErrorPath</key>
    <string>$LOG_XML</string>
</dict>
</plist>
EOF
    UID_VALUE=$(id -u)
    launchctl bootout "gui/$UID_VALUE/ai.zone.train" >/dev/null 2>&1 || true
    launchctl bootstrap "gui/$UID_VALUE" "$PLIST"
    echo "Installed host trainer LaunchAgent ai.zone.train"
}

if [ "$APPLY_NODES_ONLY" = "1" ] || [ "$INSTALL_TRAINER" = "1" ]; then
    if [ ! -d "$INSTALL_DIR" ]; then
        echo "ComfyUI is not installed at $INSTALL_DIR" >&2
        exit 1
    fi
    if [ "$APPLY_NODES_ONLY" = "1" ]; then
        apply_zone_nodes
    fi
    if [ "$INSTALL_TRAINER" = "1" ]; then
        install_trainer
    fi
    exit 0
fi

require_python

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
    "$MODELS_DIR/loras" \
    "$MODELS_DIR/upscale_models" \
    "$INSTALL_DIR/models" \
    "$INSTALL_DIR/output" \
    "$INSTALL_DIR/custom_nodes"
apply_zone_nodes
if [ "$MODELS_DIR" != "$INSTALL_DIR/models" ]; then
    for folder in checkpoints diffusion_models text_encoders vae loras upscale_models; do
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
