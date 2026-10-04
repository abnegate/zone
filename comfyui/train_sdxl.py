#!/usr/bin/env python3
"""Host LaunchAgent worker for SDXL person LoRA and UNet fine-tunes."""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import traceback
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

WATCH_INTERVAL = 2
DEFAULT_HF_BASE = 'John6666/lustify-sdxl-nsfw-checkpoint-ggwp-v7-sdxl'
DEFAULT_CHECKPOINT = 'lustifySDXLNSFW_ggwpV7.safetensors'
CONFIG_PATH = Path(__file__).with_name('train_sdxl_config.json')
TRAIN_ROOT = '.zone-train'
UNET_TARGETS = [
    'to_k',
    'to_q',
    'to_v',
    'to_out.0',
    'ff.net.0.proj',
    'ff.net.2',
    'proj_in',
    'proj_out',
]
TEXT_ENCODER_TARGETS = ['q_proj', 'k_proj', 'v_proj', 'out_proj', 'fc1', 'fc2']
JOB_KEYS = (
    'schema_version',
    'id',
    'name',
    'method',
    'trigger',
    'checkpoint',
    'filename',
    'recipe_id',
    'hf_base',
    'image_count',
    'status',
    'error',
    'step',
    'total',
    'pid',
    'started_at',
)


def load_config(path: Path | None = None) -> dict[str, Any]:
    source = Path(path) if path is not None else CONFIG_PATH
    return json.loads(source.read_text(encoding='utf-8'))


def steps_for(image_count: int, config: dict[str, Any]) -> int:
    budget = max(int(image_count), 0) * int(config['passes_per_image'])
    return min(max(budget, int(config['min_steps'])), int(config['max_steps']))


def recipe_for(method: str) -> str:
    return 'sdxl-adapter' if method == 'lora' else 'sdxl'


def default_filename(name: str) -> str:
    name = (name or 'person').strip() or 'person'
    if name.endswith('.safetensors'):
        return name
    return f'{name}.safetensors'


def weight_name(name: str, label: str) -> str:
    if not name or name != Path(name).name or '/' in name or '\\' in name or '..' in name:
        raise ValueError(f'invalid {label}: {name!r}')
    return name


def now_rfc3339() -> str:
    return datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%S.%fZ')


def atomic_write(path: Path, data: str | bytes) -> None:
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + '.tmp')
    if isinstance(data, bytes):
        temporary.write_bytes(data)
    else:
        temporary.write_text(data, encoding='utf-8')
    os.replace(temporary, path)


def write_json(path: Path, payload: dict[str, Any], *, pretty: bool = True) -> None:
    if pretty:
        encoded = json.dumps(payload, indent=2) + '\n'
    else:
        encoded = json.dumps(payload, separators=(',', ':'))
    atomic_write(path, encoded)


def count_images(dataset: Path) -> int:
    if not dataset.is_dir():
        return 0
    return sum(1 for path in dataset.iterdir() if path.is_file() and path.suffix.lower() == '.png')


def list_pairs(dataset: Path) -> list[tuple[Path, str]]:
    if not dataset.is_dir():
        return []
    pairs = []
    for png in sorted(path for path in dataset.iterdir() if path.is_file() and path.suffix.lower() == '.png'):
        caption_path = png.with_suffix('.txt')
        caption = caption_path.read_text(encoding='utf-8').strip() if caption_path.is_file() else ''
        pairs.append((png, caption))
    return pairs


def ordered_job(job: dict[str, Any]) -> dict[str, Any]:
    ordered: dict[str, Any] = {}
    for key in JOB_KEYS:
        if key in job:
            ordered[key] = job[key]
    for key, value in job.items():
        if key not in ordered:
            ordered[key] = value
    return ordered


def write_job(job_dir: Path, job: dict[str, Any]) -> None:
    write_json(Path(job_dir) / 'job.json', ordered_job(job))


def load_job(job_dir: Path) -> dict[str, Any]:
    payload = json.loads((Path(job_dir) / 'job.json').read_text(encoding='utf-8'))
    if not isinstance(payload, dict):
        raise ValueError(f'job.json is not an object: {job_dir}')
    return payload


