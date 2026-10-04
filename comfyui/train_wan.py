#!/usr/bin/env python3
"""Host worker for Wan 2.2 TI2V 5B video identity LoRA."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

import train_sdxl  # noqa: E402

CONFIG_PATH = Path(__file__).with_name('train_wan_config.json')
DEFAULT_HF_BASE = 'Comfy-Org/Wan_2.2_ComfyUI_Repackaged'
WAN_TRANSFORMER = 'wan2.2_ti2v_5B_fp16.safetensors'
WAN_VAE = 'wan2.2_vae.safetensors'
ATTN_TARGETS = ['to_q', 'to_k', 'to_v', 'to_out.0']


def load_config(path: Path | None = None) -> dict[str, Any]:
    source = Path(path) if path is not None else CONFIG_PATH
    return json.loads(source.read_text(encoding='utf-8'))


def steps_for(window_count: int, config: dict[str, Any]) -> int:
    budget = max(int(window_count), 0) * int(config['passes_per_clip'])
    return min(max(budget, int(config['min_steps'])), int(config['max_steps']))


def default_filename(name: str) -> str:
    name = (name or 'person').strip() or 'person'
    stem = name[:-12] if name.endswith('.safetensors') else name
    if not stem.endswith('-wan'):
        stem = f'{stem}-wan'
    return f'{stem}.safetensors'


def list_windows(job_dir: Path) -> list[tuple[Path, dict[str, Any]]]:
    clips = Path(job_dir) / 'clips'
    if not clips.is_dir():
        return []
    windows = []
    for mp4 in sorted(path for path in clips.iterdir() if path.is_file() and path.suffix.lower() == '.mp4'):
        meta: dict[str, Any] = {}
        sidecar = mp4.with_suffix('.json')
        if sidecar.is_file():
            payload = json.loads(sidecar.read_text(encoding='utf-8'))
            if isinstance(payload, dict):
                meta = payload
        windows.append((mp4, meta))
    return windows


def normalize_job(job: dict[str, Any], job_dir: Path) -> dict[str, Any]:
    if (job.get('method') or '') != 'video':
        raise ValueError(f'unsupported train method: {job.get("method")!r}')
    job['schema_version'] = int(job.get('schema_version') or 1)
    job['method'] = 'video'
    job['name'] = str(job.get('name') or 'person')
    job['trigger'] = str(job.get('trigger') or '')
    job['filename'] = train_sdxl.weight_name(
        str(job.get('filename') or default_filename(job['name'])), 'filename'
    )
    job['recipe_id'] = str(job.get('recipe_id') or 'wan-adapter')
    job['hf_base'] = str(job.get('hf_base') or DEFAULT_HF_BASE)
    windows = list_windows(job_dir)
    if job.get('image_count') is None:
        job['image_count'] = len(windows)
    else:
        job['image_count'] = int(job['image_count'])
    return job


def still_face(models_dir: Path, job: dict[str, Any]) -> str | None:
    filename = Path(str(job.get('filename') or '')).name
    stem = filename[:-12] if filename.endswith('.safetensors') else filename
    if stem.endswith('-wan'):
        stem = stem[:-4]
    name = str(job.get('name') or stem)
    for candidate in (stem, name):
        if not candidate:
            continue
        face = Path(models_dir) / 'loras' / f'{candidate}.face.png'
        if face.is_file():
            return face.name
    return None


def sidecar_payload(job: dict[str, Any], models_dir: Path) -> dict[str, Any]:
    payload: dict[str, Any] = {
        'recipe_id': job.get('recipe_id') or 'wan-adapter',
        'hf_base': job.get('hf_base') or DEFAULT_HF_BASE,
        'trigger': job.get('trigger') or '',
        'architecture': 'wan',
    }
    face = still_face(models_dir, job)
    if face:
        payload['face'] = face
    return payload


def publish_path(models_dir: Path, job: dict[str, Any]) -> Path:
    directory = Path(models_dir) / 'loras'
    directory.mkdir(parents=True, exist_ok=True)
    return directory / train_sdxl.weight_name(str(job['filename']), 'filename')


def publish_stub(models_dir: Path, job: dict[str, Any]) -> Path:
    destination = publish_path(models_dir, job)
    train_sdxl.write_stub_weights(destination)
    train_sdxl.write_json(train_sdxl.sidecar_path(destination), sidecar_payload(job, models_dir))
    return destination


def caption_for(job: dict[str, Any], meta: dict[str, Any]) -> str:
    trigger = str(job.get('trigger') or '').strip()
    pose = str(meta.get('pose') or '').strip()
    if trigger and pose:
        return f'{trigger} person, {pose}'
    return trigger or pose or 'a person'


def run_stub(models_dir: Path, job_dir: Path, job: dict[str, Any], config: dict[str, Any]) -> None:
    total = steps_for(int(job.get('image_count') or 0), config)
    progress = train_sdxl.Progress(job_dir, 'video', total)
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


def load_frames(path: Path, frames: int, width: int, height: int):
    import numpy as np
    import torch
    from PIL import Image

    with tempfile.TemporaryDirectory() as directory:
        destination = Path(directory) / '%06d.png'
        subprocess.run(
            [
                'ffmpeg',
                '-hide_banner',
                '-nostdin',
                '-loglevel',
                'error',
                '-i',
                str(path),
                '-vf',
                f'scale={width}:{height}:force_original_aspect_ratio=decrease,'
                f'pad={width}:{height}:(ow-iw)/2:(oh-ih)/2',
                '-frames:v',
                str(frames),
                str(destination),
            ],
            check=True,
        )
        images = sorted(Path(directory).glob('*.png'))
        if not images:
            raise RuntimeError(f'no frames decoded from {path}')
        tensors = []
        for image in images[:frames]:
            array = np.asarray(Image.open(image).convert('RGB'), dtype=np.float32) / 255.0
            tensors.append(torch.from_numpy(array).permute(2, 0, 1))
        while len(tensors) < frames:
            tensors.append(tensors[-1].clone())
        video = torch.stack(tensors, dim=1)
        return video.unsqueeze(0) * 2.0 - 1.0


def load_transformer(models_dir: Path, device: Any, dtype: Any):
    path = Path(models_dir) / 'diffusion_models' / WAN_TRANSFORMER
    if not path.is_file():
        raise FileNotFoundError(f'Wan transformer is missing: {path}')
    try:
        from diffusers import WanTransformer3DModel

        return WanTransformer3DModel.from_single_file(str(path), torch_dtype=dtype).to(device)
    except Exception as first:
        try:
            from diffusers import WanPipeline

            pipeline = WanPipeline.from_single_file(str(path), torch_dtype=dtype)
            return pipeline.transformer.to(device)
        except Exception as second:
            raise RuntimeError(
                f'could not load Wan transformer: {first}; {second}'
            ) from second


def load_vae(models_dir: Path, device: Any, dtype: Any):
    path = Path(models_dir) / 'vae' / WAN_VAE
    if not path.is_file():
        raise FileNotFoundError(f'Wan VAE is missing: {path}')
    from diffusers import AutoencoderKLWan

    vae = AutoencoderKLWan.from_single_file(str(path), torch_dtype=dtype)
    vae.requires_grad_(False)
    vae.eval()
    return vae.to(device)


def encode_prompt(caption: str, transformer: Any, device: Any):
    hidden = getattr(getattr(transformer, 'config', None), 'hidden_size', None)
    if hidden is None:
        hidden = getattr(getattr(transformer, 'config', None), 'in_features', 4096)
    import torch

    tokens = torch.zeros(1, 1, int(hidden), device=device, dtype=torch.float32)
    tokens[0, 0, 0] = min(len(caption), int(hidden) - 1)
    return tokens


def encode_latents(vae: Any, frames: Any, device: Any):
    import torch

    frames = frames.to(device=device, dtype=torch.float32)
    with torch.no_grad():
        if hasattr(vae, 'encode'):
            encoded = vae.encode(frames)
            latents = encoded.latent_dist.sample() if hasattr(encoded, 'latent_dist') else encoded
            if hasattr(latents, 'sample'):
                latents = latents.sample()
            scale = getattr(getattr(vae, 'config', None), 'scaling_factor', 1.0)
            latents = latents * scale
        else:
            latents = frames
    return latents.detach()


def flow_loss(transformer: Any, latents: Any, prompt: Any, device: Any):
    import torch
    import torch.nn.functional as functional

    latents = latents.to(device=device, dtype=torch.float32)
    noise = torch.randn_like(latents)
    timestep = torch.rand(latents.shape[0], device=device)
    while timestep.ndim < latents.ndim:
        timestep = timestep.unsqueeze(-1)
    noisy = (1.0 - timestep) * latents + timestep * noise
    t = (timestep.reshape(latents.shape[0]) * 1000).long().clamp(0, 999)
    kwargs: dict[str, Any] = {}
    try:
        predicted = transformer(noisy, t, encoder_hidden_states=prompt, **kwargs)
    except TypeError:
        predicted = transformer(noisy, timestep=t, encoder_hidden_states=prompt)
    sample = predicted.sample if hasattr(predicted, 'sample') else predicted
    target = noise - latents
    return functional.mse_loss(sample.float(), target.float())


def wan_lora_state(transformer: Any) -> dict[str, Any]:
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


def train(models_dir: Path, job_dir: Path, job: dict[str, Any], config: dict[str, Any]) -> None:
    import torch

    windows = list_windows(job_dir)
    if not windows:
        raise RuntimeError(f'no clip windows in {Path(job_dir) / "clips"}')
    job['image_count'] = len(windows)
    total = steps_for(len(windows), config)
    job['total'] = total
    job['step'] = job.get('step') or 0
    train_sdxl.write_job(job_dir, job)
    progress = train_sdxl.Progress(job_dir, 'video', total)
    progress.emit('loading', phase_step=0, phase_total=1)

    device = train_sdxl.select_device()
    dtype = torch.float32
    transformer = load_transformer(models_dir, device, dtype)
    vae = load_vae(models_dir, device, dtype)
    if bool(config.get('gradient_checkpointing', True)) and hasattr(
        transformer, 'enable_gradient_checkpointing'
    ):
        transformer.enable_gradient_checkpointing()
    transformer = train_sdxl.apply_lora(
        transformer, ATTN_TARGETS, int(config['rank']), int(config['alpha'])
    )
    optimizer = torch.optim.AdamW(
        train_sdxl.trainable_parameters(transformer), lr=float(config['lr'])
    )
    transformer.train()
    progress.emit('loading', phase_step=1, phase_total=1)

    encoded = []
    progress.emit('encoding', phase_step=0, phase_total=len(windows))
    for index, (path, meta) in enumerate(windows, start=1):
        frames = load_frames(
            path,
            int(config['frames']),
            int(config['width']),
            int(config['height']),
        )
        latents = encode_latents(vae, frames, device)
        prompt = encode_prompt(caption_for(job, meta), transformer, device)
        encoded.append((latents.cpu(), prompt.cpu()))
        progress.emit('encoding', phase_step=index, phase_total=len(windows))
    vae.to('cpu')

    progress.emit('training', phase_step=0, phase_total=total)
    for step in range(1, total + 1):
        latents, prompt = encoded[(step - 1) % len(encoded)]
        optimizer.zero_grad(set_to_none=True)
        loss = flow_loss(transformer, latents, prompt, device)
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
    destination = publish_path(models_dir, job)
    train_sdxl.save_safetensors(destination, wan_lora_state(transformer))
    train_sdxl.write_json(train_sdxl.sidecar_path(destination), sidecar_payload(job, models_dir))
    job['step'] = total
    progress.emit('publishing', step=total, phase_step=1, phase_total=1)
    train_sdxl.write_job(job_dir, job)


def process_job(models_dir: Path, job_dir: Path, stub: bool) -> None:
    job = train_sdxl.load_job(job_dir)
    try:
        job = normalize_job(job, job_dir)
        job = train_sdxl.mark_running(job_dir, job)
        config = load_config()
        if stub:
            run_stub(models_dir, job_dir, job, config)
        else:
            train(models_dir, job_dir, job, config)
        train_sdxl.mark_succeeded(job_dir, job)
    except Exception as error:
        train_sdxl.mark_failed(job_dir, job, error)
        raise
