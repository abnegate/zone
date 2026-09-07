#!/usr/bin/env python3
"""Measure an adapter against its own base, so a run can be judged without rendering.

ZONE_PROBE_MODE=loss reports the training loss at fixed noise levels for the base
and for each named adapter. ZONE_PROBE_MODE=gradient descends on one unchanging
batch, where a correct gradient has to lower the loss.
"""

from __future__ import annotations

import json
import sys
import urllib.error
from pathlib import Path

from train_lora import comfy_input_dir, env, post_json, stage_dataset, wait_prompt

CHECKPOINT = 'flux1-dev-fp8.safetensors'


def base_url() -> str:
    return env('COMFYUI_BASE_URL', env('ZONE_COMFY_URL', 'http://127.0.0.1:8188')).rstrip('/')


def dataset_nodes(folder: str, captions: dict[str, str], resolution: int) -> dict:
    return {
        '1': {
            'class_type': 'CheckpointLoaderSimple',
            'inputs': {'ckpt_name': env('ZONE_PROBE_CHECKPOINT', CHECKPOINT)},
        },
        '2': {
            'class_type': 'ZoneLoadTrainFolder',
            'inputs': {
                'folder': folder,
                'captions_json': json.dumps(captions),
                'resolution': resolution,
            },
        },
        '3': {
            'class_type': 'MakeTrainingDataset',
            'inputs': {'images': ['2', 0], 'texts': ['2', 1], 'vae': ['1', 2], 'clip': ['1', 1]},
        },
    }


def loss_graph(folder: str, captions: dict[str, str], resolution: int, lora: str) -> dict:
    nodes = dataset_nodes(folder, captions, resolution)
    model = ['1', 0]
    if lora:
        nodes['5'] = {
            'class_type': 'LoraLoaderModelOnly',
            'inputs': {
                'model': model,
                'lora_name': lora,
                'strength_model': float(env('ZONE_PROBE_STRENGTH', '1.0')),
            },
        }
        model = ['5', 0]
    nodes['4'] = {
        'class_type': 'ZoneProbeLoss',
        'inputs': {
            'model': model,
            'latents': ['3', 0],
            'positive': ['3', 1],
            'percents': env('ZONE_PROBE_PERCENTS', '0.2,0.6,0.9'),
            'seed': int(env('ZONE_PROBE_SEED', '1234')),
        },
    }
    nodes['6'] = {'class_type': 'PreviewAny', 'inputs': {'source': ['4', 0]}}
    return nodes


def gradient_graph(folder: str, captions: dict[str, str], resolution: int) -> dict:
    nodes = dataset_nodes(folder, captions, resolution)
    nodes['4'] = {
        'class_type': 'ZoneProbeGradient',
        'inputs': {
            'model': ['1', 0],
            'latents': ['3', 0],
            'positive': ['3', 1],
            'learning_rate': float(env('ZONE_PROBE_LEARNING_RATE', '0.0001')),
            'iterations': int(env('ZONE_PROBE_ITERATIONS', '40')),
            'percent': float(env('ZONE_PROBE_PERCENT', '0.5')),
            'rank': int(env('ZONE_PROBE_RANK', '8')),
            'seed': int(env('ZONE_PROBE_SEED', '1234')),
            'gradient_checkpointing': env('ZONE_PROBE_CHECKPOINTING', '1') != '0',
        },
    }
    nodes['6'] = {'class_type': 'PreviewAny', 'inputs': {'source': ['4', 0]}}
    return nodes


def report(base: str, nodes: dict, timeout: int) -> dict:
    queued = post_json(f'{base}/prompt', {'prompt': nodes})
    if queued.get('error'):
        raise SystemExit(json.dumps(queued)[:4000])
    entry = wait_prompt(base, queued['prompt_id'], timeout)
    for output in (entry.get('outputs') or {}).values():
        for value in output.values():
            text = value[0] if isinstance(value, list) and value else value
            if isinstance(text, str) and text.startswith('{'):
                return json.loads(text)
    raise SystemExit(f'no probe report in {json.dumps(entry.get("outputs"))[:1000]}')


def dataset() -> tuple[str, dict[str, str]]:
    """A probe reads the dataset the same way training does, from ComfyUI's input."""
    folder = env('ZONE_PROBE_FOLDER')
    if folder:
        source = Path(env('ZONE_COMFY_INPUT', '')) / folder
        captions = {
            png.name: png.with_suffix('.txt').read_text().strip()
            for png in sorted(source.glob('*.png'))
            if png.with_suffix('.txt').is_file()
        }
        return folder, captions
    train_dir = Path(env('ZONE_TRAIN_DIR'))
    name = env('ZONE_PROBE_NAME', train_dir.name)
    folder = f'zone-probe-{name}'
    _, captions = stage_dataset(train_dir / 'targets', comfy_input_dir() / folder)
    return folder, captions


def main() -> None:
    base = base_url()
    resolution = int(env('ZONE_PROBE_RESOLUTION', '512'))
    timeout = int(env('ZONE_PROBE_TIMEOUT', '7200'))
    folder, captions = dataset()
    if env('ZONE_PROBE_MODE', 'loss') == 'gradient':
        result = report(base, gradient_graph(folder, captions, resolution), timeout)
        losses = result['losses']
        print(json.dumps(result))
        print(f'first={losses[0]:.5f} last={losses[-1]:.5f} '
              f'change={(losses[-1] - losses[0]) / losses[0] * 100:+.2f}%')
        return
    adapters = sys.argv[1:]
    baseline = report(base, loss_graph(folder, captions, resolution, ''), timeout)
    print(f'{"base":44s} mean={baseline["mean"]:.5f}  {baseline["by_percent"]}')
    for adapter in adapters:
        measured = report(base, loss_graph(folder, captions, resolution, adapter), timeout)
        change = (measured['mean'] - baseline['mean']) / baseline['mean'] * 100
        print(f'{adapter:44s} mean={measured["mean"]:.5f}  {change:+.2f}%  {measured["by_percent"]}')


if __name__ == '__main__':
    try:
        main()
    except urllib.error.HTTPError as error:
        raise SystemExit(f'ComfyUI HTTP {error.code}: {error.read()[:2000]!r}') from error
