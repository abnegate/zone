# Third-party notices

This packaging installs third-party software and model weights; Zone does not
claim ownership of either project.

## ComfyUI

- Source: <https://github.com/comfyanonymous/ComfyUI>
- Pinned commit: `30bdda1ef13a3a34fce2cd2fec633f15d832122a`
- License: GNU General Public License v3.0
- License text: <https://github.com/comfyanonymous/ComfyUI/blob/30bdda1ef13a3a34fce2cd2fec633f15d832122a/LICENSE>

The Docker build fetches the pinned source directly from the upstream
repository and preserves its license files.

## FLUX.1 Schnell FP8

- Packaged model: <https://huggingface.co/Comfy-Org/flux1-schnell>
- Original model: <https://huggingface.co/black-forest-labs/FLUX.1-schnell>
- License: Apache License 2.0
- License text: <https://github.com/black-forest-labs/flux/blob/main/model_licenses/LICENSE-FLUX1-schnell>

The model is not included in Zone images or source distributions. It is
downloaded only when the operator runs an explicit setup command.
