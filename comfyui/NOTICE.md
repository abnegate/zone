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

## Qwen Image Edit 2511

- Packaged models: <https://huggingface.co/Comfy-Org/Qwen-Image-Edit_ComfyUI>,
  <https://huggingface.co/Comfy-Org/Qwen-Image_ComfyUI>
- Original model: <https://huggingface.co/Qwen/Qwen-Image-Edit-2511>
- Files: `qwen_image_edit_2511_fp8mixed.safetensors`,
  `qwen_2.5_vl_7b_fp8_scaled.safetensors`, `qwen_image_vae.safetensors`
- License: Apache License 2.0
- License text: <https://www.apache.org/licenses/LICENSE-2.0>

These weights are not included in Zone images or source distributions. They
are downloaded only when the operator runs the explicit image-edit model setup
command.

## Wan 2.2 TI2V 5B

- Packaged models: <https://huggingface.co/Comfy-Org/Wan_2.2_ComfyUI_Repackaged>
- Original model: <https://huggingface.co/Wan-AI/Wan2.2-TI2V-5B>
- Files: `wan2.2_ti2v_5B_fp16.safetensors`, `wan2.2_vae.safetensors`,
  `umt5_xxl_fp8_e4m3fn_scaled.safetensors`
- License: Apache License 2.0
- License text: <https://www.apache.org/licenses/LICENSE-2.0>

These weights are not included in Zone images or source distributions. They
are downloaded only when the operator runs the explicit video model setup
command.

## ACE-Step v1 3.5B

- Packaged model: <https://huggingface.co/Comfy-Org/ACE-Step_ComfyUI_repackaged>
- Original model: <https://huggingface.co/ACE-Step/ACE-Step-v1-3.5B>
- Files: `ace_step_v1_3.5b.safetensors`
- License: Apache License 2.0
- License text: <https://github.com/ace-step/ACE-Step/blob/main/LICENSE>

These weights are not included in Zone images or source distributions. They
are downloaded only when the operator runs the explicit audio model setup
command.

## Real-ESRGAN x4plus

- Packaged model: <https://huggingface.co/Comfy-Org/Real-ESRGAN_repackaged>
- Original project: <https://github.com/xinntao/Real-ESRGAN>
- File: `RealESRGAN_x4plus.safetensors`
- License: BSD 3-Clause "New" License, Copyright (c) 2021, Xintao Wang
- License text: <https://github.com/xinntao/Real-ESRGAN/blob/master/LICENSE>

These weights are not included in Zone images or source distributions. They
are downloaded only when the operator runs the explicit upscale model setup
command.

## Lustify SDXL GGWP V7 (people)

- Packaged model: <https://huggingface.co/boopyfloopy/lustifyXL_v7>
- Original model: Lustify SDXL by coyotte (CreativeML Open RAIL-M)
- Diffusers config: <https://huggingface.co/John6666/lustify-sdxl-nsfw-checkpoint-ggwp-v7-sdxl>
- File: `lustifySDXLNSFW_ggwpV7.safetensors`
- License: CreativeML Open RAIL-M
- License text: <https://huggingface.co/spaces/CompVis/stable-diffusion-license>

These weights are not included in Zone images or source distributions. They
are downloaded only when the operator runs `--bundle image-people`.

## SDXL OpenPose ControlNet

- Packaged model: <https://huggingface.co/lllyasviel/sd_control_collection>
- File: `thibaud_xl_openpose.safetensors`
- License: Apache License 2.0
- License text: <https://huggingface.co/lllyasviel/sd_control_collection>

These weights are not included in Zone images or source distributions. They
are downloaded only when the operator runs `--bundle image-people-control`.

## IP-Adapter Plus Face SDXL

- Packaged model: <https://huggingface.co/h94/IP-Adapter>
- Files: `sdxl_models/ip-adapter-plus-face_sdxl_vit-h.safetensors`,
  `models/image_encoder/model.safetensors` (installed as
  `clip_vision/CLIP-ViT-H-14-laion2B-s32B-b79K.safetensors`)
- License: Apache License 2.0
- License text: <https://huggingface.co/datasets/choosealicense/licenses/blob/main/markdown/apache-2.0.md>

These weights are not included in Zone images or source distributions. They
are downloaded only when the operator runs `--bundle image-people-control`.

## FLUX Uncensored LoRA

- Packaged model: <https://huggingface.co/kenerateai/Flux-uncensored>
- File: `lora.safetensors`, installed as `loras/flux-uncensored.safetensors`
- License: CreativeML Open RAIL-M
- License text: <https://huggingface.co/spaces/CompVis/stable-diffusion-license>

These weights are not included in Zone images or source distributions. They
are downloaded with the `image` and `image-dev` bundles and always loaded on
the packaged FLUX graphs.

## Qwen Image Edit NSFW LoRA

- Packaged model: <https://huggingface.co/ScottzillaSystems/qwen-image-edit-plus-nsfw-lora>
- File: `qwen-image-edit-plus-nsfw-lora.safetensors`
- License: CreativeML Open RAIL++-M
- License text: <https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0/blob/main/LICENSE.md>

These weights are not included in Zone images or source distributions. They
are downloaded with the `image-edit` bundle and always loaded on the packaged
Qwen Image Edit graphs.