def normalize_job(job: dict[str, Any], job_dir: Path) -> dict[str, Any]:
    method = job.get('method') or 'lora'
    if method not in {'lora', 'finetune'}:
        raise ValueError(f'unsupported train method: {method!r}')
    job['schema_version'] = int(job.get('schema_version') or 1)
    job['method'] = method
    job['name'] = str(job.get('name') or 'person')
    job['trigger'] = str(job.get('trigger') or '')
    job['checkpoint'] = weight_name(
        str(job.get('checkpoint') or DEFAULT_CHECKPOINT), 'checkpoint'
    )
    job['filename'] = weight_name(
        str(job.get('filename') or default_filename(job['name'])), 'filename'
    )
    job['recipe_id'] = str(job.get('recipe_id') or recipe_for(method))
    job['hf_base'] = str(job.get('hf_base') or DEFAULT_HF_BASE)
    if job.get('image_count') is None:
        job['image_count'] = count_images(Path(job_dir) / 'dataset')
    else:
        job['image_count'] = int(job['image_count'])
    return job


PHASE_WEIGHTS = {
    'lora': (
        ('loading', 6),
        ('encoding', 8),
        ('training', 80),
        ('publishing', 6),
    ),
    'finetune': (
        ('loading', 4),
        ('class_images', 12),
        ('encoding', 6),
        ('training', 72),
        ('publishing', 6),
    ),
}


def overall_percent(method: str, phase: str, phase_step: int, phase_total: int) -> int:
    if phase in {'queued', 'screening', 'captioning'}:
        return 0
    weights = PHASE_WEIGHTS['finetune' if method == 'finetune' else 'lora']
    completed = 0
    current = 0
    found = False
    for name, weight in weights:
        if name == phase:
            current = weight
            found = True
            break
        completed += weight
    if not found:
        return 100 if phase == 'publishing' else min(100, completed)
    if phase_total <= 0:
        fraction = 0.0
    else:
        fraction = min(1.0, max(0.0, float(phase_step) / float(phase_total)))
    return min(100, int(round(completed + current * fraction)))


def eta_seconds(elapsed: float, step: int, total: int) -> int | None:
    if step <= 0 or total <= step or elapsed <= 0:
        return None
    remaining = elapsed * (total - step) / step
    if remaining < 1:
        return None
    return int(round(remaining))


def phase_message(phase: str, phase_step: int, phase_total: int) -> str:
    if phase == 'queued':
        return 'Waiting for the host trainer'
    if phase == 'loading':
        return 'Loading the people checkpoint'
    if phase == 'class_images':
        if phase_total > 0:
            return f'Generating class image {max(phase_step, 1)} of {phase_total}'
        return 'Generating class images for prior preservation'
    if phase == 'encoding':
        if phase_total > 0:
            return f'Encoding image {max(phase_step, 1)} of {phase_total}'
        return 'Encoding dataset latents'
    if phase == 'training':
        if phase_total > 0:
            return f'Training step {phase_step} of {phase_total}'
        return 'Training'
    if phase == 'publishing':
        return 'Publishing the trained weights'
    if phase == 'screening':
        return 'Screening the dataset'
    if phase == 'captioning':
        return 'Captioning the dataset'
    return phase.replace('_', ' ').capitalize()


class Progress:
    def __init__(self, job_dir: Path, method: str, total: int) -> None:
        self.job_dir = Path(job_dir)
        self.method = 'finetune' if method == 'finetune' else 'lora'
        self.total = max(int(total), 1)
        self.phase = 'queued'
        self.phase_started = time.monotonic()

    def emit(
        self,
        phase: str,
        *,
        step: int | None = None,
        phase_step: int = 0,
        phase_total: int = 0,
        loss: float | None = None,
    ) -> None:
        if phase != self.phase:
            self.phase = phase
            self.phase_started = time.monotonic()
        if step is None:
            if phase == 'training':
                step = phase_step
            elif phase == 'publishing':
                step = self.total
            else:
                step = 0
        payload: dict[str, Any] = {
            'step': int(min(step, self.total)),
            'total': self.total,
            'phase': phase,
            'message': phase_message(phase, int(phase_step), int(phase_total)),
            'percent': overall_percent(self.method, phase, int(phase_step), int(phase_total)),
        }
        if phase_total > 0:
            payload['phase_step'] = int(phase_step)
            payload['phase_total'] = int(phase_total)
        if loss is not None and loss == loss:
            payload['loss'] = float(loss)
        remaining = eta_seconds(
            time.monotonic() - self.phase_started,
            int(phase_step),
            int(phase_total),
        )
        if remaining is not None:
            payload['eta_seconds'] = remaining
        write_progress(self.job_dir, payload['step'], payload['total'], **{
            key: value for key, value in payload.items() if key not in {'step', 'total'}
        })


