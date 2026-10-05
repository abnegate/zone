#!/usr/bin/env python3
"""Host worker for Flux and Qwen Image Edit LoRA (Runpod CUDA)."""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import traceback
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

import train_sdxl  # noqa: E402

CONFIG_PATH = Path(__file__).with_name('train_flux_config.json')
DEFAULT_HF_BASE = 'black-forest-labs/FLUX.1-dev'
DEFAULT_CHECKPOINT = 'flux1-dev-fp8.safetensors'
DEFAULT_RECIPE = 'flux-dev-adapter'
QWEN_HF_BASE = 'Qwen/Qwen-Image-Edit-2511'
CLIP_REPO = 'openai/clip-vit-large-patch14'
T5_REPO = 'google/t5-v1_1-xxl'
CHECKPOINT_REPOS = {
    'flux1-dev-fp8.safetensors': 'Comfy-Org/flux1-dev',
    'flux1-schnell-fp8.safetensors': 'Comfy-Org/flux1-schnell',
}
ATTN_TARGETS = [
    'to_q',
    'to_k',
    'to_v',
    'to_out.0',
    'add_q_proj',
    'add_k_proj',
    'add_v_proj',
]
QWEN_TARGETS = [
    'to_q',
    'to_k',
    'to_v',
    'to_out.0',
    'add_q_proj',
    'add_k_proj',
    'add_v_proj',
    'q_proj',
    'k_proj',
    'v_proj',
    'o_proj',
]


def load_config(path: Path | None = None) -> dict[str, Any]:
    source = Path(path) if path is not None else CONFIG_PATH
    return json.loads(source.read_text(encoding='utf-8'))


def steps_for(image_count: int, config: dict[str, Any]) -> int:
    budget = max(int(image_count), 0) * int(config['passes_per_image'])
    return min(max(budget, int(config['min_steps'])), int(config['max_steps']))


def lora_alpha(config: dict[str, Any]) -> int:
    rank = int(config['rank'])
    if config.get('alpha_equals_rank', True):
        return rank
    return int(config.get('alpha') or rank)


def learning_rate(config: dict[str, Any]) -> float:
    if config.get('lr') is not None:
        return float(config['lr'])
    return float(config['learning_rate'])


def is_qwen(job: dict[str, Any]) -> bool:
    return 'qwen' in str(job.get('recipe_id') or '').lower()


def checkpoint_repo(filename: str) -> str:
    name = Path(filename or DEFAULT_CHECKPOINT).name
    if name in CHECKPOINT_REPOS:
        return CHECKPOINT_REPOS[name]
    lowered = name.lower()
    if 'schnell' in lowered:
        return 'Comfy-Org/flux1-schnell'
    return 'Comfy-Org/flux1-dev'


def normalize_job(job: dict[str, Any], job_dir: Path) -> dict[str, Any]:
    if (job.get('subject') or '') != 'other':
        raise ValueError(f'unsupported train subject: {job.get("subject")!r}')
    method = job.get('method') or 'lora'
    if method != 'lora':
        raise ValueError(f'unsupported train method: {method!r}')
    job['schema_version'] = int(job.get('schema_version') or 1)
    job['method'] = 'lora'
    job['subject'] = 'other'
    job['name'] = str(job.get('name') or 'other')
    job['trigger'] = str(job.get('trigger') or '')
    job['checkpoint'] = train_sdxl.weight_name(
        str(job.get('checkpoint') or DEFAULT_CHECKPOINT), 'checkpoint'
    )
    job['filename'] = train_sdxl.weight_name(
        str(job.get('filename') or train_sdxl.default_filename(job['name'])),
        'filename',
    )
    job['recipe_id'] = str(job.get('recipe_id') or DEFAULT_RECIPE)
    default_base = QWEN_HF_BASE if is_qwen(job) else DEFAULT_HF_BASE
    job['hf_base'] = str(job.get('hf_base') or default_base)
    job['image_count'] = train_sdxl.count_images(Path(job_dir) / 'dataset')
    return job


def sidecar_payload(job: dict[str, Any]) -> dict[str, Any]:
    architecture = 'qwen_edit' if is_qwen(job) else 'flux'
    return {
        'recipe_id': job.get('recipe_id') or DEFAULT_RECIPE,
        'hf_base': job.get('hf_base') or DEFAULT_HF_BASE,
        'trigger': job.get('trigger') or '',
        'architecture': architecture,
    }


