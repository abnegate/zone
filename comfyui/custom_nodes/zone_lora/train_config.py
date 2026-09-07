from __future__ import annotations

import json
from pathlib import Path
from typing import Any


CONFIG_PATH = Path(__file__).with_name('train_config.json')
TRANSFORMER_PARTS = ('double_blocks', 'single_blocks', 'transformer_blocks')
OUTPUT_MARKERS = ('final_layer', 'img_out', 'txt_out')


def load_config(path: Path | None = None) -> dict[str, Any]:
    source = path or CONFIG_PATH
    return json.loads(source.read_text())


def train_steps(image_count: int, config: dict[str, Any] | None = None) -> int:
    settings = config or load_config()
    per_image = int(settings['steps_per_image'])
    minimum = int(settings['min_steps'])
    maximum = int(settings['max_steps'])
    return min(maximum, max(minimum, max(image_count, 1) * per_image))


def lora_alpha(rank: int, config: dict[str, Any] | None = None) -> float:
    settings = config or load_config()
    if settings.get('alpha_equals_rank', True):
        return float(rank)
    return 1.0


def is_transformer_block(name: str) -> bool:
    parts = name.split('.')
    return any(part in TRANSFORMER_PARTS for part in parts)


def is_output_module(name: str) -> bool:
    return any(marker in name for marker in OUTPUT_MARKERS)
