#!/usr/bin/env python3
"""Run the repository-owned FLUX or Qwen Image Edit LoRA training graph."""

from __future__ import annotations

import json
import math
import os
import re
import shutil
import stat
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
CANCEL_TIMEOUT = 30
POLL_INTERVAL = 2
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


class PromptFailure(SystemExit):
    def __init__(self, message: str, cleanup: bool) -> None:
        super().__init__(message)
        self.cleanup = cleanup


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
        or not math.isfinite(number)
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


def queue_prompt(base: str, graph: dict) -> str:
    try:
        queued = post_json(f'{base}/prompt', {'prompt': graph})
    except (OSError, SystemExit, urllib.error.URLError, ValueError) as error:
        raise PromptFailure(str(error), False) from error
    try:
        return prompt_id(queued)
    except SystemExit as error:
        candidate = queued.get('prompt_id')
        if valid_prompt_id(candidate):
            cleanup = cancel_prompt(base, candidate, False)
            raise PromptFailure(str(error), cleanup) from error
        raise PromptFailure(str(error), False) from error


def stage_dataset(source: Path, destination: Path, model: TrainingModel) -> tuple[int, str]:
    comfy_root = real_directory(destination.parent.parent, 'ComfyUI root')
    destination_root = real_child(
        comfy_root, destination.parent, 'ComfyUI input directory'
    )
    if not RUN_PATTERN.fullmatch(destination.name):
        raise SystemExit('training namespace must use a generated UUID name')
    if destination.exists() or destination.is_symlink():
        raise SystemExit(f'training namespace already exists: {destination}')
    destination.mkdir()
    destination = real_child(destination_root, destination, 'training namespace')
    pairs = []
    try:
        targets = images(source / 'targets')
        references = images(source / 'control_1') if model.architecture == 'qwen_edit' else []
        if references and len(references) != len(targets):
            raise SystemExit('Qwen edit training needs one reference for every target')
        (destination / 'targets').mkdir()
        target_destination = real_child(
            destination, destination / 'targets', 'training target directory'
        )
        if model.architecture == 'qwen_edit':
            (destination / 'control_1').mkdir()
            control_destination = real_child(
                destination, destination / 'control_1', 'training reference directory'
            )
        else:
            control_destination = None
        for index, target in enumerate(targets):
            name = f'{index:04}.png'
            if target.name != name:
                raise SystemExit('training image names must be contiguous indices')
            instruction_path = target.with_suffix('.txt')
            instruction = read_regular_text(instruction_path).strip()
            if not instruction:
                raise SystemExit(f'every target needs an instruction: {instruction_path}')
            copy_regular(target, target_destination / name)
            reference = None
            if model.architecture == 'qwen_edit':
                if index >= len(references) or references[index].name != name:
                    raise SystemExit('Qwen edit references must use the target index')
                if control_destination is None:
                    raise SystemExit('Qwen edit reference directory is missing')
                copy_regular(references[index], control_destination / name)
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
        remove_namespace(destination_root, destination.name)
        raise
    manifest = json.dumps(
        {'schema_version': 1, 'architecture': model.architecture, 'pairs': pairs},
        separators=(',', ':'),
    )
    return len(pairs), manifest


def images(directory: Path) -> list[Path]:
    directory = real_directory(directory, 'training image directory')
    found = sorted(path for path in directory.iterdir() if path.suffix == '.png')
    if not found:
        raise SystemExit(f'no pngs in {directory}')
    return found


def copy_regular(source: Path, destination: Path) -> None:
    try:
        metadata = source.lstat()
    except OSError as error:
        raise SystemExit(f'training input is not a regular file: {source}') from error
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
        raise SystemExit(f'training input is not a regular file: {source}')
    nofollow = getattr(os, 'O_NOFOLLOW', 0)
    source_descriptor = os.open(source, os.O_RDONLY | nofollow)
    try:
        destination_descriptor = os.open(
            destination,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | nofollow,
            0o600,
        )
    except BaseException:
        os.close(source_descriptor)
        raise
    with os.fdopen(source_descriptor, 'rb') as reader, os.fdopen(
        destination_descriptor, 'wb'
    ) as writer:
        if not stat.S_ISREG(os.fstat(reader.fileno()).st_mode):
            raise SystemExit(f'training input is not a regular file: {source}')
        shutil.copyfileobj(reader, writer)
        writer.flush()
        os.fsync(writer.fileno())


def read_regular_text(source: Path) -> str:
    try:
        metadata = source.lstat()
    except OSError as error:
        raise SystemExit(f'every target needs an instruction: {source}') from error
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
        raise SystemExit(f'every target needs an instruction: {source}')
    descriptor = os.open(source, os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0))
    with os.fdopen(descriptor, 'r') as reader:
        if not stat.S_ISREG(os.fstat(reader.fileno()).st_mode):
            raise SystemExit(f'every target needs an instruction: {source}')
        return reader.read()


def valid_prompt_id(identifier: object) -> bool:
    try:
        parsed = uuid.UUID(identifier)
    except (AttributeError, TypeError, ValueError):
        return False
    return parsed.version == 4 and str(parsed) == identifier


def terminal(entry: object) -> bool:
    if not isinstance(entry, dict) or not isinstance(entry.get('status'), dict):
        return False
    status = entry['status']
    state = status.get('status_str')
    return status.get('completed') is True or (
        isinstance(state, str) and state.lower() in {'error', 'success'}
    )