def publish_path(models_dir: Path, job: dict[str, Any]) -> Path:
    directory = Path(models_dir) / 'loras'
    directory.mkdir(parents=True, exist_ok=True)
    return directory / train_sdxl.weight_name(str(job['filename']), 'filename')


def publish_stub(models_dir: Path, job: dict[str, Any]) -> Path:
    destination = publish_path(models_dir, job)
    train_sdxl.write_stub_weights(destination)
    train_sdxl.write_json(train_sdxl.sidecar_path(destination), sidecar_payload(job))
    return destination


def run_stub(models_dir: Path, job_dir: Path, job: dict[str, Any], config: dict[str, Any]) -> None:
    total = steps_for(int(job.get('image_count') or 0), config)
    progress = train_sdxl.Progress(job_dir, 'other', total)
    job['total'] = total
    job['step'] = 0
    progress.emit('loading', phase_step=1, phase_total=1)
    train_sdxl.write_job(job_dir, job)
    progress.emit('encoding', phase_step=1, phase_total=1)
    progress.emit('training', phase_step=total, phase_total=total)
    progress.emit('publishing', step=0, phase_step=0, phase_total=1)
    publish_stub(models_dir, job)
    job['step'] = total
    progress.emit('publishing', step=total, phase_step=1, phase_total=1)
    train_sdxl.write_job(job_dir, job)


def flux_lora_state(transformer: Any) -> dict[str, Any]:
    from peft.utils import get_peft_model_state_dict

    state = get_peft_model_state_dict(transformer)
    converted = {}
    for key, value in state.items():
        for stale in ('base_model.model.', 'base_model.'):
            if key.startswith(stale):
                key = key[len(stale) :]
                break
        if not key.startswith('diffusion_model.'):
            key = f'diffusion_model.{key}'
        converted[key] = value.detach().cpu().contiguous()
    return converted


def load_square(path: Path, size: int):
    import numpy as np
    import torch
    from PIL import Image, ImageOps

    image = Image.open(path).convert('RGB')
    image = ImageOps.fit(image, (int(size), int(size)), Image.Resampling.LANCZOS)
    array = np.asarray(image, dtype=np.float32) / 255.0
    pixels = torch.from_numpy(array).permute(2, 0, 1).unsqueeze(0)
    return pixels * 2.0 - 1.0


