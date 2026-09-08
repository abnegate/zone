from __future__ import annotations

import json
import re
from pathlib import Path

import folder_paths
import numpy as np
import torch
from PIL import Image
from comfy_api.latest import io

from .train_node import square


RUN = re.compile(
    r'zone-(?:train|probe)-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}'
    r'-[89ab][0-9a-f]{3}-[0-9a-f]{12}'
)
PAIR_FIELDS = frozenset({'index', 'target', 'reference', 'instruction'})


def _image(path: Path, resolution: int) -> torch.Tensor:
    image = square(Image.open(path).convert('RGB'), resolution)
    array = np.asarray(image).astype(np.float32) / 255.0
    return torch.from_numpy(array)[None,]


def _file(root: Path, relative: str, expected: str) -> Path:
    if relative != expected:
        raise ValueError(f'training manifest path must be {expected}')
    path = root / relative
    if path.is_symlink():
        raise ValueError(f'training manifest refuses symlink {relative}')
    resolved = path.resolve(strict=True)
    if not resolved.is_relative_to(root):
        raise ValueError(f'training manifest path escapes its run: {relative}')
    if not resolved.is_file():
        raise ValueError(f'training manifest path is not a file: {relative}')
    return resolved


class ZoneLoadTrainDataset(io.ComfyNode):
    """Load an exact ordered manifest so edit references cannot drift from targets."""

    @classmethod
    def define_schema(cls):
        return io.Schema(
            node_id='ZoneLoadTrainDataset',
            display_name='Zone Load Train Dataset',
            category='zone/training',
            inputs=[
                io.String.Input('folder', default=''),
                io.String.Input('manifest_json', default=''),
                io.Int.Input('resolution', default=512, min=64, max=2048),
            ],
            outputs=[
                io.Image.Output(display_name='targets', is_output_list=True),
                io.Image.Output(display_name='references', is_output_list=True),
                io.String.Output(display_name='instructions', is_output_list=True),
            ],
        )

    @classmethod
    def execute(cls, folder, manifest_json, resolution):
        if not RUN.fullmatch(folder):
            raise ValueError('training folder must be a server-generated Zone run UUID')
        input_root = Path(folder_paths.get_input_directory()).resolve(strict=True)
        root_path = input_root / folder
        if root_path.is_symlink():
            raise ValueError('training folder cannot be a symlink')
        root = root_path.resolve(strict=True)
        if not root.is_relative_to(input_root) or not root.is_dir():
            raise ValueError('training folder escapes the ComfyUI input root')

        manifest = json.loads(manifest_json)
        if set(manifest) != {'schema_version', 'architecture', 'pairs'}:
            raise ValueError('training manifest has unexpected fields')
        if manifest['schema_version'] != 1:
            raise ValueError('unsupported training manifest schema')
        architecture = manifest['architecture']
        if architecture not in ('flux', 'qwen_edit'):
            raise ValueError('unsupported training manifest architecture')
        if not isinstance(manifest['pairs'], list) or not manifest['pairs']:
            raise ValueError('training manifest has no pairs')

        targets = []
        references = []
        instructions = []
        for index, pair in enumerate(manifest['pairs']):
            if not isinstance(pair, dict) or set(pair) != PAIR_FIELDS:
                raise ValueError(f'training pair {index} has unexpected fields')
            if pair['index'] != index:
                raise ValueError('training manifest indices must be contiguous and ordered')
            name = f'{index:04}.png'
            target = _file(root, pair['target'], f'targets/{name}')
            instruction = pair['instruction']
            if not isinstance(instruction, str) or not instruction.strip():
                raise ValueError(f'training pair {index} has no instruction')
            reference = pair['reference']
            if architecture == 'qwen_edit':
                if not isinstance(reference, str):
                    raise ValueError(f'Qwen edit pair {index} has no reference')
                reference_path = _file(root, reference, f'control_1/{name}')
            elif reference is not None:
                raise ValueError(f'FLUX pair {index} unexpectedly has a reference')
            else:
                reference_path = target
            targets.append(_image(target, resolution))
            references.append(_image(reference_path, resolution))
            instructions.append(instruction.strip())
        return io.NodeOutput(targets, references, instructions)