def write_progress(job_dir: Path, step: int, total: int, **extra: Any) -> None:
    payload: dict[str, Any] = {'step': int(step), 'total': int(total)}
    payload.update(extra)
    write_json(Path(job_dir) / 'progress.json', payload, pretty=False)


def sidecar_path(weight: Path) -> Path:
    return weight.with_name(weight.name + '.zone.json')


def sidecar_payload(job: dict[str, Any]) -> dict[str, Any]:
    return {
        'recipe_id': job.get('recipe_id') or recipe_for(job.get('method') or 'lora'),
        'hf_base': job.get('hf_base') or DEFAULT_HF_BASE,
        'trigger': job.get('trigger') or '',
    }


def write_sidecar(weight: Path, job: dict[str, Any]) -> None:
    write_json(sidecar_path(weight), sidecar_payload(job))


def publish_directory(models_dir: Path, method: str) -> Path:
    directory = Path(models_dir) / ('loras' if method == 'lora' else 'checkpoints')
    directory.mkdir(parents=True, exist_ok=True)
    return directory


def publish_path(models_dir: Path, job: dict[str, Any]) -> Path:
    filename = weight_name(str(job['filename']), 'filename')
    return publish_directory(models_dir, job['method']) / filename


def pid_alive(pid: object) -> bool:
    if not isinstance(pid, int) or isinstance(pid, bool) or pid <= 0:
        return False
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    except OSError:
        return False
    return True


def unused_pid() -> int:
    pid = 2**22
    while pid < 2**31:
        if not pid_alive(pid):
            return pid
        pid += 1
    raise RuntimeError('could not find an unused pid')


def find_job(models_dir: Path) -> Path | None:
    root = Path(models_dir) / TRAIN_ROOT
    if not root.is_dir():
        return None
    queued: list[tuple[str, Path]] = []
    resumable: list[tuple[str, Path]] = []
    for entry in root.iterdir():
        if not entry.is_dir():
            continue
        path = entry / 'job.json'
        if not path.is_file():
            continue
        try:
            job = json.loads(path.read_text(encoding='utf-8'))
        except (OSError, ValueError):
            continue
        if not isinstance(job, dict):
            continue
        status = job.get('status')
        started = str(job.get('started_at') or '')
        if status == 'queued':
            queued.append((started, entry))
        elif status == 'running' and not pid_alive(job.get('pid')):
            resumable.append((started, entry))
    queued.sort()
    if queued:
        return queued[0][1]
    resumable.sort()
    if resumable:
        return resumable[0][1]
    return None


def mark_running(job_dir: Path, job: dict[str, Any]) -> dict[str, Any]:
    job['status'] = 'running'
    job['pid'] = os.getpid()
    job['error'] = None
    if not job.get('started_at'):
        job['started_at'] = now_rfc3339()
    write_job(job_dir, job)
    atomic_write(Path(job_dir) / 'pid', f'{os.getpid()}\n')
    return job


def mark_failed(job_dir: Path, job: dict[str, Any], error: BaseException | str) -> dict[str, Any]:
    job['status'] = 'failed'
    job['error'] = str(error)
    job['pid'] = None
    write_job(job_dir, job)
    return job


def mark_succeeded(job_dir: Path, job: dict[str, Any]) -> dict[str, Any]:
    job['status'] = 'succeeded'
    job['error'] = None
    job['pid'] = None
    write_job(job_dir, job)
    return job


def write_stub_weights(path: Path) -> None:
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        import torch
        from safetensors.torch import save_file

        temporary = path.with_name(path.name + '.tmp')
        save_file({'stub': torch.zeros(1)}, str(temporary))
        os.replace(temporary, path)
        return
    except ImportError:
        pass
    atomic_write(path, b'stub')


def publish_stub(models_dir: Path, job: dict[str, Any]) -> Path:
    destination = publish_path(models_dir, job)
    write_stub_weights(destination)
    write_sidecar(destination, job)
    return destination


