#!/usr/bin/env python3
"""Host LaunchAgent worker for SDXL person LoRA, pivotal, and UNet fine-tunes."""

from __future__ import annotations

import argparse
import errno
import gc
import json
import math
import os
import random
import shutil
import struct
import sys
import threading
import time
import traceback
import uuid
import zlib
from collections.abc import Callable
from collections.abc import MutableMapping
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

METAL_HIGH_WATERMARK_VARIABLE = 'PYTORCH_MPS_HIGH_WATERMARK_RATIO'
METAL_LOW_WATERMARK_VARIABLE = 'PYTORCH_MPS_LOW_WATERMARK_RATIO'
METAL_WATERMARK_VARIABLES = (METAL_HIGH_WATERMARK_VARIABLE, METAL_LOW_WATERMARK_VARIABLE)
METAL_HIGH_WATERMARK = '1.0'
METAL_LOW_WATERMARK = '0.8'
METAL_WATERMARK_TOO_LATE = (
    'warning: torch was imported before train_sdxl, so the Metal memory watermarks '
    'it sets may be ignored and an overcommit can hang macOS'
)


def metal_watermark_unreadable(variable: str, value: str) -> str:
    return f'warning: {variable}={value!r} is not a number, so train_sdxl leaves the Metal cap alone'


def apply_metal_watermarks(environment: MutableMapping[str, str] = os.environ) -> None:
    changed = METAL_HIGH_WATERMARK_VARIABLE not in environment
    environment.setdefault(METAL_HIGH_WATERMARK_VARIABLE, METAL_HIGH_WATERMARK)
    try:
        high = float(environment[METAL_HIGH_WATERMARK_VARIABLE])
        low = environment.get(METAL_LOW_WATERMARK_VARIABLE)
        if high > 0 and (low is None or float(low) > high):
            environment[METAL_LOW_WATERMARK_VARIABLE] = str(min(high, float(METAL_LOW_WATERMARK)))
            changed = True
    except ValueError:
        for variable in METAL_WATERMARK_VARIABLES:
            if variable in environment:
                print(metal_watermark_unreadable(variable, environment[variable]), file=sys.stderr)
        return
    if changed and 'torch' in sys.modules:
        print(METAL_WATERMARK_TOO_LATE, file=sys.stderr)


apply_metal_watermarks()

ROOT = Path(__file__).resolve().parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

WATCH_INTERVAL = 2
TRANSIENT_ATTEMPT_LIMIT = 3
TRANSIENT_ATTEMPTS_KEY = 'transient_attempts'
METAL_OUT_OF_MEMORY = 'MPS backend out of memory'
DEFAULT_HF_BASE = 'John6666/lustify-sdxl-nsfw-checkpoint-ggwp-v7-sdxl'
DEFAULT_CHECKPOINT = 'lustifySDXLNSFW_ggwpV7.safetensors'
CONFIG_PATH = Path(__file__).with_name('train_sdxl_config.json')
TRAIN_ROOT = '.zone-train'
CLAIM_FILE = 'claimed'
LORAS = 'loras'
CHECKPOINTS = 'checkpoints'
SIDECAR_SUFFIX = '.zone.json'
GENERATION = 'generation'
LANGUAGE_SUBJECT = 'language'
LATEST_SNAPSHOT = 'latest.pt'
STILL_METHODS = {'lora', 'finetune', 'pivotal'}
ADAPTER_METHODS = {'lora', 'pivotal'}
FINETUNE_METHOD = 'finetune'
CUDA_DEVICE = 'cuda'
MPS_DEVICE = 'mps'
HOST_DEVICE = 'cpu'
FINETUNE_MIN_MEMORY = 40 * 1024**3
ADAPTER_MIN_MEMORY = 20 * 1024**3
TRAINABLE_COPIES = 4
UPDATE_GRADIENT_COPIES = 3
MOMENT_KEYS = ('exp_avg', 'exp_avg_sq')
PRODIGY_OPTIMIZER = 'prodigy'
ADAMW_OPTIMIZER = 'adamw'
MASK_THRESHOLD = 0.2
KIND_BOOST = 1.25
REBALANCE_SHARE = 0.40
CLASS_POSE_BANK = (
    'standing',
    'sitting',
    'lying on back',
    'from behind',
    'hands visible',
    'full body',
    'portrait',
    'walking',
    'profile',
    'three quarter view',
    'crouching',
    'looking at camera',
    'close-up face',
    'wide shot',
    'arms at sides',
    'looking away',
)
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
    'subject',
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
    if method == FINETUNE_METHOD:
        return 'sdxl'
    return 'sdxl-adapter'


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


def is_frame_png(path: Path) -> bool:
    name = path.name.lower()
    return path.is_file() and name.endswith('.png') and not name.endswith('.mask.png')


def count_images(dataset: Path) -> int:
    if not dataset.is_dir():
        return 0
    return sum(1 for path in dataset.iterdir() if is_frame_png(path))


def list_frames(dataset: Path) -> list[dict[str, Any]]:
    if not dataset.is_dir():
        return []
    frames = []
    for png in sorted(path for path in dataset.iterdir() if is_frame_png(path)):
        stem = png.stem
        caption_path = png.with_suffix('.txt')
        caption = caption_path.read_text(encoding='utf-8').strip() if caption_path.is_file() else ''
        mask = png.with_name(f'{stem}.mask.png')
        kind_path = png.with_name(f'{stem}.kind')
        pose_path = png.with_name(f'{stem}.pose')
        kind = kind_path.read_text(encoding='utf-8').strip().lower() if kind_path.is_file() else ''
        pose = pose_path.read_text(encoding='utf-8').strip().lower() if pose_path.is_file() else ''
        frames.append(
            {
                'image': png,
                'caption': caption,
                'mask': mask if mask.is_file() else None,
                'kind': kind,
                'pose': pose,
            }
        )
    return frames


def list_pairs(dataset: Path) -> list[tuple[Path, str]]:
    return [(frame['image'], frame['caption']) for frame in list_frames(dataset)]


def drop_caption(
    caption: str,
    class_prompt: str,
    dropout: float,
    rng: random.Random,
) -> str:
    if dropout <= 0 or rng.random() >= dropout:
        return caption
    return class_prompt


def masked_mse(
    predicted: list[float],
    target: list[float],
    mask: list[float] | None = None,
    threshold: float = MASK_THRESHOLD,
) -> float:
    total = 0.0
    count = 0
    if mask is None:
        for left, right in zip(predicted, target):
            delta = left - right
            total += delta * delta
            count += 1
        return total / count if count else 0.0
    for left, right, weight in zip(predicted, target, mask):
        if weight < threshold:
            continue
        delta = left - right
        total += delta * delta
        count += 1
    return total / count if count else 0.0


def cap_cluster_weights(
    weights: list[float],
    cluster_ids: list[str],
    share: float,
) -> list[float]:
    clusters: dict[str, list[int]] = {}
    for index, cluster in enumerate(cluster_ids):
        clusters.setdefault(cluster, []).append(index)
    if len(clusters) <= 1 or share >= 1:
        return list(weights)
    if (1.0 / len(clusters)) >= share:
        return list(weights)
    capped = list(weights)
    masses = {
        cluster: sum(capped[index] for index in indices)
        for cluster, indices in clusters.items()
    }
    changed = True
    while changed:
        changed = False
        total = sum(masses.values())
        if total <= 0:
            break
        for cluster, mass in masses.items():
            if mass / total <= share + 1e-12:
                continue
            rest = total - mass
            if rest <= 0:
                continue
            target = share * rest / (1.0 - share)
            if target >= mass or mass <= 0:
                continue
            scale = target / mass
            for index in clusters[cluster]:
                capped[index] *= scale
            masses[cluster] = target
            changed = True
            break
    return capped


def pose_sample_weights(
    frames: list[dict[str, Any]],
    config: dict[str, Any] | None = None,
) -> list[float]:
    config = config or {}
    count = len(frames)
    if count == 0:
        return []
    if not any(str(frame.get('pose') or '') for frame in frames):
        return [1.0] * count
    boost = float(config.get('kind_boost') if config.get('kind_boost') is not None else KIND_BOOST)
    share = float(
        config.get('rebalance_share')
        if config.get('rebalance_share') is not None
        else REBALANCE_SHARE
    )
    clusters: dict[str, list[int]] = {}
    cluster_ids = []
    for index, frame in enumerate(frames):
        cluster = str(frame.get('pose') or 'unknown')
        cluster_ids.append(cluster)
        clusters.setdefault(cluster, []).append(index)
    weights = [0.0] * count
    for indices in clusters.values():
        inverse = 1.0 / len(indices)
        for index in indices:
            kind = str(frames[index].get('kind') or '')
            extra = boost if kind in {'hand', 'head'} else 1.0
            weights[index] = inverse * extra
    return cap_cluster_weights(weights, cluster_ids, share)


