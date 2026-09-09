#!/bin/sh
# Stand-in trainer for the live rig: writes a regular file where the pipeline
# expects the adapter, so screening, selection and the receipt all run for real
# against a stand-in ComfyUI. It trains nothing.
#
# Set COMFYUI_TRAIN_COMMAND to this script. The server passes ZONE_TRAIN_OUTPUT.
#
#   COMFYUI_TRAIN_COMMAND=scripts/live-verify/stand-in-trainer.sh
set -eu

readonly OUTPUT=${ZONE_TRAIN_OUTPUT:?ZONE_TRAIN_OUTPUT must name the adapter to write}
readonly CONTENT='zone-live-verification-adapter'

if ! printf '%s' "$CONTENT" >"$OUTPUT"; then
  echo "stand-in trainer could not write $OUTPUT" >&2
  exit 1
fi