def run_stub(models_dir: Path, job_dir: Path, job: dict[str, Any], config: dict[str, Any]) -> None:
    total = steps_for(int(job.get('image_count') or 0), config)
    progress = Progress(job_dir, str(job.get('method') or 'lora'), total)
    job['total'] = total
    job['step'] = 0
    progress.emit('loading', phase_step=1, phase_total=1)
    write_job(job_dir, job)
    progress.emit('publishing', step=0, phase_step=0, phase_total=1)
    publish_stub(models_dir, job)
    job['step'] = total
    progress.emit('publishing', step=total, phase_step=1, phase_total=1)
    write_job(job_dir, job)


def snapshot_path(directory: Path, step: int) -> Path:
    return directory / f'step-{step:06d}.pt'


def snapshot_steps(directory: Path) -> list[int]:
    if not directory.is_dir():
        return []
    steps = []
    for entry in directory.iterdir():
        name = entry.name
        if not name.startswith('step-') or not name.endswith('.pt'):
            continue
        try:
            steps.append(int(name[5:-3]))
        except ValueError:
            continue
    return sorted(steps)


def latest_snapshot(directory: Path) -> int | None:
    steps = snapshot_steps(directory)
    return steps[-1] if steps else None


def prune_snapshots(directory: Path, keep: int, best_step: int | None) -> None:
    retain = set(snapshot_steps(directory)[-max(int(keep), 0) :])
    if best_step is not None:
        retain.add(best_step)
    for step in snapshot_steps(directory):
        if step not in retain:
            snapshot_path(directory, step).unlink(missing_ok=True)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description='Zone host SDXL person trainer')
    parser.add_argument(
        '--models-dir',
        default=os.environ.get('COMFYUI_MODELS_DIR') or './models',
    )
    parser.add_argument('--config', default=str(CONFIG_PATH))
    parser.add_argument('--once', action='store_true')
    parser.add_argument('--stub', action='store_true')
    return parser.parse_args(argv)


def stub_enabled(args: argparse.Namespace) -> bool:
    if args.stub:
        return True
    return os.environ.get('ZONE_TRAIN_STUB', '') == '1'


def process_job(
    models_dir: Path,
    job_dir: Path,
    config: dict[str, Any],
    stub: bool,
) -> None:
    job = load_job(job_dir)
    try:
        job = normalize_job(job, job_dir)
        job = mark_running(job_dir, job)
        if stub:
            run_stub(models_dir, job_dir, job, config)
        else:
            train(models_dir, job_dir, job, config)
        mark_succeeded(job_dir, job)
    except Exception as error:
        mark_failed(job_dir, job, error)
        raise


def watch(models_dir: Path, config: dict[str, Any], once: bool, stub: bool) -> int:
    while True:
        job_dir = find_job(models_dir)
        if job_dir is None:
            if once:
                return 0
            time.sleep(WATCH_INTERVAL)
            continue
        try:
            print(f'processing {job_dir}', flush=True)
            process_job(models_dir, job_dir, config, stub)
            print(f'finished {job_dir}', flush=True)
        except Exception:
            traceback.print_exc()
            if once:
                return 1
        if once:
            return 0


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    config = load_config(Path(args.config))
    models_dir = Path(args.models_dir).expanduser()
    return watch(models_dir, config, once=args.once, stub=stub_enabled(args))


def select_device():
    import torch

    if hasattr(torch.backends, 'mps') and torch.backends.mps.is_available():
        return torch.device('mps')
    return torch.device('cpu')


def matching_targets(model: Any, candidates: list[str]) -> list[str]:
    names = [name for name, _ in model.named_modules()]
    found = []
    for target in candidates:
        if any(name == target or name.endswith('.' + target) for name in names):
            found.append(target)
    if not found:
        raise RuntimeError(f'no LoRA target modules matched: {candidates}')
    return found


def trainable_parameters(model: Any) -> list[Any]:
    return [parameter for parameter in model.parameters() if parameter.requires_grad]


def load_pixels(path: Path):
    import numpy as np
    import torch
    from PIL import Image

    image = Image.open(path).convert('RGB')
    width, height = image.size
    array = np.asarray(image, dtype=np.float32) / 255.0
    pixels = torch.from_numpy(array).permute(2, 0, 1).unsqueeze(0)
    return pixels * 2.0 - 1.0, height, width


