#!/usr/bin/env python3
"""Measure a FLUX or Qwen Image Edit adapter against its explicit base."""

from __future__ import annotations

import json
import sys
import urllib.error
import uuid
from pathlib import Path

from train_lora import (
    Run,
    TrainingModel,
    comfy_input_dir,
    env,
    load_config,
    post_json,
    prompt_id,
    stage_dataset,
    wait_prompt,
)


def base_url() -> str:
    return env('COMFYUI_BASE_URL', env('ZONE_COMFY_URL', 'http://127.0.0.1:8188')).rstrip('/')


def dataset_nodes(
    model: TrainingModel, folder: str, manifest: str, resolution: int
) -> tuple[dict, list, list, list]:
    dataset = {
        'class_type': 'ZoneLoadTrainDataset',
        'inputs': {'folder': folder, 'manifest_json': manifest, 'resolution': resolution},
    }
    if model.architecture == 'flux':
        return (
            {
                '1': {
                    'class_type': 'CheckpointLoaderSimple',
                    'inputs': {'ckpt_name': model.checkpoint},
                },
                '2': dataset,
                '3': {
                    'class_type': 'VAEEncode',
                    'inputs': {'pixels': ['2', 0], 'vae': ['1', 2]},
                },
                '4': {
                    'class_type': 'CLIPTextEncode',
                    'inputs': {'text': ['2', 2], 'clip': ['1', 1]},
                },
            },
            ['1', 0],
            ['3', 0],
            ['4', 0],
        )
    if model.architecture == 'qwen_edit':
        return (
            {
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
                '5': {
                    'class_type': 'VAEEncode',
                    'inputs': {'pixels': ['4', 0], 'vae': ['3', 0]},
                },
                '6': {
                    'class_type': 'TextEncodeQwenImageEditPlus',
                    'inputs': {
                        'clip': ['2', 0],
                        'prompt': ['4', 2],
                        'vae': ['3', 0],
                        'image1': ['4', 1],
                    },
                },
            },
            ['1', 0],
            ['5', 0],
            ['6', 0],
        )
    raise SystemExit('unsupported training architecture')


def loss_graph(
    model: TrainingModel, folder: str, manifest: str, resolution: int, lora: str
) -> dict:
    nodes, base, latents, positive = dataset_nodes(model, folder, manifest, resolution)
    applied = base
    if lora:
        loader = f'lora-{uuid.uuid4()}'
        nodes[loader] = {
            'class_type': 'LoraLoaderModelOnly',
            'inputs': {
                'model': base,
                'lora_name': lora,
                'strength_model': float(env('ZONE_PROBE_STRENGTH', '1.0')),
            },
        }
        applied = [loader, 0]
    probe = '7' if model.architecture == 'qwen_edit' else '5'
    preview = '8' if model.architecture == 'qwen_edit' else '6'
    nodes[probe] = {
        'class_type': 'ZoneProbeLoss',
        'inputs': {
            'model': applied,
            'latents': latents,
            'positive': positive,
            'percents': env('ZONE_PROBE_PERCENTS', '0.2,0.6,0.9'),
            'seed': int(env('ZONE_PROBE_SEED', '1234')),
        },
    }
    nodes[preview] = {'class_type': 'PreviewAny', 'inputs': {'source': [probe, 0]}}
    return nodes


def gradient_graph(model: TrainingModel, folder: str, manifest: str, resolution: int) -> dict:
    nodes, base, latents, positive = dataset_nodes(model, folder, manifest, resolution)
    probe = '7' if model.architecture == 'qwen_edit' else '5'
    preview = '8' if model.architecture == 'qwen_edit' else '6'
    nodes[probe] = {
        'class_type': 'ZoneProbeGradient',
        'inputs': {
            'model': base,
            'latents': latents,
            'positive': positive,
            'learning_rate': float(env('ZONE_PROBE_LEARNING_RATE', '0.0001')),
            'iterations': int(env('ZONE_PROBE_ITERATIONS', '40')),
            'percent': float(env('ZONE_PROBE_PERCENT', '0.5')),
            'rank': int(env('ZONE_PROBE_RANK') or load_config()['rank']),
            'seed': int(env('ZONE_PROBE_SEED', '1234')),
            'gradient_checkpointing': env('ZONE_PROBE_CHECKPOINTING', '1') != '0',
        },
    }
    nodes[preview] = {'class_type': 'PreviewAny', 'inputs': {'source': [probe, 0]}}
    return nodes


