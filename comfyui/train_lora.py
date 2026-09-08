#!/usr/bin/env python3
"""Run the repository-owned FLUX or Qwen Image Edit LoRA training graph."""

from __future__ import annotations

import json
import os
import re
import shutil
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from dataclasses import dataclass
from pathlib import Path

CONFIG_PATH = Path(__file__).with_name('custom_nodes') / 'zone_lora' / 'train_config.json'
MIN_WEIGHT_BYTES = 10_000
RUN_PATTERN = re.compile(
    r'zone-(?:train|probe)-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}'
)
ARTIFACT_PATTERN = re.compile(
    r'zone-lora-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}'
)


@dataclass(frozen=True)
class TrainingModel:
    architecture: str
    checkpoint: str = ''
    unet: str = ''
    clip: str = ''
    vae: str = ''

    @classmethod
    def from_environment(cls) -> 'TrainingModel':
        architecture = env('ZONE_TRAIN_ARCHITECTURE')
        if architecture == 'flux':
            return cls(architecture='flux', checkpoint=weight('ZONE_TRAIN_CHECKPOINT'))
        if architecture == 'qwen_edit':
            return cls(
                architecture='qwen_edit',
                unet=weight('ZONE_TRAIN_UNET'),
                clip=weight('ZONE_TRAIN_CLIP'),
                vae=weight('ZONE_TRAIN_VAE'),
            )
        raise SystemExit('ZONE_TRAIN_ARCHITECTURE must be flux or qwen_edit')


@dataclass(frozen=True)
class Run:
    folder: str
    artifact: str

    @classmethod
    def create(cls, kind: str = 'train') -> 'Run':
        return cls(folder=f'zone-{kind}-{uuid.uuid4()}', artifact=f'zone-lora-{uuid.uuid4()}')

    @classmethod
    def from_environment(cls) -> 'Run':
        folder = env('ZONE_TRAIN_FOLDER')
        artifact = env('ZONE_TRAIN_ARTIFACT')
        if not folder and not artifact:
            return cls.create()
        if not folder or not artifact:
            raise SystemExit('ZONE_TRAIN_FOLDER and ZONE_TRAIN_ARTIFACT must be set together')
        run = cls(folder=folder, artifact=artifact)
        run.validate()
        return run

    def validate(self) -> None:
        if not RUN_PATTERN.fullmatch(self.folder) or not ARTIFACT_PATTERN.fullmatch(self.artifact):
            raise SystemExit('training run identity must use generated UUID names')


def load_config() -> dict:
    return json.loads(CONFIG_PATH.read_text())


def train_steps(image_count: int, config: dict) -> int:
    override = env('ZONE_TRAIN_STEPS')
    if override:
        return int(override)
    budget = max(image_count, 1) * int(config['passes_per_image'])
    return min(int(config['max_steps']), max(int(config['min_steps']), budget))


def env(name: str, default: str = '') -> str:
    return os.environ.get(name, default)


def weight(name: str) -> str:
    value = env(name)
    if not value or len(value) > 256 or '/' in value or '\\' in value or '..' in value:
        raise SystemExit(f'{name} must name one model file')
    return value


def request(url: str, payload: dict | None = None, timeout: int = 60):
    headers = {}
    data = None
    if payload is not None:
        data = json.dumps(payload).encode()
        headers['Content-Type'] = 'application/json'
    token = env('COMFYUI_API_TOKEN')
    if token:
        headers['X-Zone-ComfyUI-Token'] = token
    method = 'POST' if payload is not None else 'GET'
    return urllib.request.urlopen(
        urllib.request.Request(url, data=data, headers=headers, method=method),
        timeout=timeout,
    )


def post_json(url: str, payload: dict, timeout: int = 60) -> dict:
    with request(url, payload, timeout) as response:
        parsed = json.loads(response.read().decode())
    if not isinstance(parsed, dict):
        raise SystemExit('ComfyUI returned a non-object JSON response')
    return parsed


def get_json(url: str, timeout: int = 60) -> dict:
    with request(url, timeout=timeout) as response:
        parsed = json.loads(response.read().decode())
    if not isinstance(parsed, dict):
        raise SystemExit('ComfyUI returned a non-object JSON response')
    return parsed


def prompt_id(queued: dict) -> str:
    if set(queued) != {'prompt_id', 'number', 'node_errors'}:
        raise SystemExit('ComfyUI returned an unexpected prompt response')
    number = queued.get('number')
    if (
        queued.get('node_errors')
        or not isinstance(number, (int, float))
        or isinstance(number, bool)
        or number < 0
    ):
        raise SystemExit(json.dumps(queued)[:4000])
    identifier = queued.get('prompt_id')
    try:
        parsed = uuid.UUID(identifier)
    except (AttributeError, TypeError, ValueError) as error:
        raise SystemExit('ComfyUI returned an invalid prompt UUID') from error
    if parsed.version != 4 or str(parsed) != identifier:
        raise SystemExit('ComfyUI returned an invalid prompt UUID')
    return identifier