def encode_latents(vae: Any, path: Path, device: Any):
    import torch

    pixels, height, width = load_pixels(path)
    pixels = pixels.to(device=device, dtype=torch.float32)
    with torch.no_grad():
        latents = vae.encode(pixels).latent_dist.sample()
        latents = latents * vae.config.scaling_factor
    return latents.detach().cpu(), height, width


def encode_prompt(
    prompt: str,
    tokenizer: Any,
    tokenizer_2: Any,
    text_encoder: Any,
    text_encoder_2: Any,
    device: Any,
    train_text_encoder: bool,
):
    import torch

    tokens = tokenizer(
        prompt,
        padding='max_length',
        max_length=tokenizer.model_max_length,
        truncation=True,
        return_tensors='pt',
    )
    tokens_2 = tokenizer_2(
        prompt,
        padding='max_length',
        max_length=tokenizer_2.model_max_length,
        truncation=True,
        return_tensors='pt',
    )
    ids = tokens.input_ids.to(device)
    ids_2 = tokens_2.input_ids.to(device)
    if train_text_encoder:
        output_1 = text_encoder(ids, output_hidden_states=True)
    else:
        with torch.no_grad():
            output_1 = text_encoder(ids, output_hidden_states=True)
    with torch.no_grad():
        output_2 = text_encoder_2(ids_2, output_hidden_states=True)
    hidden_1 = output_1.hidden_states[-2]
    hidden_2 = output_2.hidden_states[-2]
    pooled = getattr(output_2, 'text_embeds', None)
    if pooled is None:
        pooled = output_2[0]
    return torch.cat([hidden_1, hidden_2], dim=-1), pooled


def apply_lora(model: Any, targets: list[str], rank: int, alpha: int):
    from peft import LoraConfig, get_peft_model

    config = LoraConfig(
        r=int(rank),
        lora_alpha=int(alpha),
        target_modules=matching_targets(model, targets),
    )
    return get_peft_model(model, config)


def kohya_state_dict(unet: Any, text_encoder: Any, alpha: int):
    import torch
    try:
        from diffusers.utils import convert_state_dict_to_kohya
    except ImportError:
        from diffusers.utils.state_dict_utils import convert_state_dict_to_kohya
    from peft.utils import get_peft_model_state_dict

    def prefixed(prefix: str, state: dict[str, Any]) -> dict[str, Any]:
        converted = {}
        for key, value in state.items():
            for stale in ('base_model.model.', 'base_model.'):
                if key.startswith(stale):
                    key = key[len(stale) :]
                    break
            converted[f'{prefix}.{key}'] = value
        return converted

    combined = prefixed('unet', get_peft_model_state_dict(unet))
    te_keys = [name for name, _ in text_encoder.named_parameters()]
    if any('lora_A' in name or 'lora_B' in name or name.startswith('lora_') for name in te_keys):
        combined.update(prefixed('text_encoder', get_peft_model_state_dict(text_encoder)))
    kohya = convert_state_dict_to_kohya(combined)
    scale = torch.tensor(float(alpha))
    for key, value in list(kohya.items()):
        if key.endswith('.alpha'):
            kohya[key] = scale.to(device=value.device, dtype=value.dtype)
    return {key: value.detach().cpu().contiguous() for key, value in kohya.items()}


def save_safetensors(path: Path, tensors: dict[str, Any]) -> None:
    from safetensors.torch import save_file

    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + '.tmp')
    save_file(tensors, str(temporary))
    os.replace(temporary, path)


def ensure_class_images(
    pipeline: Any,
    class_dir: Path,
    count: int,
    prompt: str,
    device: Any,
    progress: Progress | None = None,
) -> list[tuple[Path, str]]:
    import torch

    class_dir.mkdir(parents=True, exist_ok=True)
    existing = sorted(
        path for path in class_dir.iterdir() if path.is_file() and path.suffix.lower() == '.png'
    )
    if progress is not None:
        progress.emit('class_images', phase_step=len(existing), phase_total=count)
    if len(existing) < count:
        pipeline = pipeline.to(device)
        for index in range(len(existing), count):
            with torch.inference_mode():
                image = pipeline(
                    prompt,
                    height=1024,
                    width=1024,
                    num_inference_steps=30,
                    guidance_scale=5.0,
                ).images[0]
            destination = class_dir / f'{index:04}.png'
            image.save(destination)
            print(f'class image {index + 1}/{count}', flush=True)
            if progress is not None:
                progress.emit('class_images', phase_step=index + 1, phase_total=count)
        existing = sorted(
            path
            for path in class_dir.iterdir()
            if path.is_file() and path.suffix.lower() == '.png'
        )
    return [(path, prompt) for path in existing[:count]]