def weighted_index(weights: list[float], rng: random.Random) -> int:
    total = 0.0
    for weight in weights:
        total += weight
    if not weights:
        raise ValueError('no sample weights')
    if total <= 0:
        return rng.randrange(len(weights))
    pick = rng.random() * total
    running = 0.0
    last = 0
    for index, weight in enumerate(weights):
        running += weight
        last = index
        if pick < running:
            return index
    return last


def class_prompt_for(
    base: str,
    index: int,
    bank: tuple[str, ...] | list[str] | None = None,
) -> str:
    poses = tuple(bank) if bank is not None else CLASS_POSE_BANK
    if not poses:
        return base
    return f'{base}, {poses[index % len(poses)]}'


def class_cache_path(models_dir: Path, config: dict[str, Any]) -> Path:
    relative = str(config.get('class_cache_dir') or '.zone-class/person')
    path = Path(relative)
    if path.is_absolute():
        return path
    return Path(models_dir) / path


def list_class_pngs(class_dir: Path) -> list[Path]:
    if not class_dir.is_dir():
        return []
    return sorted(path for path in class_dir.iterdir() if is_frame_png(path))


def class_pairs(paths: list[Path], fallback: str) -> list[tuple[Path, str]]:
    pairs = []
    for path in paths:
        caption_path = path.with_suffix('.txt')
        if caption_path.is_file():
            caption = caption_path.read_text(encoding='utf-8').strip() or fallback
        else:
            caption = fallback
        pairs.append((path, caption))
    return pairs


def optimizer_kind(method: str, config: dict[str, Any]) -> str:
    if method == FINETUNE_METHOD:
        return str(config.get('finetune_optimizer') or ADAMW_OPTIMIZER).strip().lower()
    return str(config.get('optimizer') or PRODIGY_OPTIMIZER).strip().lower()


def load_prodigy() -> Any:
    try:
        from prodigyopt import Prodigy
    except ImportError as error:
        raise RuntimeError(
            'Prodigy is not installed in the train venv; pip install prodigyopt'
        ) from error
    return Prodigy


PRODIGY_LR = 1.0