def stage_dataset(source: Path, destination: Path, model: TrainingModel) -> tuple[int, str]:
    if destination.exists() or destination.is_symlink():
        raise SystemExit(f'training namespace already exists: {destination}')
    destination.mkdir(parents=True)
    pairs = []
    try:
        targets = images(source / 'targets')
        references = images(source / 'control_1') if model.architecture == 'qwen_edit' else []
        if references and len(references) != len(targets):
            raise SystemExit('Qwen edit training needs one reference for every target')
        (destination / 'targets').mkdir()
        if model.architecture == 'qwen_edit':
            (destination / 'control_1').mkdir()
        for index, target in enumerate(targets):
            name = f'{index:04}.png'
            if target.name != name:
                raise SystemExit('training image names must be contiguous indices')
            instruction_path = target.with_suffix('.txt')
            if instruction_path.is_symlink() or not instruction_path.is_file():
                raise SystemExit(f'every target needs an instruction: {instruction_path}')
            instruction = instruction_path.read_text().strip()
            if not instruction:
                raise SystemExit(f'every target needs an instruction: {instruction_path}')
            copy_regular(target, destination / 'targets' / name)
            reference = None
            if model.architecture == 'qwen_edit':
                if index >= len(references) or references[index].name != name:
                    raise SystemExit('Qwen edit references must use the target index')
                copy_regular(references[index], destination / 'control_1' / name)
                reference = f'control_1/{name}'
            pairs.append(
                {
                    'index': index,
                    'target': f'targets/{name}',
                    'reference': reference,
                    'instruction': instruction,
                }
            )
    except BaseException:
        shutil.rmtree(destination, ignore_errors=True)
        raise
    manifest = json.dumps(
        {'schema_version': 1, 'architecture': model.architecture, 'pairs': pairs},
        separators=(',', ':'),
    )
    return len(pairs), manifest


def images(directory: Path) -> list[Path]:
    if directory.is_symlink() or not directory.is_dir():
        raise SystemExit(f'training image directory is missing: {directory}')
    found = sorted(directory.glob('*.png'))
    if not found:
        raise SystemExit(f'no pngs in {directory}')
    return found


def copy_regular(source: Path, destination: Path) -> None:
    if source.is_symlink() or not source.is_file():
        raise SystemExit(f'training input is not a regular file: {source}')
    with source.open('rb') as reader, destination.open('xb') as writer:
        shutil.copyfileobj(reader, writer)


def wait_prompt(base: str, identifier: str, timeout: int) -> dict:
    try:
        parsed = uuid.UUID(identifier)
    except (AttributeError, TypeError, ValueError) as error:
        raise SystemExit('invalid prompt UUID') from error
    if parsed.version != 4 or str(parsed) != identifier:
        raise SystemExit('invalid prompt UUID')
    expected = identifier
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        history = get_json(f'{base}/history/{expected}')
        if history and set(history) != {expected}:
            raise SystemExit('ComfyUI history returned an unexpected prompt')
        entry = history.get(expected)
        if entry:
            status = entry.get('status') or {}
            state = (status.get('status_str') or '').lower()
            if state == 'error':
                raise SystemExit(json.dumps(status, indent=2)[:4000])
            if status.get('completed') is True or state == 'success':
                return entry
        time.sleep(2)
    raise SystemExit(f'train timed out after {timeout}s')


def trainer(
    model: list,
    latents: list,
    positive: list,
    artifact: str,
    config: dict,
    steps: int,
) -> dict:
    return {
        'class_type': 'ZoneTrainLoRA',
        'inputs': {
            'model': model,
            'latents': latents,
            'positive': positive,
            'steps': steps,
            'learning_rate': float(config['learning_rate']),
            'rank': int(config['rank']),
            'seed': int(config['seed']),
            'training_dtype': config['training_dtype'],
            'lora_dtype': config['lora_dtype'],
            'gradient_checkpointing': bool(config['gradient_checkpointing']),
            'checkpoint_depth': int(config['checkpoint_depth']),
            'bypass_mode': bool(config['bypass_mode']),
            'save_name': artifact,
        },
    }