def prediction_loss(
    unet: Any,
    scheduler: Any,
    latents: Any,
    prompt_embeds: Any,
    pooled: Any,
    height: int,
    width: int,
    device: Any,
):
    import torch
    import torch.nn.functional as functional

    latents = latents.to(device=device, dtype=torch.float32)
    noise = torch.randn_like(latents)
    timesteps = torch.randint(
        0,
        int(scheduler.config.num_train_timesteps),
        (latents.shape[0],),
        device=device,
        dtype=torch.long,
    )
    noisy = scheduler.add_noise(latents, noise, timesteps)
    add_time_ids = torch.tensor(
        [[height, width, 0, 0, height, width]],
        dtype=torch.float32,
        device=device,
    )
    if add_time_ids.shape[0] != latents.shape[0]:
        add_time_ids = add_time_ids.repeat(latents.shape[0], 1)
    added = {'text_embeds': pooled.to(device=device, dtype=torch.float32), 'time_ids': add_time_ids}
    predicted = unet(
        noisy,
        timesteps,
        encoder_hidden_states=prompt_embeds,
        added_cond_kwargs=added,
    ).sample
    if getattr(scheduler.config, 'prediction_type', 'epsilon') == 'v_prediction':
        target = scheduler.get_velocity(latents, noise, timesteps)
    else:
        target = noise
    return functional.mse_loss(predicted.float(), target.float())


def load_base_pipeline(checkpoint: Path, hf_base: str):
    import torch
    from diffusers import StableDiffusionXLPipeline

    kwargs: dict[str, Any] = {
        'torch_dtype': torch.float32,
        'use_safetensors': True,
    }
    if hf_base:
        try:
            return StableDiffusionXLPipeline.from_single_file(
                str(checkpoint), config=hf_base, **kwargs
            )
        except (OSError, TypeError, ValueError):
            pass
    return StableDiffusionXLPipeline.from_single_file(str(checkpoint), **kwargs)


def save_snapshot(
    path: Path,
    *,
    method: str,
    step: int,
    loss: float,
    best_loss: float,
    best_step: int | None,
    optimizer: Any,
    unet: Any,
    text_encoder: Any,
) -> None:
    import torch

    payload: dict[str, Any] = {
        'method': method,
        'step': step,
        'loss': loss,
        'best_loss': best_loss,
        'best_step': best_step,
        'optimizer': optimizer.state_dict(),
    }
    if method == 'lora':
        from peft.utils import get_peft_model_state_dict

        payload['unet_lora'] = get_peft_model_state_dict(unet)
        payload['text_encoder_lora'] = get_peft_model_state_dict(text_encoder)
    else:
        payload['unet'] = unet.state_dict()
        payload['text_encoder'] = text_encoder.state_dict()
    temporary = path.with_name(path.name + '.tmp')
    torch.save(payload, temporary)
    os.replace(temporary, path)


def restore_snapshot(
    path: Path,
    *,
    method: str,
    device: Any,
    optimizer: Any,
    unet: Any,
    text_encoder: Any,
) -> tuple[int, float, int | None]:
    import torch

    try:
        from peft.utils import set_peft_model_state_dict
    except ImportError:
        from peft import set_peft_model_state_dict

    try:
        payload = torch.load(path, map_location=device, weights_only=False)
    except TypeError:
        payload = torch.load(path, map_location=device)
    if method == 'lora':
        set_peft_model_state_dict(unet, payload['unet_lora'])
        if 'text_encoder_lora' in payload:
            set_peft_model_state_dict(text_encoder, payload['text_encoder_lora'])
    else:
        unet.load_state_dict(payload['unet'])
        text_encoder.load_state_dict(payload['text_encoder'])
    optimizer.load_state_dict(payload['optimizer'])
    return (
        int(payload['step']),
        float(payload.get('best_loss', payload.get('loss', float('inf')))),
        payload.get('best_step'),
    )