def prodigy_param_groups(groups: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Every group gets Prodigy's documented `lr` of 1.0, so no group keeps its own rate.

    prodigyopt 1.0 through 1.1.2 steps every group at `d * max(group lr)`:
    `lr` only multiplies the one adaptive D estimate. A group whose `lr` is
    neither that maximum nor 0 makes `step()` raise `Setting different lr
    values in different parameter groups is only supported for values of 0`.
    A per-group rate cannot be expressed, so CLIP-L moves at the UNet's
    adaptive step and `unet_lr` / `text_encoder_lr` only shape AdamW.
    """
    return [{**group, 'lr': PRODIGY_LR} for group in groups]


def prodigy_rate_keys(method: str) -> tuple[str, ...]:
    if method == FINETUNE_METHOD:
        return ('finetune_lr',)
    return ('unet_lr', 'text_encoder_lr')


def prodigy_rate_notice(
    method: str,
    groups: list[dict[str, Any]],
    config: dict[str, Any],
) -> str:
    configured = [f'{key}={config[key]}' for key in prodigy_rate_keys(method) if key in config]
    rates = ', '.join(configured or [f"lr={group.get('lr')}" for group in groups])
    return (
        f'prodigy ignores {rates}: every parameter group steps at one adaptive '
        f'rate (lr={PRODIGY_LR}), so the text encoder moves as fast as the unet'
    )


def make_optimizer(method: str, groups: list[dict[str, Any]], config: dict[str, Any]) -> Any:
    kind = optimizer_kind(method, config)
    if kind == PRODIGY_OPTIMIZER:
        if any(group.get('lr') != PRODIGY_LR for group in groups):
            print(prodigy_rate_notice(method, groups, config), flush=True)
        return load_prodigy()(prodigy_param_groups(groups))
    import torch

    return torch.optim.AdamW(groups)


def offloads_moments(device: Any, kind: str) -> bool:
    return kind == ADAMW_OPTIMIZER and device_kind(device) == MPS_DEVICE


def move_moments(state: dict[str, Any], device: Any) -> None:
    for key in MOMENT_KEYS:
        if key in state:
            state[key] = state[key].to(device)


def offload_optimizer_moments(optimizer: Any) -> None:
    for state in optimizer.state.values():
        move_moments(state, HOST_DEVICE)


@dataclass(frozen=True)
class Losses:
    parts: list[Any]
    total: Any


def combine_losses(parts: list[Any]) -> Losses:
    total = parts[0]
    for part in parts[1:]:
        total = total + part
    return Losses(parts=parts, total=total)


class StepInBackward:
    """Step AdamW one parameter at a time as backward finishes its gradient, then drop it.

    The full gradient set is never resident. With offload, the moments live on the host and
    visit the parameter's device only for its update, and every loss part after the first is
    backpropagated first with its finished gradients stashed on the host, so autograd never
    buffers them on the device while it waits for the first part.
    """

    def __init__(self, optimizer: Any, enabled: bool, offload: bool = False) -> None:
        self.optimizer = optimizer
        self.enabled = enabled
        self.offload = enabled and offload
        self.handles: list[Any] = []
        self.stashed: dict[Any, Any] = {}
        self.stashing = False

    def __enter__(self) -> StepInBackward:
        if self.offload:
            offload_optimizer_moments(self.optimizer)
            release_device_cache(MPS_DEVICE)
        if self.enabled:
            self.handles = [
                parameter.register_post_accumulate_grad_hook(self.update)
                for group in self.optimizer.param_groups
                for parameter in group['params']
            ]
        return self

    def __exit__(self, *details: object) -> None:
        for handle in self.handles:
            handle.remove()
        self.handles = []
        self.stashed.clear()

    def backward(self, losses: Losses) -> None:
        if not self.offload:
            losses.total.backward()
            return
        # Bit-identical to one backward through the total only while each parameter is used at
        # most once per part: float addition commutes but does not associate.
        self.stashed.clear()
        self.stashing = True
        try:
            for part in reversed(losses.parts[1:]):
                part.backward()
        finally:
            self.stashing = False
        losses.parts[0].backward()
        self.step_stashed()

    def update(self, parameter: Any) -> None:
        # Every other gradient is None here (zero_grad(set_to_none=True) runs before backward and
        # each hook clears its own), so step() skips them and updates only this parameter.
        if self.stashing:
            self.stash(parameter)
        elif self.offload:
            self.merge_stashed(parameter)
            self.step_on_device(parameter)
        else:
            self.optimizer.step()
        parameter.grad = None

    def stash(self, parameter: Any) -> None:
        gradient = parameter.grad
        previous = self.stashed.get(parameter)
        if previous is not None:
            gradient = previous.to(gradient.device) + gradient
        self.stashed[parameter] = gradient.to(HOST_DEVICE)

    def merge_stashed(self, parameter: Any) -> None:
        stashed = self.stashed.pop(parameter, None)
        if stashed is not None:
            parameter.grad = stashed.to(parameter.device) + parameter.grad

    def step_stashed(self) -> None:
        while self.stashed:
            parameter, stashed = self.stashed.popitem()
            parameter.grad = stashed.to(parameter.device)
            self.step_on_device(parameter)
            parameter.grad = None

    def step_on_device(self, parameter: Any) -> None:
        state = self.optimizer.state[parameter]
        move_moments(state, parameter.device)
        self.optimizer.step()
        move_moments(state, HOST_DEVICE)

    def step(self) -> None:
        if not self.enabled:
            self.optimizer.step()


def embedding_filename(job: dict[str, Any]) -> str:
    filename = str(job.get('filename') or default_filename(str(job.get('name') or 'person')))
    return f'{Path(filename).stem}.safetensors'


def embedding_path(models_dir: Path, job: dict[str, Any]) -> Path:
    return Path(models_dir) / 'embeddings' / embedding_filename(job)


def placeholder_png_bytes() -> bytes:
    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack('>I', len(data))
            + tag
            + data
            + struct.pack('>I', zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    ihdr = struct.pack('>IIBBBBB', 1, 1, 8, 2, 0, 0, 0)
    idat = zlib.compress(b'\x00\x00\x00\x00', 9)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', ihdr) + chunk(b'IDAT', idat) + chunk(b'IEND', b'')


def preview_relpath(step: int, index: int) -> str:
    return f'previews/step-{step}-{index}.png'


def write_preview_placeholders(
    job_dir: Path,
    step: int,
    count: int = 4,
) -> list[str]:
    directory = Path(job_dir) / 'previews'
    directory.mkdir(parents=True, exist_ok=True)
    payload = placeholder_png_bytes()
    paths = []
    for index in range(count):
        relative = preview_relpath(step, index)
        atomic_write(Path(job_dir) / relative, payload)
        paths.append(relative)
    return paths


def preview_specs(trigger: str) -> list[tuple[int, str]]:
    subject = f'{trigger} person' if (trigger or '').strip() else 'a person'
    return [
        (1000, f'identity portrait of {subject}, face close-up, looking at camera, studio light'),
        (1001, f'full body photo of {subject}, standing, entire figure visible, even light'),
        (1002, f'photo of {subject}, hands visible, detailed hands, medium shot'),
        (1003, f'profile photo of {subject}, side view, head and shoulders'),
    ]


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
    if method not in STILL_METHODS:
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
    job['image_count'] = count_images(Path(job_dir) / 'dataset')
    return job


PHASE_WEIGHTS = {
    'lora': (
        ('loading', 6),
        ('class_images', 8),
        ('encoding', 8),
        ('training', 72),
        ('publishing', 6),
    ),
    'pivotal': (
        ('loading', 6),
        ('class_images', 8),
        ('encoding', 8),
        ('training', 72),
        ('publishing', 6),
    ),
    'finetune': (
        ('loading', 4),
        ('class_images', 12),
        ('encoding', 6),
        ('training', 72),
        ('publishing', 6),
    ),
    'language': (
        ('converting', 10),
        ('loading', 10),
        ('training', 70),
        ('publishing', 10),
    ),
    'other': (
        ('loading', 10),
        ('encoding', 10),
        ('training', 70),
        ('publishing', 10),
    ),
}


def overall_percent(method: str, phase: str, phase_step: int, phase_total: int) -> int:
    if phase in {'queued', 'screening', 'captioning'}:
        return 0
    weights = PHASE_WEIGHTS.get(method, PHASE_WEIGHTS['lora'])
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


def phase_message(
    phase: str, phase_step: int, phase_total: int, method: str = ''
) -> str:
    if phase == 'queued':
        return 'Waiting for the host trainer'
    if phase == 'converting':
        return 'Converting the dataset'
    if phase == 'loading':
        if method == 'language':
            return 'Loading the language model'
        if method == 'other':
            return 'Loading the image checkpoint'
        if method == 'video':
            return 'Loading the video checkpoint'
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
        self.method = method if method in PHASE_WEIGHTS else 'lora'
        self.total = max(int(total), 1)
        self.phase = 'queued'
        self.phase_started = time.monotonic()
        self.previews: list[str] = []

    def emit(
        self,
        phase: str,
        *,
        step: int | None = None,
        phase_step: int = 0,
        phase_total: int = 0,
        loss: float | None = None,
        previews: list[str] | None = None,
    ) -> None:
        if phase != self.phase:
            self.phase = phase
            self.phase_started = time.monotonic()
        if previews is not None:
            self.previews = list(previews)
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
            'message': phase_message(
                phase, int(phase_step), int(phase_total), method=self.method
            ),
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
        if self.previews:
            payload['previews'] = self.previews
        write_progress(self.job_dir, payload['step'], payload['total'], **{
            key: value for key, value in payload.items() if key not in {'step', 'total'}
        })


def write_progress(job_dir: Path, step: int, total: int, **extra: Any) -> None:
    payload: dict[str, Any] = {'step': int(step), 'total': int(total)}
    payload.update(extra)
    write_json(Path(job_dir) / 'progress.json', payload, pretty=False)


def weight_path(job_dir: Path, job: dict[str, Any]) -> Path:
    return Path(job_dir) / weight_name(str(job['filename']), 'filename')


def legacy(job: dict[str, Any]) -> bool:
    """A job queued before abnegate-comfy has no `base`, and its trainer publishes it."""
    return not job.get('base') and str(job.get('subject') or '') != LANGUAGE_SUBJECT


def legacy_destination(models_dir: Path, job: dict[str, Any]) -> Path:
    folder = CHECKPOINTS if job.get('method') == FINETUNE_METHOD else LORAS
    return Path(models_dir) / folder / weight_name(str(job['filename']), 'filename')


def sidecar_path(weight: Path) -> Path:
    return weight.with_name(weight.name + SIDECAR_SUFFIX)


def sidecar_payload(job: dict[str, Any]) -> dict[str, Any]:
    payload = {
        'recipe_id': job.get('recipe_id') or recipe_for(job.get('method') or 'lora'),
        'hf_base': job.get('hf_base') or DEFAULT_HF_BASE,
        'trigger': job.get('trigger') or '',
    }
    if (job.get('method') or '') == 'pivotal':
        payload['embedding'] = embedding_filename(job)
    return payload


def with_generation(sidecar: dict[str, Any]) -> dict[str, Any]:
    """abnegate-comfy replaces a weight only when its sidecar names a `generation`."""
    return {**sidecar, GENERATION: str(uuid.uuid4())}


def legacy_publish(
    models_dir: Path,
    job_dir: Path,
    job: dict[str, Any],
    sidecar: dict[str, Any],
) -> Path:
    destination = legacy_destination(models_dir, job)
    destination.parent.mkdir(parents=True, exist_ok=True)
    os.replace(weight_path(job_dir, job), destination)
    write_json(sidecar_path(destination), with_generation(sidecar))
    return destination


def succeed(
    models_dir: Path,
    job_dir: Path,
    job: dict[str, Any],
    sidecar: dict[str, Any],
) -> dict[str, Any]:
    if legacy(job):
        legacy_publish(models_dir, job_dir, job, sidecar)
    return mark_succeeded(job_dir, job)


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
        if status == 'queued' and not (entry / CLAIM_FILE).exists():
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


def claim(job_dir: Path, job: dict[str, Any]) -> bool:
    # abnegate-comfy creates the same claim before it gives up on a queued job;
    # a running job was claimed by this trainer before it crashed.
    if job.get('status') != 'queued':
        return job.get('status') == 'running'
    try:
        descriptor = os.open(
            Path(job_dir) / CLAIM_FILE, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600
        )
    except FileExistsError:
        return False
    os.close(descriptor)
    return True


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


def transient_failure(attempts: int, error: BaseException) -> str:
    if METAL_OUT_OF_MEMORY in str(error):
        return (
            f'Metal ran out of memory on {attempts} attempts in a row without saving a new '
            'snapshot. The trainer caps Metal at its recommended working set so an overcommit '
            'fails instead of hanging macOS; do not raise or disable '
            f'{METAL_HIGH_WATERMARK_VARIABLE}. Quit other GPU apps such as ComfyUI or Ollama, '
            f'or train on Runpod. Last error: {error}'
        )
    return (
        f'ran out of memory on {attempts} attempts in a row without saving a new snapshot; '
        f'last error: {error}'
    )


def record_transient_crash(
    job_dir: Path, job: dict[str, Any], error: BaseException
) -> dict[str, Any]:
    attempts = int(job.get(TRANSIENT_ATTEMPTS_KEY) or 0) + 1
    job[TRANSIENT_ATTEMPTS_KEY] = attempts
    if attempts >= TRANSIENT_ATTEMPT_LIMIT:
        return mark_failed(job_dir, job, transient_failure(attempts, error))
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


def write_stub(models_dir: Path, job_dir: Path, job: dict[str, Any]) -> Path:
    destination = weight_path(job_dir, job)
    write_stub_weights(destination)
    if job.get('method') == 'pivotal':
        write_stub_weights(embedding_path(models_dir, job))
    return destination


def run_stub(models_dir: Path, job_dir: Path, job: dict[str, Any], config: dict[str, Any]) -> None:
    total = steps_for(int(job.get('image_count') or 0), config)
    progress = Progress(job_dir, str(job.get('method') or 'lora'), total)
    job['total'] = total
    job['step'] = 0
    progress.emit('loading', phase_step=1, phase_total=1)
    write_job(job_dir, job)
    class_count = int(config.get('class_cache_count') or 256)
    progress.emit('class_images', phase_step=class_count, phase_total=class_count)
    encoded = int(job.get('image_count') or 0)
    progress.emit('encoding', phase_step=encoded, phase_total=max(encoded, 1))
    every = int(config.get('checkpoint_every') or 0)
    snapshot = total if not every else max(every, (total // every) * every)
    preview_count = int(config.get('preview_count') or 4)
    previews = write_preview_placeholders(job_dir, snapshot, preview_count)
    progress.emit(
        'training',
        step=total,
        phase_step=total,
        phase_total=total,
        previews=previews,
    )
    progress.emit('publishing', step=0, phase_step=0, phase_total=1)
    write_stub(models_dir, job_dir, job)
    job['step'] = total
    progress.emit('publishing', step=total, phase_step=1, phase_total=1)
    write_job(job_dir, job)


def snapshot_path(directory: Path, step: int) -> Path:
    return directory / f'step-{step:06d}.pt'


def latest_snapshot_file(directory: Path) -> Path:
    return Path(directory) / LATEST_SNAPSHOT


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


def resume_snapshot_path(directory: Path) -> Path | None:
    latest = latest_snapshot_file(directory)
    try:
        if latest.is_file() and latest.stat().st_size > 0:
            return latest
    except OSError:
        pass
    step = latest_snapshot(directory)
    if step is None:
        return None
    return snapshot_path(directory, step)


def copy_snapshot(source: Path, destination: Path) -> None:
    source = Path(source)
    destination = Path(destination)
    if source.resolve() == destination.resolve():
        return
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(destination.name + '.tmp')
    temporary.unlink(missing_ok=True)
    try:
        os.link(source, temporary)
    except OSError:
        shutil.copy2(source, temporary)
    os.replace(temporary, destination)


def prune_snapshots(directory: Path, keep: int, best_step: int | None) -> None:
    retain = set(snapshot_steps(directory)[-max(int(keep), 0) :])
    if best_step is not None:
        retain.add(best_step)
    for step in snapshot_steps(directory):
        if step not in retain:
            snapshot_path(directory, step).unlink(missing_ok=True)


def clear_incomplete_saves(*directories: Path) -> None:
    for directory in directories:
        if not directory.is_dir():
            continue
        for entry in directory.iterdir():
            if entry.name.endswith('.tmp'):
                entry.unlink(missing_ok=True)


def is_transient_crash(error: BaseException) -> bool:
    if isinstance(error, MemoryError):
        return True
    if isinstance(error, OSError) and getattr(error, 'errno', None) == errno.ENOMEM:
        return True
    if isinstance(error, RuntimeError):
        message = str(error).lower()
        if 'out of memory' in message or 'not enough memory' in message:
            return True
    return False


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description='Zone host SDXL person trainer')
    parser.add_argument(
        '--models-dir',
        default=os.environ.get('COMFYUI_MODELS_DIRECTORY') or './models',
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
    if (job.get('provider') or '') == 'runpod':
        import train_runpod

        train_runpod.process_job(models_dir, job_dir, config, stub)
        return
    if (job.get('subject') or '') == 'language':
        import train_llm

        train_llm.process_job(models_dir, job_dir, stub)
        return
    if (job.get('subject') or '') == 'other':
        import train_flux

        train_flux.process_job(models_dir, job_dir, stub)
        return
    if (job.get('method') or '') == 'video':
        import train_wan

        train_wan.process_job(models_dir, job_dir, stub)
        return
    if not claim(job_dir, job):
        return
    try:
        job = normalize_job(job, job_dir)
        job = mark_running(job_dir, job)
        if stub:
            run_stub(models_dir, job_dir, job, config)
        else:
            train(models_dir, job_dir, job, config)
        succeed(models_dir, job_dir, job, sidecar_payload(job))
    except Exception as error:
        if is_transient_crash(error):
            record_transient_crash(job_dir, job, error)
        else:
            mark_failed(job_dir, job, error)
        raise
    finally:
        wait_for_latest_save()


def run_job(models_dir: Path, job_dir: Path, config: dict[str, Any], stub: bool) -> bool:
    try:
        print(f'processing {job_dir}', flush=True)
        process_job(models_dir, job_dir, config, stub)
        print(f'finished {job_dir}', flush=True)
        return True
    except Exception:
        traceback.print_exc()
        return False


def release_memory() -> None:
    gc.collect()
    if 'torch' in sys.modules:
        release_device_cache(select_device())


def watch(models_dir: Path, config: dict[str, Any], once: bool, stub: bool) -> int:
    while True:
        job_dir = find_job(models_dir)
        if job_dir is None:
            if once:
                return 0
            time.sleep(WATCH_INTERVAL)
            continue
        if run_job(models_dir, job_dir, config, stub):
            if once:
                return 0
            continue
        release_memory()
        if once:
            return 1
        time.sleep(WATCH_INTERVAL)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    config = load_config(Path(args.config))
    models_dir = Path(args.models_dir).expanduser()
    return watch(models_dir, config, once=args.once, stub=stub_enabled(args))


def device_kind(device: Any) -> str:
    kind = getattr(device, 'type', None)
    if isinstance(kind, str) and kind:
        return kind
    text = str(device)
    if not text:
        return 'cpu'
    return text.split(':', 1)[0]


def select_device() -> Any:
    import torch

    if torch.cuda.is_available():
        torch.backends.cuda.matmul.allow_tf32 = False
        torch.backends.cudnn.allow_tf32 = False
        return torch.device(CUDA_DEVICE)
    if hasattr(torch.backends, 'mps') and torch.backends.mps.is_available():
        return torch.device(MPS_DEVICE)
    return torch.device(HOST_DEVICE)


def assert_device_capacity(device: Any, method: str) -> None:
    kind = device_kind(device)
    if kind == CUDA_DEVICE:
        assert_cuda_capacity(device, method)
    elif kind == MPS_DEVICE and method == FINETUNE_METHOD:
        assert_metal_capacity()


def assert_cuda_capacity(device: Any, method: str) -> None:
    import torch

    required = FINETUNE_MIN_MEMORY if method == FINETUNE_METHOD else ADAPTER_MIN_MEMORY
    index = getattr(device, 'index', None)
    total = int(torch.cuda.get_device_properties(0 if index is None else index).total_memory)
    if total >= required:
        return
    if method == FINETUNE_METHOD:
        raise RuntimeError(
            'this recipe needs a 48 GB card for fine-tune; refusing to drop precision'
        )
    raise RuntimeError('this recipe needs a 24 GB card for LoRA; refusing to drop precision')


def gibibytes(size: int) -> str:
    return f'{size / 1024**3:.1f} GiB'


def metal_refusal(job: str, required: int, available: int) -> str:
    return (
        f'{job} needs {gibibytes(required)} of Metal working memory but this Mac reports '
        f'{gibibytes(available)} (torch.mps.recommended_max_memory); refusing to start '
        'rather than overcommit Metal or drop precision, so train it on Runpod instead'
    )


def assert_metal_capacity() -> None:
    import torch

    available = int(torch.mps.recommended_max_memory())
    if available < FINETUNE_MIN_MEMORY:
        raise RuntimeError(metal_refusal('an SDXL fine-tune', FINETUNE_MIN_MEMORY, available))


def unique_parameters(models: list[Any]) -> list[Any]:
    seen: set[int] = set()
    parameters = []
    for model in models:
        for parameter in model.parameters():
            if id(parameter) not in seen:
                seen.add(id(parameter))
                parameters.append(parameter)
    return parameters


def parameter_bytes(parameters: list[Any]) -> int:
    return sum(int(parameter.numel()) * int(parameter.element_size()) for parameter in parameters)


def trainable_working_set(trainable: list[Any], offload: bool) -> int:
    if not offload:
        return TRAINABLE_COPIES * parameter_bytes(trainable)
    largest = max((parameter_bytes([parameter]) for parameter in trainable), default=0)
    return parameter_bytes(trainable) + (UPDATE_GRADIENT_COPIES + len(MOMENT_KEYS)) * largest


def metal_working_set(models: list[Any], offload: bool = False) -> int:
    parameters = unique_parameters(models)
    trainable = [parameter for parameter in parameters if parameter.requires_grad]
    frozen = [parameter for parameter in parameters if not parameter.requires_grad]
    return trainable_working_set(trainable, offload) + parameter_bytes(frozen)


def assert_metal_working_set(device: Any, models: list[Any], offload: bool = False) -> None:
    if device_kind(device) != MPS_DEVICE:
        return
    import torch

    required = metal_working_set(models, offload)
    available = int(torch.mps.recommended_max_memory())
    if required > available:
        raise RuntimeError(metal_refusal('this job', required, available))


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


def unique_param_groups(groups: list[dict[str, Any]]) -> list[dict[str, Any]]:
    seen: set[int] = set()
    unique = []
    for group in groups:
        params = []
        for parameter in group['params']:
            identity = id(parameter)
            if identity in seen:
                continue
            seen.add(identity)
            params.append(parameter)
        if params:
            unique.append({**group, 'params': params})
    return unique


def load_pixels(path: Path):
    import numpy as np
    import torch
    from PIL import Image

    image = Image.open(path).convert('RGB')
    width, height = image.size
    array = np.asarray(image, dtype=np.float32) / 255.0
    pixels = torch.from_numpy(array).permute(2, 0, 1).unsqueeze(0)
    return pixels * 2.0 - 1.0, height, width


def load_latent_mask(path: Path, height: int, width: int):
    import numpy as np
    import torch
    from PIL import Image

    image = Image.open(path).convert('L')
    array = np.asarray(image, dtype=np.float32) / 255.0
    tensor = torch.from_numpy(array).unsqueeze(0).unsqueeze(0)
    latent_height = max(int(height) // 8, 1)
    latent_width = max(int(width) // 8, 1)
    if tensor.shape[-2] != latent_height or tensor.shape[-1] != latent_width:
        tensor = torch.nn.functional.interpolate(
            tensor, size=(latent_height, latent_width), mode='nearest'
        )
    return tensor[0, 0]


def encode_latents(vae: Any, path: Path, device: Any):
    import torch

    pixels, height, width = load_pixels(path)
    pixels = pixels.to(device=device, dtype=torch.float32)
    with torch.no_grad():
        latents = vae.encode(pixels).latent_dist.sample()
        latents = latents * vae.config.scaling_factor
    return latents.detach().cpu(), height, width


def latent_sidecar(path: Path) -> Path:
    path = Path(path)
    return path.with_name(path.stem + '.latent.pt')


def sidecar_is_current(sidecar: Path, image: Path) -> bool:
    try:
        return sidecar.is_file() and sidecar.stat().st_mtime >= image.stat().st_mtime
    except OSError:
        return False


def write_latent_sidecar(path: Path, latents: Any, height: int, width: int) -> None:
    import torch

    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + '.tmp')
    torch.save(
        {
            'latents': latents.detach().cpu(),
            'height': int(height),
            'width': int(width),
        },
        temporary,
    )
    os.replace(temporary, path)


def read_latent_sidecar(path: Path) -> tuple[Any, int, int] | None:
    import torch

    try:
        try:
            payload = torch.load(path, map_location='cpu', weights_only=True)
        except TypeError:
            payload = torch.load(path, map_location='cpu')
    except Exception:
        return None
    if not isinstance(payload, dict) or 'latents' not in payload:
        return None
    try:
        return payload['latents'], int(payload['height']), int(payload['width'])
    except (KeyError, TypeError, ValueError):
        return None


def load_or_encode_latents(vae: Any, path: Path, device: Any):
    sidecar = latent_sidecar(path)
    if sidecar_is_current(sidecar, path):
        loaded = read_latent_sidecar(sidecar)
        if loaded is not None:
            return loaded
    latents, height, width = encode_latents(vae, path, device)
    write_latent_sidecar(sidecar, latents, height, width)
    return latents, height, width


def release_device_cache(device: Any) -> None:
    import torch

    if device_kind(device) == MPS_DEVICE and hasattr(torch, 'mps'):
        torch.mps.empty_cache()


class UncachedPromptError(KeyError):
    def __init__(self, prompt: str) -> None:
        super().__init__(f'no cached text_encoder_2 output for prompt {prompt!r}')
        self.prompt = prompt

    def __str__(self) -> str:
        return str(self.args[0])


@dataclass(frozen=True)
class SecondaryEmbedding:
    hidden: Any
    pooled: Any


@dataclass(frozen=True)
class PromptCache:
    embeddings: dict[str, SecondaryEmbedding]

    def __len__(self) -> int:
        return len(self.embeddings)

    def lookup(self, prompt: str, device: Any) -> tuple[Any, Any]:
        embedding = self.embeddings.get(prompt)
        if embedding is None:
            raise UncachedPromptError(prompt)
        return embedding.hidden.to(device, copy=True), embedding.pooled.to(device, copy=True)


def training_prompts(
    instance_captions: list[str],
    class_prompt: str,
    class_captions: list[str],
) -> list[str]:
    return list(dict.fromkeys([*instance_captions, class_prompt, *class_captions]))


def encode_secondary(
    prompt: str,
    tokenizer_2: Any,
    text_encoder_2: Any,
    device: Any,
) -> tuple[Any, Any]:
    import torch

    tokens_2 = tokenizer_2(
        prompt,
        padding='max_length',
        max_length=tokenizer_2.model_max_length,
        truncation=True,
        return_tensors='pt',
    )
    ids_2 = tokens_2.input_ids.to(device)
    with torch.no_grad():
        output_2 = text_encoder_2(ids_2, output_hidden_states=True)
    pooled = getattr(output_2, 'text_embeds', None)
    if pooled is None:
        pooled = output_2[0]
    return output_2.hidden_states[-2], pooled


def cache_secondary_embeddings(
    prompts: list[str],
    tokenizer_2: Any,
    text_encoder_2: Any,
    device: Any,
) -> PromptCache:
    """Encode every prompt the loop can request once on the device, then park CLIP-G on the host.

    CLIP-G is frozen and in eval mode, so its output depends only on the prompt and draws no
    random numbers; an uncached prompt raises because a host forward differs bitwise from Metal.
    """
    embeddings: dict[str, SecondaryEmbedding] = {}
    for prompt in dict.fromkeys(prompts):
        hidden, pooled = encode_secondary(prompt, tokenizer_2, text_encoder_2, device)
        embeddings[prompt] = SecondaryEmbedding(hidden.detach().cpu(), pooled.detach().cpu())
    text_encoder_2.to(HOST_DEVICE)
    release_device_cache(device)
    return PromptCache(embeddings)


def encode_prompt(
    prompt: str,
    tokenizer: Any,
    text_encoder: Any,
    secondary: PromptCache,
    device: Any,
    train_text_encoder: bool,
) -> tuple[Any, Any]:
    import torch

    tokens = tokenizer(
        prompt,
        padding='max_length',
        max_length=tokenizer.model_max_length,
        truncation=True,
        return_tensors='pt',
    )
    ids = tokens.input_ids.to(device)
    if train_text_encoder:
        output_1 = text_encoder(ids, output_hidden_states=True)
    else:
        with torch.no_grad():
            output_1 = text_encoder(ids, output_hidden_states=True)
    hidden_1 = output_1.hidden_states[-2]
    hidden_2, pooled = secondary.lookup(prompt, device)
    return torch.cat([hidden_1, hidden_2], dim=-1), pooled


def apply_lora(model: Any, targets: list[str], rank: int, alpha: int):
    from peft import LoraConfig, get_peft_model

    config = LoraConfig(
        r=int(rank),
        lora_alpha=int(alpha),
        target_modules=matching_targets(model, targets),
    )
    return get_peft_model(model, config)


def install_trigger(tokenizer: Any, text_encoder: Any, trigger: str) -> int:
    import torch

    trigger = (trigger or '').strip()
    if not trigger:
        raise ValueError('pivotal training needs a trigger word')
    added = tokenizer.add_tokens(trigger)
    if added:
        text_encoder.resize_token_embeddings(len(tokenizer))
    ids = tokenizer.encode(trigger, add_special_tokens=False)
    if not ids:
        raise ValueError(f'could not encode trigger {trigger!r}')
    token_id = int(ids[0])
    person_ids = tokenizer.encode('person', add_special_tokens=False)
    with torch.no_grad():
        weight = text_encoder.get_input_embeddings().weight
        if person_ids:
            source = weight[torch.tensor(person_ids, device=weight.device)]
            if source.ndim > 1:
                source = source.mean(dim=0)
            weight[token_id] = source
    return token_id


def trigger_embedding_tensors(text_encoder: Any, token_id: int, trigger: str) -> dict[str, Any]:
    weight = text_encoder.get_input_embeddings().weight[token_id].detach().cpu().contiguous()
    if weight.ndim == 1:
        weight = weight.unsqueeze(0)
    return {trigger: weight}


def restore_frozen_embeddings(text_encoder: Any, original: Any, token_id: int) -> None:
    import torch

    embed = text_encoder.get_input_embeddings()
    with torch.no_grad():
        rows = embed.weight.shape[0]
        restore = torch.ones(rows, dtype=torch.bool, device=embed.weight.device)
        if 0 <= token_id < rows:
            restore[token_id] = False
        embed.weight.data[restore] = original[restore]


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
    pose_bank: tuple[str, ...] | list[str] | None = None,
) -> list[tuple[Path, str]]:
    class_dir.mkdir(parents=True, exist_ok=True)
    existing = list_class_pngs(class_dir)
    if progress is not None:
        progress.emit('class_images', phase_step=min(len(existing), count), phase_total=count)
    if len(existing) >= count:
        if progress is not None:
            progress.emit('class_images', phase_step=count, phase_total=count)
        return class_pairs(existing[:count], prompt)
    pipeline = pipeline.to(device)
    bank = tuple(pose_bank) if pose_bank is not None else CLASS_POSE_BANK
    for index in range(len(existing), count):
        item_prompt = class_prompt_for(prompt, index, bank)
        image = pipeline(
            item_prompt,
            height=1024,
            width=1024,
            num_inference_steps=30,
            guidance_scale=5.0,
        ).images[0]
        destination = class_dir / f'{index:04}.png'
        image.save(destination)
        atomic_write(destination.with_suffix('.txt'), item_prompt + '\n')
        print(f'class image {index + 1}/{count}', flush=True)
        if progress is not None:
            progress.emit('class_images', phase_step=index + 1, phase_total=count)
    existing = list_class_pngs(class_dir)
    return class_pairs(existing[:count], prompt)


def compute_snr(scheduler: Any, timesteps: Any):
    import torch

    alphas = scheduler.alphas_cumprod.to(device=timesteps.device, dtype=torch.float32)
    alpha = alphas[timesteps]
    sigma = (1.0 - alpha).clamp(min=1e-8)
    return alpha / sigma


def reduce_prediction_error(error: Any, mask: Any | None, threshold: float) -> Any:
    if mask is None:
        return error.mean()
    mask = mask.to(device=error.device, dtype=error.dtype)
    if mask.ndim == 2:
        mask = mask[None, None, :, :]
    elif mask.ndim == 3:
        mask = mask[:, None, :, :]
    active = (mask >= threshold).to(error.dtype)
    active = active.expand_as(error)
    denom = active.sum().clamp(min=1.0)
    return (error * active).sum() / denom


def prediction_loss(
    unet: Any,
    scheduler: Any,
    latents: Any,
    prompt_embeds: Any,
    pooled: Any,
    height: int,
    width: int,
    device: Any,
    mask: Any | None = None,
    noise_offset: float = 0.0,
    min_snr_gamma: float = 0.0,
    mask_threshold: float = MASK_THRESHOLD,
):
    import torch
    import torch.nn.functional as functional

    latents = latents.to(device=device, dtype=torch.float32)
    noise = torch.randn_like(latents)
    if noise_offset:
        noise = noise + float(noise_offset) * torch.randn(
            latents.shape[0],
            latents.shape[1],
            1,
            1,
            device=latents.device,
            dtype=latents.dtype,
        )
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
    error = functional.mse_loss(predicted.float(), target.float(), reduction='none')
    if min_snr_gamma:
        snr = compute_snr(scheduler, timesteps)
        gamma = float(min_snr_gamma)
        if getattr(scheduler.config, 'prediction_type', 'epsilon') == 'v_prediction':
            weight = torch.clamp(snr, max=gamma) / (snr + 1.0)
        else:
            weight = torch.clamp(snr, max=gamma) / snr.clamp(min=1e-8)
        error = error * weight.view(-1, 1, 1, 1)
    return reduce_prediction_error(error, mask, mask_threshold)


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


def python_rng_state(value: Any) -> Any:
    if isinstance(value, list):
        return tuple(python_rng_state(item) for item in value)
    return value


def capture_rng_state(
    sample_rng: random.Random,
    dropout_rng: random.Random,
) -> dict[str, Any]:
    import torch

    payload: dict[str, Any] = {
        'sample_rng': sample_rng.getstate(),
        'dropout_rng': dropout_rng.getstate(),
        'torch_rng': torch.get_rng_state(),
    }
    if hasattr(torch, 'mps') and hasattr(torch.mps, 'get_rng_state'):
        try:
            payload['torch_mps_rng'] = torch.mps.get_rng_state()
        except (RuntimeError, AttributeError):
            pass
    return payload


def apply_rng_state(
    payload: dict[str, Any],
    sample_rng: random.Random | None = None,
    dropout_rng: random.Random | None = None,
) -> None:
    if sample_rng is not None and payload.get('sample_rng') is not None:
        sample_rng.setstate(python_rng_state(payload['sample_rng']))
    if dropout_rng is not None and payload.get('dropout_rng') is not None:
        dropout_rng.setstate(python_rng_state(payload['dropout_rng']))
    import torch

    if payload.get('torch_rng') is not None:
        torch.set_rng_state(payload['torch_rng'].cpu())
    if payload.get('torch_mps_rng') is not None and hasattr(torch, 'mps'):
        try:
            torch.mps.set_rng_state(payload['torch_mps_rng'].cpu())
        except (RuntimeError, AttributeError):
            pass


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
    token_id: int | None = None,
    sample_rng: random.Random | None = None,
    dropout_rng: random.Random | None = None,
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
    if sample_rng is not None and dropout_rng is not None:
        payload.update(capture_rng_state(sample_rng, dropout_rng))
    if method in ADAPTER_METHODS:
        from peft.utils import get_peft_model_state_dict

        payload['unet_lora'] = get_peft_model_state_dict(unet)
        payload['text_encoder_lora'] = get_peft_model_state_dict(text_encoder)
        if method == 'pivotal' and token_id is not None:
            payload['token_id'] = token_id
            payload['trigger_embedding'] = (
                text_encoder.get_input_embeddings().weight[token_id].detach().cpu()
            )
    else:
        payload['unet'] = unet.state_dict()
        payload['text_encoder'] = text_encoder.state_dict()
    temporary = path.with_name(path.name + '.tmp')
    torch.save(payload, temporary)
    os.replace(temporary, path)


def clone_host_tensors(values: dict[str, Any]) -> None:
    import torch

    for key, value in list(values.items()):
        if isinstance(value, torch.Tensor) and value.device.type == HOST_DEVICE:
            values[key] = value.clone()


def release_snapshot_storage(optimizer: Any) -> None:
    """Copy the optimizer's host tensors out of the memory-mapped snapshot.

    load_state_dict leaves tensors that stay on the host, such as AdamW's step counters, as
    views of the mapping, which would keep a replaced latest.pt alive for the whole run.
    """
    for state in optimizer.state.values():
        clone_host_tensors(state)
    for group in optimizer.param_groups:
        clone_host_tensors(group)


def restore_snapshot(
    path: Path,
    *,
    method: str,
    device: Any,
    optimizer: Any,
    unet: Any,
    text_encoder: Any,
    token_id: int | None = None,
    sample_rng: random.Random | None = None,
    dropout_rng: random.Random | None = None,
    host_moments: bool = False,
) -> tuple[int, float, int | None]:
    """Restore a snapshot; with host_moments the optimizer state never lands on the device.

    load_state_dict casts every moment onto its parameter's device, so the trained modules wait
    on the host while it runs and go back to the device afterwards as the same parameters.
    """
    if host_moments:
        unet.to(HOST_DEVICE)
        text_encoder.to(HOST_DEVICE)
    try:
        return load_snapshot(
            path,
            method=method,
            device=HOST_DEVICE if host_moments else device,
            optimizer=optimizer,
            unet=unet,
            text_encoder=text_encoder,
            token_id=token_id,
            sample_rng=sample_rng,
            dropout_rng=dropout_rng,
        )
    finally:
        if host_moments:
            unet.to(device)
            text_encoder.to(device)
            release_device_cache(device)


def load_snapshot(
    path: Path,
    *,
    method: str,
    device: Any,
    optimizer: Any,
    unet: Any,
    text_encoder: Any,
    token_id: int | None,
    sample_rng: random.Random | None,
    dropout_rng: random.Random | None,
) -> tuple[int, float, int | None]:
    import torch

    try:
        from peft.utils import set_peft_model_state_dict
    except ImportError:
        from peft import set_peft_model_state_dict

    try:
        payload = torch.load(path, map_location=HOST_DEVICE, mmap=True, weights_only=False)
    except TypeError:
        payload = torch.load(path, map_location=HOST_DEVICE)
    if method in ADAPTER_METHODS:
        set_peft_model_state_dict(unet, payload['unet_lora'])
        if 'text_encoder_lora' in payload:
            set_peft_model_state_dict(text_encoder, payload['text_encoder_lora'])
        saved_token = payload.get('token_id', token_id)
        if method == 'pivotal' and saved_token is not None and 'trigger_embedding' in payload:
            with torch.no_grad():
                text_encoder.get_input_embeddings().weight[int(saved_token)] = payload[
                    'trigger_embedding'
                ].to(device=device)
    else:
        unet.load_state_dict(payload['unet'])
        text_encoder.load_state_dict(payload['text_encoder'])
    optimizer.load_state_dict(payload['optimizer'])
    release_snapshot_storage(optimizer)
    apply_rng_state(payload, sample_rng=sample_rng, dropout_rng=dropout_rng)
    return (
        int(payload['step']),
        float(payload.get('best_loss', payload.get('loss', float('inf')))),
        payload.get('best_step'),
    )


_latest_save_lock = threading.Lock()
_latest_save_thread: threading.Thread | None = None


def wait_for_latest_save() -> None:
    global _latest_save_thread
    thread = _latest_save_thread
    if thread is not None:
        thread.join()
    with _latest_save_lock:
        if _latest_save_thread is thread:
            _latest_save_thread = None


def start_latest_save(path: Path, **kwargs: Any) -> None:
    global _latest_save_thread

    def run() -> None:
        try:
            save_snapshot(path, **kwargs)
        except Exception:
            traceback.print_exc()

    with _latest_save_lock:
        current = _latest_save_thread
        if current is not None and current.is_alive():
            return
        thread = threading.Thread(target=run, name='zone-latest-pt', daemon=True)
        _latest_save_thread = thread
        thread.start()


@dataclass
class Cadence:
    interval: float
    clock: Callable[[], float] = time.monotonic
    marked: float | None = None

    def due(self) -> bool:
        return self.marked is None or self.clock() - self.marked >= self.interval

    def mark(self) -> None:
        self.marked = self.clock()


def latest_interval(config: dict[str, Any]) -> float:
    interval = float(config.get('latest_interval_seconds') or 0)
    if not math.isfinite(interval):
        raise ValueError(f'latest_interval_seconds must be finite, got {interval!r}')
    return max(0.0, interval)


def persist_training_step(
    checkpoint_dir: Path,
    job_dir: Path,
    job: dict[str, Any],
    *,
    method: str,
    step: int,
    total: int,
    loss: float,
    best_loss: float,
    best_step: int | None,
    optimizer: Any,
    unet: Any,
    text_encoder: Any,
    token_id: int | None,
    sample_rng: random.Random,
    dropout_rng: random.Random,
    every: int,
    keep: int,
    pipeline: Any,
    device: Any,
    config: dict[str, Any],
    progress: Progress,
    cadence: Cadence | None = None,
) -> None:
    latest = latest_snapshot_file(checkpoint_dir)
    numbered = bool(every) and (step % every == 0 or step == total)
    snapshot: dict[str, Any] = {
        'method': method,
        'step': step,
        'loss': loss,
        'best_loss': best_loss,
        'best_step': best_step,
        'optimizer': optimizer,
        'unet': unet,
        'text_encoder': text_encoder,
        'token_id': token_id,
        'sample_rng': sample_rng,
        'dropout_rng': dropout_rng,
    }
    if cadence is None:
        cadence = Cadence(0)
    if device_kind(device) == CUDA_DEVICE:
        if method == FINETUNE_METHOD and not numbered:
            start_latest_save(latest, **snapshot)
        else:
            wait_for_latest_save()
            save_snapshot(latest, **snapshot)
        job[TRANSIENT_ATTEMPTS_KEY] = 0
    elif numbered or step == total or cadence.due():
        cadence.mark()
        save_snapshot(latest, **snapshot)
        job[TRANSIENT_ATTEMPTS_KEY] = 0
    job['step'] = step
    job['total'] = total
    write_job(job_dir, job)
    if numbered:
        copy_snapshot(latest, snapshot_path(checkpoint_dir, step))
        write_json(
            checkpoint_dir / 'best.json',
            {'step': best_step, 'loss': best_loss},
        )
        prune_snapshots(checkpoint_dir, keep, best_step)
        write_snapshot_previews(
            pipeline,
            job_dir,
            step,
            str(job.get('trigger') or ''),
            device,
            config,
            progress,
        )


def render_previews(
    pipeline: Any,
    job_dir: Path,
    step: int,
    trigger: str,
    device: Any,
    config: dict[str, Any],
) -> list[str]:
    import torch

    count = int(config.get('preview_count') or 4)
    specs = preview_specs(trigger)[:count]
    preview_dir = Path(job_dir) / 'previews'
    preview_dir.mkdir(parents=True, exist_ok=True)
    steps = max(int(config.get('preview_steps') or 25), 1)
    size = int(config.get('resolution') or 1024)
    paths: list[str] = []
    was_training = pipeline.unet.training
    try:
        pipeline.unet.eval()
        if hasattr(pipeline, 'text_encoder') and pipeline.text_encoder is not None:
            pipeline.text_encoder.eval()
        pipeline.vae.to(device)
        pipeline.text_encoder_2.to(device)
        with torch.inference_mode():
            for index, (seed, prompt) in enumerate(specs):
                generator = torch.Generator(device='cpu').manual_seed(int(seed))
                image = pipeline(
                    prompt,
                    height=size,
                    width=size,
                    num_inference_steps=steps,
                    guidance_scale=5.0,
                    generator=generator,
                ).images[0]
                relative = preview_relpath(step, index)
                image.save(Path(job_dir) / relative)
                paths.append(relative)
    finally:
        pipeline.vae.to(HOST_DEVICE)
        pipeline.text_encoder_2.to(HOST_DEVICE)
        if was_training:
            pipeline.unet.train()
            if hasattr(pipeline, 'text_encoder') and pipeline.text_encoder is not None:
                pipeline.text_encoder.train()
    return paths


def write_snapshot_previews(
    pipeline: Any,
    job_dir: Path,
    step: int,
    trigger: str,
    device: Any,
    config: dict[str, Any],
    progress: Progress,
) -> None:
    count = int(config.get('preview_count') or 4)
    try:
        paths = render_previews(pipeline, job_dir, step, trigger, device, config)
    except Exception:
        traceback.print_exc()
        paths = write_preview_placeholders(job_dir, step, count)
    progress.previews = paths


def save_trained(
    models_dir: Path,
    job_dir: Path,
    job: dict[str, Any],
    *,
    method: str,
    unet: Any,
    vae: Any,
    text_encoder: Any,
    text_encoder_2: Any,
    alpha: int,
    token_id: int | None = None,
) -> Path:
    destination = weight_path(job_dir, job)
    if method in ADAPTER_METHODS:
        save_safetensors(destination, kohya_state_dict(unet, text_encoder, alpha))
        if method == 'pivotal':
            if token_id is None:
                raise ValueError('pivotal publishing needs the trained trigger embedding')
            save_safetensors(
                embedding_path(models_dir, job),
                trigger_embedding_tensors(text_encoder, token_id, str(job.get('trigger') or '')),
            )
    else:
        from sdxl_checkpoint import pipeline_to_original, save_original_checkpoint

        save_original_checkpoint(
            destination,
            pipeline_to_original(unet, vae, text_encoder, text_encoder_2),
        )
    return destination


def train(models_dir: Path, job_dir: Path, job: dict[str, Any], config: dict[str, Any]) -> None:
    import torch
    from diffusers import DDPMScheduler

    method = job['method']
    checkpoint = Path(models_dir) / CHECKPOINTS / job['checkpoint']
    if not checkpoint.is_file():
        raise FileNotFoundError(f'base checkpoint is missing: {checkpoint}')
    frames = list_frames(Path(job_dir) / 'dataset')
    if not frames:
        raise RuntimeError(f'no png dataset images in {job_dir / "dataset"}')
    job['image_count'] = len(frames)
    total = steps_for(len(frames), config)
    job['total'] = total
    if resume_snapshot_path(Path(job_dir) / 'checkpoints') is None:
        job['step'] = 0
    else:
        job['step'] = job.get('step') or 0
    write_job(job_dir, job)
    progress = Progress(job_dir, method, total)
    progress.emit('loading', phase_step=0, phase_total=1)

    device = select_device()
    assert_device_capacity(device, method)
    print(f'device={device} method={method} steps={total} images={len(frames)}', flush=True)
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

    class_prompt = str(config.get('class_prompt') or 'a photo of a person')
    class_pairs_list: list[tuple[Path, str]] = []
    if bool(config.get('prior_preservation', True)):
        class_pairs_list = ensure_class_images(
            pipeline,
            class_cache_path(models_dir, config),
            int(config.get('class_cache_count') or 256),
            class_prompt,
            device,
            progress,
        )
    else:
        progress.emit('class_images', phase_step=1, phase_total=1)

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

    token_id: int | None = None
    original_embeddings = None
    if method == 'pivotal':
        token_id = install_trigger(tokenizer, text_encoder, str(job.get('trigger') or ''))

    if method in ADAPTER_METHODS:
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
        if method == 'pivotal' and token_id is not None:
            embed = text_encoder.get_input_embeddings()
            embed.weight.requires_grad_(True)
            original_embeddings = embed.weight.data.clone()
            groups.append(
                {
                    'params': [embed.weight],
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
    groups = unique_param_groups([group for group in groups if group['params']])
    optimizer = make_optimizer(method, groups, config)
    kind = optimizer_kind(method, config)
    offload = offloads_moments(device, kind)
    print(f'optimizer={kind} offload_moments={offload}', flush=True)
    assert_metal_working_set(device, [unet, text_encoder, text_encoder_2], offload)
    pipeline.unet = unet
    pipeline.text_encoder = text_encoder
    unet.to(device)
    text_encoder.to(device)
    text_encoder_2.to(device)
    vae.to(device)
    unet.train()
    if train_text_encoder or method == 'pivotal':
        text_encoder.train()
    else:
        text_encoder.eval()

    encode_total = len(frames) + len(class_pairs_list)
    encoded_instance: list[Any] = []
    encoded_class: list[Any] = []
    encoded = 0
    checkpoint_dir = Path(job_dir) / 'checkpoints'
    checkpoint_dir.mkdir(parents=True, exist_ok=True)
    clear_incomplete_saves(
        checkpoint_dir,
        Path(job_dir) / 'dataset',
        class_cache_path(models_dir, config) if class_pairs_list else Path(job_dir),
    )
    progress.emit('encoding', phase_step=0, phase_total=encode_total)
    for frame in frames:
        latents, height, width = load_or_encode_latents(vae, frame['image'], device)
        mask = None
        if frame['mask'] is not None:
            mask = load_latent_mask(frame['mask'], height, width)
        encoded_instance.append((latents, height, width, frame['caption'], mask))
        encoded += 1
        progress.emit('encoding', phase_step=encoded, phase_total=encode_total)
    for path, caption in class_pairs_list:
        encoded_class.append(load_or_encode_latents(vae, path, device) + (caption, None))
        encoded += 1
        progress.emit('encoding', phase_step=encoded, phase_total=encode_total)
    vae.to(HOST_DEVICE)
    release_device_cache(device)
    secondary = cache_secondary_embeddings(
        training_prompts(
            [frame['caption'] for frame in frames],
            class_prompt,
            [caption for _, caption in class_pairs_list],
        ),
        tokenizer_2,
        text_encoder_2,
        device,
    )
    print(f'cached text_encoder_2 outputs for {len(secondary)} prompts', flush=True)

    every = int(config.get('checkpoint_every') or 0)
    keep = int(config.get('keep_snapshots') or 2)
    cadence = Cadence(latest_interval(config))
    dropout = float(config.get('caption_dropout') or 0.0)
    mask_threshold = float(
        config.get('mask_threshold')
        if config.get('mask_threshold') is not None
        else MASK_THRESHOLD
    )
    noise_offset = float(config.get('noise_offset') or 0.0)
    min_snr_gamma = float(config.get('min_snr_gamma') or 0.0)
    prior_weight = float(config.get('prior_loss_weight') or 1.0)
    seed = int(config.get('seed') or 0)
    sample_rng = random.Random(seed)
    dropout_rng = random.Random(seed ^ 0x9E3779B9)
    start_step = 1
    best_loss = float('inf')
    best_step: int | None = None
    resume_path = resume_snapshot_path(checkpoint_dir)
    if resume_path is not None:
        start_step, best_loss, best_step = restore_snapshot(
            resume_path,
            method=method,
            device=device,
            optimizer=optimizer,
            unet=unet,
            text_encoder=text_encoder,
            token_id=token_id,
            sample_rng=sample_rng,
            dropout_rng=dropout_rng,
            host_moments=offload,
        )
        resume_step = start_step
        start_step += 1
        job['step'] = resume_step
        write_job(job_dir, job)
        cadence.mark()
        print(f'resume from step {resume_step}', flush=True)
        progress.emit(
            'training',
            step=resume_step,
            phase_step=resume_step,
            phase_total=total,
            loss=best_loss if best_loss < float('inf') else None,
        )

    weights = pose_sample_weights(frames, config)
    te_trainable = train_text_encoder or method == 'pivotal'
    with StepInBackward(optimizer, offload, offload) as stepper:
        for step in range(start_step, total + 1):
            optimizer.zero_grad(set_to_none=True)
            index = weighted_index(weights, sample_rng)
            latents, height, width, caption, mask = encoded_instance[index]
            caption = drop_caption(caption, class_prompt, dropout, dropout_rng)
            prompt_embeds, pooled = encode_prompt(
                caption,
                tokenizer,
                text_encoder,
                secondary,
                device,
                te_trainable,
            )
            parts = [
                prediction_loss(
                    unet,
                    scheduler,
                    latents,
                    prompt_embeds,
                    pooled,
                    height,
                    width,
                    device,
                    mask=mask,
                    noise_offset=noise_offset,
                    min_snr_gamma=min_snr_gamma,
                    mask_threshold=mask_threshold,
                )
            ]
            if encoded_class:
                class_index = (step - 1) % len(encoded_class)
                class_latents, class_height, class_width, class_caption, _class_mask = (
                    encoded_class[class_index]
                )
                class_embeds, class_pooled = encode_prompt(
                    class_caption,
                    tokenizer,
                    text_encoder,
                    secondary,
                    device,
                    te_trainable,
                )
                parts.append(
                    prior_weight
                    * prediction_loss(
                        unet,
                        scheduler,
                        class_latents,
                        class_embeds,
                        class_pooled,
                        class_height,
                        class_width,
                        device,
                        noise_offset=noise_offset,
                        min_snr_gamma=min_snr_gamma,
                        mask_threshold=mask_threshold,
                    )
                )
            losses = combine_losses(parts)
            stepper.backward(losses)
            stepper.step()
            if original_embeddings is not None and token_id is not None:
                restore_frozen_embeddings(text_encoder, original_embeddings, token_id)
            value = float(losses.total.detach().cpu())
            if value < best_loss:
                best_loss = value
                best_step = step
            persist_training_step(
                checkpoint_dir,
                job_dir,
                job,
                method=method,
                step=step,
                total=total,
                loss=value,
                best_loss=best_loss,
                best_step=best_step,
                optimizer=optimizer,
                unet=unet,
                text_encoder=text_encoder,
                token_id=token_id,
                sample_rng=sample_rng,
                dropout_rng=dropout_rng,
                every=every,
                keep=keep,
                pipeline=pipeline,
                device=device,
                config=config,
                progress=progress,
                cadence=cadence,
            )
            progress.emit('training', step=step, phase_step=step, phase_total=total, loss=value)
            if step == start_step or step % 10 == 0 or step == total:
                print(f'step {step}/{total} loss={value:.4f}', flush=True)

    wait_for_latest_save()
    progress.emit('publishing', step=total, phase_step=0, phase_total=1)
    destination = save_trained(
        models_dir,
        job_dir,
        job,
        method=method,
        unet=unet,
        vae=vae,
        text_encoder=text_encoder,
        text_encoder_2=text_encoder_2,
        alpha=int(config['alpha']),
        token_id=token_id,
    )
    progress.emit('publishing', step=total, phase_step=1, phase_total=1)
    print(f'wrote {destination}', flush=True)


if __name__ == '__main__':
    sys.exit(main())