def train_graph(
    model: TrainingModel,
    folder: str,
    manifest: str,
    artifact: str,
    config: dict,
    steps: int,
) -> dict:
    dataset = {
        'class_type': 'ZoneLoadTrainDataset',
        'inputs': {
            'folder': folder,
            'manifest_json': manifest,
            'resolution': int(config['resolution']),
        },
    }
    if model.architecture == 'flux':
        return {
            '1': {
                'class_type': 'CheckpointLoaderSimple',
                'inputs': {'ckpt_name': model.checkpoint},
            },
            '2': dataset,
            '3': {'class_type': 'VAEEncode', 'inputs': {'pixels': ['2', 0], 'vae': ['1', 2]}},
            '4': {'class_type': 'CLIPTextEncode', 'inputs': {'text': ['2', 2], 'clip': ['1', 1]}},
            '5': trainer(['1', 0], ['3', 0], ['4', 0], artifact, config, steps),
        }
    if model.architecture == 'qwen_edit':
        return {
            '1': {
                'class_type': 'UNETLoader',
                'inputs': {'unet_name': model.unet, 'weight_dtype': 'default'},
            },
            '2': {
                'class_type': 'CLIPLoader',
                'inputs': {'clip_name': model.clip, 'type': 'qwen_image'},
            },
            '3': {'class_type': 'VAELoader', 'inputs': {'vae_name': model.vae}},
            '4': dataset,
            '5': {'class_type': 'VAEEncode', 'inputs': {'pixels': ['4', 0], 'vae': ['3', 0]}},
            '6': {
                'class_type': 'TextEncodeQwenImageEditPlus',
                'inputs': {
                    'clip': ['2', 0],
                    'prompt': ['4', 2],
                    'vae': ['3', 0],
                    'image1': ['4', 1],
                },
            },
            '7': trainer(['1', 0], ['5', 0], ['6', 0], artifact, config, steps),
        }
    raise SystemExit('unsupported training architecture')


def comfy_input_dir() -> Path:
    override = env('ZONE_COMFY_INPUT')
    if override:
        return Path(override)
    models_dir = env('COMFYUI_MODELS_DIR')
    if models_dir:
        return Path(models_dir).parent / 'input'
    raise SystemExit('ZONE_COMFY_INPUT is required so images land in ComfyUI/input')


def download(base: str, run: Run, output: Path) -> None:
    filename = f'{run.artifact}.safetensors'
    query = urllib.parse.urlencode({'filename': filename, 'subfolder': 'loras', 'type': 'output'})
    with request(f'{base}/view?{query}', timeout=120) as response:
        if response.headers.get('Content-Disposition') != f'filename="{filename}"':
            raise SystemExit('ComfyUI view response named a different artifact')
        if response.headers.get_content_type() != 'application/octet-stream':
            raise SystemExit('ComfyUI view response is not safetensors data')
        weights = response.read()
    if len(weights) < MIN_WEIGHT_BYTES:
        raise SystemExit('ComfyUI returned a LoRA that is too small to be trained weights')
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.is_symlink():
        raise SystemExit('training output cannot be a symlink')
    temporary = output.parent / f'.{output.name}.{uuid.uuid4()}.tmp'
    try:
        with temporary.open('xb') as writer:
            writer.write(weights)
            writer.flush()
            os.fsync(writer.fileno())
        temporary.replace(output)
    finally:
        temporary.unlink(missing_ok=True)


def cleanup(base: str, run: Run, staged: Path) -> None:
    if staged.exists() and not staged.is_symlink():
        shutil.rmtree(staged, ignore_errors=True)
    try:
        identifier = prompt_id(
            post_json(
                f'{base}/prompt',
                {
                    'prompt': {
                        '1': {
                            'class_type': 'ZoneCleanupTrainingRun',
                            'inputs': {'folder': run.folder, 'artifact': run.artifact},
                        }
                    }
                },
            )
        )
        wait_prompt(base, identifier, 30)
    except (SystemExit, urllib.error.URLError):
        pass


def main() -> None:
    config = load_config()
    model = TrainingModel.from_environment()
    run = Run.from_environment()
    run.validate()
    train_dir_value = env('ZONE_TRAIN_DIR')
    output_value = env('ZONE_TRAIN_OUTPUT')
    if not train_dir_value or not output_value:
        raise SystemExit('ZONE_TRAIN_DIR and ZONE_TRAIN_OUTPUT are required')
    train_dir = Path(train_dir_value)
    output = Path(output_value)
    base = env('COMFYUI_BASE_URL', env('ZONE_COMFY_URL', 'http://127.0.0.1:8188')).rstrip('/')
    staged = comfy_input_dir() / run.folder
    succeeded = False
    try:
        count, manifest = stage_dataset(train_dir, staged, model)
        steps = train_steps(count, config)
        identifier = prompt_id(
            post_json(
                f'{base}/prompt',
                {'prompt': train_graph(model, run.folder, manifest, run.artifact, config, steps)},
            )
        )
        print(f'train queued {identifier} steps={steps} images={count}', flush=True)
        wait_prompt(base, identifier, int(env('ZONE_TRAIN_TIMEOUT', '3600')))
        download(base, run, output)
        succeeded = True
        print(f'wrote {output} {output.stat().st_size} bytes', flush=True)
    finally:
        if not succeeded or env('ZONE_TRAIN_DEFER_CLEANUP') != '1':
            cleanup(base, run, staged)


if __name__ == '__main__':
    try:
        main()
    except urllib.error.HTTPError as error:
        raise SystemExit(f'ComfyUI HTTP {error.code}: {error.read()[:2000]!r}') from error
