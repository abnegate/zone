#!/usr/bin/env python3
"""Stage a Zone train set and run ZoneTrainLoRA against a ComfyUI server."""

from __future__ import annotations

import json
import os
import shutil
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

CONFIG_PATH = Path(__file__).with_name('custom_nodes') / 'zone_lora' / 'train_config.json'


def load_config() -> dict:
    return json.loads(CONFIG_PATH.read_text())


def train_steps(image_count: int, config: dict) -> int:
    """ZONE_TRAIN_STEPS is what keeps a diagnostic run short enough to be worth running."""
    override = env('ZONE_TRAIN_STEPS')
    if override:
        return int(override)
    return min(
        int(config['max_steps']),
        max(int(config['min_steps']), max(image_count, 1) * int(config['steps_per_image'])),
    )


def env(name: str, default: str = '') -> str:
    return os.environ.get(name, default)


def post_json(url: str, payload: dict, timeout: int = 60) -> dict:
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode(),
        headers={'Content-Type': 'application/json'},
        method='POST',
    )
    token = env('COMFYUI_API_TOKEN')
    if token:
        request.add_header('X-Zone-ComfyUI-Token', token)
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read().decode())


def get_bytes(url: str, timeout: int = 120) -> bytes:
    request = urllib.request.Request(url)
    token = env('COMFYUI_API_TOKEN')
    if token:
        request.add_header('X-Zone-ComfyUI-Token', token)
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return response.read()


def stage_dataset(source: Path, destination: Path) -> tuple[int, dict[str, str]]:
    if destination.exists():
        shutil.rmtree(destination)
    destination.mkdir(parents=True)
    captions: dict[str, str] = {}
    count = 0
    for png in sorted(source.glob('*.png')):
        shutil.copy2(png, destination / png.name)
        txt = png.with_suffix('.txt')
        if txt.is_file():
            shutil.copy2(txt, destination / txt.name)
            captions[png.name] = txt.read_text().strip()
        count += 1
    if count == 0:
        raise SystemExit(f'no pngs in {source}')
    return count, captions


def wait_prompt(base: str, prompt_id: str, timeout: int) -> dict:
    deadline = time.time() + timeout
    while time.time() < deadline:
        history = json.loads(get_bytes(f'{base}/history/{prompt_id}').decode())
        entry = history.get(prompt_id)
        if entry:
            status = ((entry.get('status') or {}).get('status_str') or '').lower()
            if status == 'error':
                raise SystemExit(json.dumps(entry.get('status'), indent=2)[:4000])
            completed = (entry.get('status') or {}).get('completed')
            if completed or (completed is None and status == 'success'):
                return entry
        time.sleep(2)
    raise SystemExit(f'train timed out after {timeout}s')


def train_graph(checkpoint: str, folder: str, captions: dict[str, str], save_name: str, config: dict, steps: int) -> dict:
    return {
        '1': {
            'class_type': 'CheckpointLoaderSimple',
            'inputs': {'ckpt_name': checkpoint},
        },
        '2': {
            'class_type': 'ZoneLoadTrainFolder',
            'inputs': {
                'folder': folder,
                'captions_json': json.dumps(captions),
                'resolution': int(config['resolution']),
            },
        },
        '3': {
            'class_type': 'MakeTrainingDataset',
            'inputs': {
                'images': ['2', 0],
                'texts': ['2', 1],
                'vae': ['1', 2],
                'clip': ['1', 1],
            },
        },
        '4': {
            'class_type': 'ZoneTrainLoRA',
            'inputs': {
                'model': ['1', 0],
                'latents': ['3', 0],
                'positive': ['3', 1],
                'steps': steps,
                'learning_rate': float(config['learning_rate']),
                'rank': int(config['rank']),
                'seed': int(config['seed']),
                'training_dtype': config['training_dtype'],
                'lora_dtype': config['lora_dtype'],
                'gradient_checkpointing': bool(config['gradient_checkpointing']),
                'checkpoint_depth': int(config['checkpoint_depth']),
                'bypass_mode': bool(config['bypass_mode']),
                'save_name': save_name,
            },
        },
    }


def comfy_input_dir() -> Path:
    """Path('') is '.', so the override has to be tested as a string or staging lands in the cwd."""
    override = env('ZONE_COMFY_INPUT')
    if override:
        return Path(override)
    models_dir = env('COMFYUI_MODELS_DIR')
    if models_dir:
        return Path(models_dir).parent / 'input'
    raise SystemExit('ZONE_COMFY_INPUT is required so images land in ComfyUI/input')


def main() -> None:
    config = load_config()
    train_dir = Path(env('ZONE_TRAIN_DIR'))
    output = Path(env('ZONE_TRAIN_OUTPUT'))
    name = Path(env('ZONE_TRAIN_NAME', output.name)).name.removesuffix('.safetensors')
    base_url = env('COMFYUI_BASE_URL', env('ZONE_COMFY_URL', 'http://127.0.0.1:8188')).rstrip('/')
    checkpoint = env('ZONE_TRAIN_CHECKPOINT', 'flux1-schnell-fp8.safetensors')
    comfy_input = comfy_input_dir()
    folder = f'zone-train-{name}'
    count, captions = stage_dataset(Path(train_dir) / 'targets', comfy_input / folder)
    steps = train_steps(count, config)
    queued = post_json(f'{base_url}/prompt', {'prompt': train_graph(checkpoint, folder, captions, name, config, steps)})
    if queued.get('error'):
        raise SystemExit(json.dumps(queued)[:4000])
    prompt_id = queued['prompt_id']
    print(f'train queued {prompt_id} steps={steps} images={count}', flush=True)
    wait_prompt(base_url, prompt_id, int(env('ZONE_TRAIN_TIMEOUT', '3600')))
    query = urllib.parse.urlencode(
        {'filename': f'{name}.safetensors', 'subfolder': 'loras', 'type': 'output'}
    )
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(get_bytes(f'{base_url}/view?{query}', timeout=120))
    print(f'wrote {output} {output.stat().st_size} bytes', flush=True)


if __name__ == '__main__':
    try:
        main()
    except urllib.error.HTTPError as error:
        raise SystemExit(f'ComfyUI HTTP {error.code}: {error.read()[:2000]!r}') from error