def cancel_prompt(
    base: str,
    identifier: str,
    terminal_observed: bool,
    timeout: int = CANCEL_TIMEOUT,
) -> bool:
    if not valid_prompt_id(identifier):
        return False
    try:
        with request(
            f'{base}/api/jobs/{identifier}/cancel',
            {},
            timeout=min(timeout, 5) if timeout > 0 else 1,
        ):
            pass
    except (OSError, SystemExit, urllib.error.URLError):
        pass
    if terminal_observed:
        return True
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            history = get_json(
                f'{base}/history/{identifier}',
                timeout=min(max(int(deadline - time.monotonic()), 1), 5),
            )
            if set(history) == {identifier} and terminal(history[identifier]):
                return True
        except (OSError, SystemExit, urllib.error.URLError, ValueError):
            pass
        time.sleep(min(POLL_INTERVAL, max(deadline - time.monotonic(), 0)))
    return False


def wait_prompt(base: str, identifier: str, timeout: int) -> dict:
    if not valid_prompt_id(identifier):
        raise SystemExit('invalid prompt UUID')
    expected = identifier
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            history = get_json(f'{base}/history/{expected}')
        except BaseException as error:
            cleanup = cancel_prompt(base, expected, False)
            raise PromptFailure(str(error), cleanup) from error
        if history and set(history) != {expected}:
            cleanup = cancel_prompt(base, expected, False)
            raise PromptFailure('ComfyUI history returned an unexpected prompt', cleanup)
        entry = history.get(expected)
        if entry:
            if not isinstance(entry, dict) or not isinstance(entry.get('status'), dict):
                cleanup = cancel_prompt(base, expected, False)
                raise PromptFailure('ComfyUI history status is malformed', cleanup)
            status = entry.get('status') or {}
            raw_state = status.get('status_str')
            completed = status.get('completed')
            if (raw_state is not None and not isinstance(raw_state, str)) or (
                completed is not None and not isinstance(completed, bool)
            ):
                cleanup = cancel_prompt(base, expected, False)
                raise PromptFailure('ComfyUI history status is malformed', cleanup)
            state = (raw_state or '').lower()
            if state == 'error':
                cancel_prompt(base, expected, True)
                raise PromptFailure(json.dumps(status, indent=2)[:4000], True)
            if completed is True or state == 'success':
                return entry
        time.sleep(POLL_INTERVAL)
    cleanup = cancel_prompt(base, expected, False)
    raise PromptFailure(f'train timed out after {timeout}s', cleanup)


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
        directory = Path(override)
    else:
        models_dir = env('COMFYUI_MODELS_DIR')
        if not models_dir:
            raise SystemExit('ZONE_COMFY_INPUT is required so images land in ComfyUI/input')
        directory = Path(models_dir).parent / 'input'
    root = real_directory(directory.parent, 'ComfyUI root')
    return real_child(root, directory, 'ComfyUI input directory')


def real_directory(path: Path, label: str) -> Path:
    try:
        metadata = path.lstat()
    except OSError as error:
        raise SystemExit(f'{label} is missing: {path}') from error
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
        raise SystemExit(f'{label} must be a real directory: {path}')
    return path.resolve(strict=True)


def real_child(root: Path, child: Path, label: str) -> Path:
    root = real_directory(root, label)
    child = real_directory(child, label)
    if child.parent != root:
        raise SystemExit(f'{label} escapes its root')
    return child


def remove_namespace(root: Path, name: str) -> bool:
    if not RUN_PATTERN.fullmatch(name):
        return False
    try:
        root = real_directory(root, 'cleanup root')
        descriptor = os.open(
            root,
            os.O_RDONLY | getattr(os, 'O_DIRECTORY', 0) | getattr(os, 'O_NOFOLLOW', 0),
        )
    except (OSError, SystemExit):
        return False
    try:
        metadata = os.stat(name, dir_fd=descriptor, follow_symlinks=False)
        if not stat.S_ISDIR(metadata.st_mode):
            return False
        shutil.rmtree(name, dir_fd=descriptor)
        return True
    except OSError:
        return False
    finally:
        os.close(descriptor)


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


def cleanup(base: str, run: Run, staged: Path) -> bool:
    run.validate()
    remote_safe = True
    try:
        identifier = queue_prompt(
            base,
            {
                '1': {
                    'class_type': 'ZoneCleanupTrainingRun',
                    'inputs': {'folder': run.folder, 'artifact': run.artifact},
                }
            },
        )
        wait_prompt(base, identifier, 30)
    except PromptFailure as error:
        remote_safe = error.cleanup
    except (SystemExit, urllib.error.URLError):
        remote_safe = False
    if remote_safe:
        remove_namespace(staged.parent, run.folder)
    return remote_safe


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
    cleanup_safe = True
    try:
        try:
            count, manifest = stage_dataset(train_dir, staged, model)
            steps = train_steps(count, config)
            identifier = queue_prompt(
                base,
                train_graph(model, run.folder, manifest, run.artifact, config, steps),
            )
            print(f'train queued {identifier} steps={steps} images={count}', flush=True)
            wait_prompt(base, identifier, int(env('ZONE_TRAIN_TIMEOUT', '3600')))
        except PromptFailure as error:
            cleanup_safe = error.cleanup
            raise
        try:
            download(base, run, output)
        except BaseException:
            cancel_prompt(base, identifier, True)
            raise
        succeeded = True
        print(f'wrote {output} {output.stat().st_size} bytes', flush=True)
    finally:
        if cleanup_safe and (not succeeded or env('ZONE_TRAIN_DEFER_CLEANUP') != '1'):
            cleanup(base, run, staged)


if __name__ == '__main__':
    try:
        main()
    except urllib.error.HTTPError as error:
        raise SystemExit(f'ComfyUI HTTP {error.code}: {error.read()[:2000]!r}') from error