def publish_trained(
    models_dir: Path,
    job: dict[str, Any],
    *,
    method: str,
    unet: Any,
    vae: Any,
    text_encoder: Any,
    text_encoder_2: Any,
    alpha: int,
) -> Path:
    destination = publish_path(models_dir, job)
    if method == 'lora':
        save_safetensors(destination, kohya_state_dict(unet, text_encoder, alpha))
    else:
        from sdxl_checkpoint import pipeline_to_original, save_original_checkpoint

        save_original_checkpoint(
            destination,
            pipeline_to_original(unet, vae, text_encoder, text_encoder_2),
        )
    write_sidecar(destination, job)
    return destination


def train(models_dir: Path, job_dir: Path, job: dict[str, Any], config: dict[str, Any]) -> None:
    import torch
    from diffusers import DDPMScheduler

    method = job['method']
    checkpoint = Path(models_dir) / 'checkpoints' / job['checkpoint']
    if not checkpoint.is_file():
        raise FileNotFoundError(f'base checkpoint is missing: {checkpoint}')
    pairs = list_pairs(Path(job_dir) / 'dataset')
    if not pairs:
        raise RuntimeError(f'no png dataset images in {job_dir / "dataset"}')
    job['image_count'] = len(pairs)
    total = steps_for(len(pairs), config)
    job['total'] = total
    job['step'] = job.get('step') or 0
    write_job(job_dir, job)
    progress = Progress(job_dir, method, total)
    progress.emit('loading', phase_step=0, phase_total=1)

    device = select_device()
    print(f'device={device} method={method} steps={total} images={len(pairs)}', flush=True)
    pipeline = load_base_pipeline(checkpoint, str(job.get('hf_base') or ''))
    progress.emit('loading', phase_step=1, phase_total=1)
    unet = pipeline.unet
    vae = pipeline.vae
    text_encoder = pipeline.text_encoder
    text_encoder_2 = pipeline.text_encoder_2
    tokenizer = pipeline.tokenizer
    tokenizer_2 = pipeline.tokenizer_2
    try:
        scheduler = DDPMScheduler.from_config(pipeline.scheduler.config)
    except Exception:
        scheduler = DDPMScheduler(
            num_train_timesteps=1000,
            beta_start=0.00085,
            beta_end=0.012,
            beta_schedule='scaled_linear',
            prediction_type='epsilon',
            clip_sample=False,
        )

    class_pairs: list[tuple[Path, str]] = []
    if method == 'finetune' and bool(config.get('prior_preservation', True)):
        class_pairs = ensure_class_images(
            pipeline,
            Path(job_dir) / 'class',
            len(pairs),
            str(config.get('class_prompt') or 'a photo of a person'),
            device,
            progress,
        )

    vae.requires_grad_(False)
    text_encoder_2.requires_grad_(False)
    vae.eval()
    text_encoder_2.eval()
    train_text_encoder = bool(config.get('train_text_encoder', True))
    if bool(config.get('gradient_checkpointing', True)):
        unet.enable_gradient_checkpointing()
        if train_text_encoder and hasattr(text_encoder, 'gradient_checkpointing_enable'):
            text_encoder.gradient_checkpointing_enable()
            if hasattr(text_encoder, 'config'):
                text_encoder.config.use_cache = False

    if method == 'lora':
        unet = apply_lora(unet, UNET_TARGETS, int(config['rank']), int(config['alpha']))
        if train_text_encoder:
            text_encoder = apply_lora(
                text_encoder,
                TEXT_ENCODER_TARGETS,
                int(config['rank']),
                int(config['alpha']),
            )
        else:
            text_encoder.requires_grad_(False)
        groups = [
            {'params': trainable_parameters(unet), 'lr': float(config['unet_lr'])},
        ]
        if train_text_encoder:
            groups.append(
                {
                    'params': trainable_parameters(text_encoder),
                    'lr': float(config['text_encoder_lr']),
                }
            )
    else:
        unet.requires_grad_(True)
        if train_text_encoder:
            text_encoder.requires_grad_(True)
        else:
            text_encoder.requires_grad_(False)
        groups = [
            {
                'params': trainable_parameters(unet) + trainable_parameters(text_encoder),
                'lr': float(config['finetune_lr']),
            }
        ]
    groups = [group for group in groups if group['params']]
    optimizer = torch.optim.AdamW(groups)
    unet.to(device)
    text_encoder.to(device)
    text_encoder_2.to(device)
    vae.to(device)
    unet.train()
    if train_text_encoder:
        text_encoder.train()
    else:
        text_encoder.eval()

    encode_total = len(pairs) + len(class_pairs)
    encoded_instance: list[Any] = []
    encoded_class: list[Any] = []
    encoded = 0
    progress.emit('encoding', phase_step=0, phase_total=encode_total)
    for path, caption in pairs:
        encoded_instance.append(encode_latents(vae, path, device) + (caption,))
        encoded += 1
        progress.emit('encoding', phase_step=encoded, phase_total=encode_total)
    for path, caption in class_pairs:
        encoded_class.append(encode_latents(vae, path, device) + (caption,))
        encoded += 1
        progress.emit('encoding', phase_step=encoded, phase_total=encode_total)
    vae.to('cpu')

    checkpoint_dir = Path(job_dir) / 'checkpoints'
    checkpoint_dir.mkdir(parents=True, exist_ok=True)
    start_step = 1
    best_loss = float('inf')
    best_step: int | None = None
    resume_step = latest_snapshot(checkpoint_dir)
    if resume_step is not None:
        start_step, best_loss, best_step = restore_snapshot(
            snapshot_path(checkpoint_dir, resume_step),
            method=method,
            device=device,
            optimizer=optimizer,
            unet=unet,
            text_encoder=text_encoder,
        )
        start_step += 1
        print(f'resume from step {resume_step}', flush=True)
        progress.emit(
            'training',
            step=resume_step,
            phase_step=resume_step,
            phase_total=total,
            loss=best_loss if best_loss < float('inf') else None,
        )

    every = int(config.get('checkpoint_every') or 0)
    keep = int(config.get('keep_snapshots') or 2)
    for step in range(start_step, total + 1):
        optimizer.zero_grad(set_to_none=True)
        index = (step - 1) % len(encoded_instance)
        latents, height, width, caption = encoded_instance[index]
        prompt_embeds, pooled = encode_prompt(
            caption,
            tokenizer,
            tokenizer_2,
            text_encoder,
            text_encoder_2,
            device,
            train_text_encoder,
        )
        loss = prediction_loss(
            unet, scheduler, latents, prompt_embeds, pooled, height, width, device
        )
        if encoded_class:
            class_index = (step - 1) % len(encoded_class)
            class_latents, class_height, class_width, class_caption = encoded_class[class_index]
            class_embeds, class_pooled = encode_prompt(
                class_caption,
                tokenizer,
                tokenizer_2,
                text_encoder,
                text_encoder_2,
                device,
                train_text_encoder,
            )
            loss = loss + prediction_loss(
                unet,
                scheduler,
                class_latents,
                class_embeds,
                class_pooled,
                class_height,
                class_width,
                device,
            )
        loss.backward()
        optimizer.step()
        value = float(loss.detach().cpu())
        job['step'] = step
        job['total'] = total
        progress.emit('training', step=step, phase_step=step, phase_total=total, loss=value)
        if step == start_step or step % 10 == 0 or step == total:
            write_job(job_dir, job)
            print(f'step {step}/{total} loss={value:.4f}', flush=True)
        if value < best_loss:
            best_loss = value
            best_step = step
        if every and (step % every == 0 or step == total):
            save_snapshot(
                snapshot_path(checkpoint_dir, step),
                method=method,
                step=step,
                loss=value,
                best_loss=best_loss,
                best_step=best_step,
                optimizer=optimizer,
                unet=unet,
                text_encoder=text_encoder,
            )
            write_json(
                checkpoint_dir / 'best.json',
                {'step': best_step, 'loss': best_loss},
            )
            prune_snapshots(checkpoint_dir, keep, best_step)

    progress.emit('publishing', step=total, phase_step=0, phase_total=1)
    destination = publish_trained(
        models_dir,
        job,
        method=method,
        unet=unet,
        vae=vae,
        text_encoder=text_encoder,
        text_encoder_2=text_encoder_2,
        alpha=int(config['alpha']),
    )
    progress.emit('publishing', step=total, phase_step=1, phase_total=1)
    print(f'wrote {destination}', flush=True)


if __name__ == '__main__':
    sys.exit(main())