def report(base: str, nodes: dict, timeout: int) -> dict:
    identifier = prompt_id(post_json(f'{base}/prompt', {'prompt': nodes}))
    entry = wait_prompt(base, identifier, timeout)
    for output in (entry.get('outputs') or {}).values():
        for value in output.values():
            text = value[0] if isinstance(value, list) and value else value
            if isinstance(text, str) and text.startswith('{'):
                parsed = json.loads(text)
                if isinstance(parsed, dict):
                    return parsed
    raise SystemExit(f'no probe report in {json.dumps(entry.get("outputs"))[:1000]}')


def dataset(model: TrainingModel) -> tuple[Run, str, bool]:
    folder = env('ZONE_PROBE_FOLDER')
    if folder:
        manifest = env('ZONE_PROBE_MANIFEST')
        if not manifest:
            raise SystemExit('ZONE_PROBE_MANIFEST is required with ZONE_PROBE_FOLDER')
        run = Run(folder=folder, artifact=f'zone-lora-{uuid.uuid4()}')
        run.validate()
        source = comfy_input_dir() / folder
        if source.is_symlink() or not source.is_dir():
            raise SystemExit(f'no probe folder at {source}')
        parsed = json.loads(manifest)
        if parsed.get('architecture') != model.architecture:
            raise SystemExit('probe manifest architecture does not match its model')
        return run, manifest, False
    train_dir = env('ZONE_TRAIN_DIR')
    if not train_dir:
        raise SystemExit('ZONE_TRAIN_DIR is required when no staged probe folder is supplied')
    run = Run.create('probe')
    _, manifest = stage_dataset(Path(train_dir), comfy_input_dir() / run.folder, model)
    return run, manifest, True


def main() -> None:
    model = TrainingModel.from_environment()
    base = base_url()
    resolution = int(env('ZONE_PROBE_RESOLUTION') or load_config()['resolution'])
    timeout = int(env('ZONE_PROBE_TIMEOUT', '7200'))
    run, manifest, staged = dataset(model)
    try:
        if env('ZONE_PROBE_MODE', 'loss') == 'gradient':
            result = report(base, gradient_graph(model, run.folder, manifest, resolution), timeout)
            losses = result['losses']
            print(json.dumps(result))
            print(
                f'first={losses[0]:.5f} last={losses[-1]:.5f} '
                f'change={(losses[-1] - losses[0]) / losses[0] * 100:+.2f}%'
            )
            return
        baseline = report(base, loss_graph(model, run.folder, manifest, resolution, ''), timeout)
        print(f'{"base":44s} mean={baseline["mean"]:.5f}  {baseline["by_percent"]}')
        for adapter in sys.argv[1:]:
            measured = report(
                base,
                loss_graph(model, run.folder, manifest, resolution, adapter),
                timeout,
            )
            change = (measured['mean'] - baseline['mean']) / baseline['mean'] * 100
            print(
                f'{adapter:44s} mean={measured["mean"]:.5f}  '
                f'{change:+.2f}%  {measured["by_percent"]}'
            )
    finally:
        if staged:
            directory = comfy_input_dir() / run.folder
            if directory.is_dir() and not directory.is_symlink():
                import shutil

                shutil.rmtree(directory)


if __name__ == '__main__':
    try:
        main()
    except urllib.error.HTTPError as error:
        raise SystemExit(f'ComfyUI HTTP {error.code}: {error.read()[:2000]!r}') from error