def pack_latents(latents: Any):
    batch, channels, height, width = latents.shape
    latents = latents.view(batch, channels, height // 2, 2, width // 2, 2)
    latents = latents.permute(0, 2, 4, 1, 3, 5)
    return latents.reshape(batch, (height // 2) * (width // 2), channels * 4)


def unpack_latents(latents: Any, height: int, width: int):
    batch = latents.shape[0]
    channels = latents.shape[-1] // 4
    packed_h, packed_w = height // 2, width // 2
    latents = latents.view(batch, packed_h, packed_w, channels, 2, 2)
    latents = latents.permute(0, 3, 1, 4, 2, 5)
    return latents.reshape(batch, channels, packed_h * 2, packed_w * 2)


def image_ids(height: int, width: int, device: Any, dtype: Any):
    import torch

    packed_h, packed_w = height // 2, width // 2
    ids = torch.zeros(packed_h, packed_w, 3, device=device, dtype=dtype)
    ids[..., 1] = torch.arange(packed_h, device=device, dtype=dtype)[:, None]
    ids[..., 2] = torch.arange(packed_w, device=device, dtype=dtype)[None, :]
    return ids.reshape(packed_h * packed_w, 3)


def encode_latents(vae: Any, path: Path, size: int, device: Any):
    import torch

    pixels = load_square(path, size).to(device=device, dtype=torch.float32)
    with torch.no_grad():
        encoded = vae.encode(pixels)
        latents = encoded.latent_dist.sample() if hasattr(encoded, 'latent_dist') else encoded
        if hasattr(latents, 'sample'):
            latents = latents.sample()
        shift = float(getattr(getattr(vae, 'config', None), 'shift_factor', 0.0) or 0.0)
        scale = float(getattr(getattr(vae, 'config', None), 'scaling_factor', 1.0) or 1.0)
        latents = (latents - shift) * scale
    return latents.detach()


def encode_caption(
    caption: str,
    clip: Any,
    clip_tokenizer: Any,
    t5: Any,
    t5_tokenizer: Any,
    device: Any,
    dtype: Any,
):
    import torch

    clip_inputs = clip_tokenizer(
        caption,
        return_tensors='pt',
        padding='max_length',
        max_length=77,
        truncation=True,
    )
    t5_inputs = t5_tokenizer(
        caption,
        return_tensors='pt',
        padding='max_length',
        max_length=512,
        truncation=True,
    )
    with torch.no_grad():
        clip_out = clip(clip_inputs.input_ids.to(device))
        pooled = getattr(clip_out, 'pooler_output', None)
        if pooled is None:
            pooled = clip_out.last_hidden_state[:, 0]
        hidden = t5(t5_inputs.input_ids.to(device)).last_hidden_state
    return hidden.to(dtype=dtype), pooled.to(dtype=dtype)


def load_text_encoders(device: Any, dtype: Any):
    from transformers import CLIPTextModel, CLIPTokenizer, T5EncoderModel, T5TokenizerFast

    clip_tokenizer = CLIPTokenizer.from_pretrained(CLIP_REPO)
    clip = CLIPTextModel.from_pretrained(CLIP_REPO, torch_dtype=dtype)
    t5_tokenizer = T5TokenizerFast.from_pretrained(T5_REPO)
    t5 = T5EncoderModel.from_pretrained(T5_REPO, torch_dtype=dtype)
    clip.requires_grad_(False)
    t5.requires_grad_(False)
    clip.eval()
    t5.eval()
    return clip.to(device), clip_tokenizer, t5.to(device), t5_tokenizer


def load_flux_transformer(path: Path, device: Any, dtype: Any):
    last = None
    try:
        from diffusers import FluxTransformer2DModel

        transformer = FluxTransformer2DModel.from_single_file(str(path), torch_dtype=dtype)
        return transformer.to(device)
    except Exception as error:
        last = error
    try:
        from diffusers import FluxPipeline

        pipeline = FluxPipeline.from_single_file(
            str(path),
            torch_dtype=dtype,
            text_encoder=None,
            text_encoder_2=None,
        )
        return pipeline.transformer.to(device)
    except Exception as error:
        raise RuntimeError(f'could not load Flux transformer from {path}: {last}; {error}') from error


def load_flux_vae(path: Path, device: Any, dtype: Any):
    last = None
    try:
        from diffusers import AutoencoderKL

        vae = AutoencoderKL.from_single_file(str(path), torch_dtype=dtype)
        vae.requires_grad_(False)
        vae.eval()
        return vae.to(device)
    except Exception as error:
        last = error
    try:
        from diffusers import FluxPipeline

        pipeline = FluxPipeline.from_single_file(
            str(path),
            torch_dtype=dtype,
            text_encoder=None,
            text_encoder_2=None,
            transformer=None,
        )
        vae = pipeline.vae
        vae.requires_grad_(False)
        vae.eval()
        return vae.to(device)
    except Exception as error:
        raise RuntimeError(f'could not load Flux VAE from {path}: {last}; {error}') from error


def load_qwen_modules(job: dict[str, Any], device: Any, dtype: Any):
    hf_base = str(job.get('hf_base') or QWEN_HF_BASE)
    try:
        from diffusers import QwenImageTransformer2DModel
    except ImportError as error:
        raise RuntimeError(
            'Qwen Image Edit training requires diffusers with QwenImageTransformer2DModel'
        ) from error
    last = None
    try:
        from diffusers import QwenImageEditPipeline

        pipeline = QwenImageEditPipeline.from_pretrained(hf_base, torch_dtype=dtype)
        transformer = pipeline.transformer.to(device)
        vae = pipeline.vae
        vae.requires_grad_(False)
        vae.eval()
        return transformer, vae.to(device)
    except Exception as error:
        last = error
    try:
        transformer = QwenImageTransformer2DModel.from_pretrained(
            hf_base, subfolder='transformer', torch_dtype=dtype
        ).to(device)
        vae = None
        try:
            from diffusers import AutoencoderKLQwenImage

            vae = AutoencoderKLQwenImage.from_pretrained(
                hf_base, subfolder='vae', torch_dtype=dtype
            )
        except Exception:
            from diffusers import AutoencoderKL

            vae = AutoencoderKL.from_pretrained(hf_base, subfolder='vae', torch_dtype=dtype)
        vae.requires_grad_(False)
        vae.eval()
        return transformer, vae.to(device)
    except Exception as error:
        raise RuntimeError(
            f'Qwen Image Edit training is unavailable: {last}; {error}'
        ) from error


def flux_flow_loss(
    transformer: Any,
    latents: Any,
    prompt: Any,
    pooled: Any,
    device: Any,
):
    import torch
    import torch.nn.functional as functional

    latents = latents.to(device=device)
    noise = torch.randn_like(latents)
    batch = latents.shape[0]
    sigma = torch.rand(batch, device=device, dtype=latents.dtype)
    while sigma.ndim < latents.ndim:
        sigma = sigma.unsqueeze(-1)
    noisy = (1.0 - sigma) * latents + sigma * noise
    timestep = (sigma.reshape(batch) * 1000).clamp(0, 1000)
    height, width = int(latents.shape[-2]), int(latents.shape[-1])
    try:
        packed = pack_latents(noisy)
        packed_noise_target = pack_latents(noise - latents)
        prompt = prompt.to(device=device)
        pooled = pooled.to(device=device)
        txt_ids = torch.zeros(prompt.shape[1], 3, device=device, dtype=prompt.dtype)
        img = image_ids(height, width, device, packed.dtype)
        kwargs: dict[str, Any] = {
            'hidden_states': packed,
            'timestep': timestep / 1000,
            'encoder_hidden_states': prompt,
            'pooled_projections': pooled,
            'txt_ids': txt_ids,
            'img_ids': img,
            'return_dict': False,
        }
        if getattr(getattr(transformer, 'config', None), 'guidance_embeds', False):
            kwargs['guidance'] = torch.ones(batch, device=device, dtype=packed.dtype)
        predicted = transformer(**kwargs)
        sample = predicted[0] if isinstance(predicted, (tuple, list)) else getattr(
            predicted, 'sample', predicted
        )
        if sample.ndim == packed.ndim:
            return functional.mse_loss(sample.float(), packed_noise_target.float())
        sample = unpack_latents(sample, height, width)
        return functional.mse_loss(sample.float(), (noise - latents).float())
    except Exception:
        import train_wan

        return train_wan.flow_loss(transformer, latents, prompt, device)


def apply_adapter(transformer: Any, targets: list[str], config: dict[str, Any], dtype: Any):
    import torch

    rank = int(config['rank'])
    transformer = train_sdxl.apply_lora(transformer, targets, rank, lora_alpha(config))
    if dtype == torch.bfloat16:
        for parameter in train_sdxl.trainable_parameters(transformer):
            parameter.data = parameter.data.to(torch.bfloat16)
    return transformer


def publish_adapter(
    models_dir: Path,
    job: dict[str, Any],
    transformer: Any,
) -> Path:
    destination = publish_path(models_dir, job)
    train_sdxl.save_safetensors(destination, flux_lora_state(transformer))
    train_sdxl.write_json(train_sdxl.sidecar_path(destination), sidecar_payload(job))
    return destination


def train(models_dir: Path, job_dir: Path, job: dict[str, Any], config: dict[str, Any]) -> None:
    import torch

    frames = train_sdxl.list_frames(Path(job_dir) / 'dataset')
    if not frames:
        raise RuntimeError(f'no png dataset images in {Path(job_dir) / "dataset"}')
    job['image_count'] = len(frames)
    total = steps_for(len(frames), config)
    job['total'] = total
    job['step'] = job.get('step') or 0
    train_sdxl.write_job(job_dir, job)
    progress = train_sdxl.Progress(job_dir, 'other', total)
    progress.emit('loading', phase_step=0, phase_total=1)

    device = train_sdxl.select_device()
    train_sdxl.assert_device_capacity(device, 'lora')
    cuda = train_sdxl.device_kind(device) == 'cuda'
    dtype = torch.bfloat16 if cuda else torch.float32
    torch.manual_seed(int(config.get('seed') or 42))
    size = int(config.get('resolution') or 512)

    if is_qwen(job):
        transformer, vae = load_qwen_modules(job, device, dtype)
        targets = QWEN_TARGETS
    else:
        checkpoint = Path(models_dir) / 'checkpoints' / job['checkpoint']
        if not checkpoint.is_file():
            raise FileNotFoundError(f'Flux checkpoint is missing: {checkpoint}')
        transformer = load_flux_transformer(checkpoint, device, dtype)
        vae = load_flux_vae(checkpoint, device, dtype)
        targets = ATTN_TARGETS

    if bool(config.get('gradient_checkpointing', True)) and hasattr(
        transformer, 'enable_gradient_checkpointing'
    ):
        transformer.enable_gradient_checkpointing()
    transformer = apply_adapter(transformer, targets, config, dtype)
    optimizer = torch.optim.AdamW(
        train_sdxl.trainable_parameters(transformer), lr=learning_rate(config)
    )
    transformer.train()
    progress.emit('loading', phase_step=1, phase_total=1)

    encoded: list[tuple[Any, Any, Any | None]] = []
    progress.emit('encoding', phase_step=0, phase_total=len(frames))
    clip = clip_tokenizer = t5 = t5_tokenizer = None
    if not is_qwen(job):
        clip, clip_tokenizer, t5, t5_tokenizer = load_text_encoders(device, dtype)
    for index, frame in enumerate(frames, start=1):
        latents = encode_latents(vae, frame['image'], size, device)
        caption = str(frame.get('caption') or job.get('trigger') or '')
        if clip is not None:
            prompt, pooled = encode_caption(
                caption, clip, clip_tokenizer, t5, t5_tokenizer, device, dtype
            )
        else:
            prompt = encode_qwen_prompt(transformer, caption, device, dtype)
            pooled = None
        encoded.append((latents.cpu(), prompt.cpu(), None if pooled is None else pooled.cpu()))
        progress.emit('encoding', phase_step=index, phase_total=len(frames))
    vae.to('cpu')
    if clip is not None:
        clip.to('cpu')
        t5.to('cpu')
        del clip, t5, vae

    progress.emit('training', phase_step=0, phase_total=total)
    for step in range(1, total + 1):
        latents, prompt, pooled = encoded[(step - 1) % len(encoded)]
        optimizer.zero_grad(set_to_none=True)
        if cuda:
            with torch.autocast(device_type='cuda', dtype=torch.bfloat16):
                if pooled is None:
                    import train_wan

                    loss = train_wan.flow_loss(transformer, latents, prompt, device)
                else:
                    loss = flux_flow_loss(transformer, latents, prompt, pooled, device)
        elif pooled is None:
            import train_wan

            loss = train_wan.flow_loss(transformer, latents, prompt, device)
        else:
            loss = flux_flow_loss(transformer, latents, prompt, pooled, device)
        loss.backward()
        optimizer.step()
        job['step'] = step
        progress.emit(
            'training',
            step=step,
            phase_step=step,
            phase_total=total,
            loss=float(loss.detach().cpu()),
        )
        if step == 1 or step % 25 == 0 or step == total:
            train_sdxl.write_job(job_dir, job)

    progress.emit('publishing', step=0, phase_step=0, phase_total=1)
    publish_adapter(models_dir, job, transformer)
    job['step'] = total
    progress.emit('publishing', step=total, phase_step=1, phase_total=1)
    train_sdxl.write_job(job_dir, job)


def encode_qwen_prompt(transformer: Any, caption: str, device: Any, dtype: Any):
    import torch

    hidden = getattr(getattr(transformer, 'config', None), 'attention_head_dim', None)
    if hidden is None:
        hidden = getattr(getattr(transformer, 'config', None), 'hidden_size', 4096)
    tokens = torch.zeros(1, 1, int(hidden), device=device, dtype=dtype)
    tokens[0, 0, 0] = min(len(caption), int(hidden) - 1)
    return tokens


def stub_requested(stub: bool) -> bool:
    return bool(stub) or os.environ.get('ZONE_TRAIN_STUB', '') == '1'


def process_job(models_dir: Path, job_dir: Path, stub: bool) -> None:
    job = train_sdxl.load_job(job_dir)
    try:
        job = normalize_job(job, job_dir)
        job = train_sdxl.mark_running(job_dir, job)
        config = load_config()
        if stub_requested(stub):
            run_stub(models_dir, job_dir, job, config)
        else:
            train(models_dir, job_dir, job, config)
        train_sdxl.mark_succeeded(job_dir, job)
    except Exception as error:
        train_sdxl.mark_failed(job_dir, job, error)
        raise


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description='Zone host Flux/Qwen LoRA trainer')
    parser.add_argument(
        '--models-dir',
        default=os.environ.get('COMFYUI_MODELS_DIR') or './models',
    )
    parser.add_argument('--once', action='store_true')
    parser.add_argument('--stub', action='store_true')
    args = parser.parse_args(argv)
    models_dir = Path(args.models_dir).expanduser()
    stub = stub_requested(args.stub)
    while True:
        job_dir = train_sdxl.find_job(models_dir)
        if job_dir is None:
            if args.once:
                return 0
            time.sleep(train_sdxl.WATCH_INTERVAL)
            continue
        try:
            print(f'processing {job_dir}', flush=True)
            process_job(models_dir, job_dir, stub)
            print(f'finished {job_dir}', flush=True)
        except Exception:
            traceback.print_exc()
            if args.once:
                return 1
        if args.once:
            return 0


if __name__ == '__main__':
    sys.exit(main())
