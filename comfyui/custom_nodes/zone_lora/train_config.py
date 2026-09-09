from __future__ import annotations

import json
from pathlib import Path
from typing import Any


CONFIG_PATH = Path(__file__).with_name('train_config.json')
TRANSFORMER_PARTS = ('double_blocks', 'single_blocks', 'transformer_blocks')
OUTPUT_MARKERS = ('final_layer', 'img_out', 'txt_out')
MODULATION_PARTS = ('img_mod', 'txt_mod', 'modulation')


def load_config(path: Path | None = None) -> dict[str, Any]:
    source = path or CONFIG_PATH
    return json.loads(source.read_text())


def lora_alpha(rank: int, config: dict[str, Any] | None = None) -> float:
    settings = config or load_config()
    if settings.get('alpha_equals_rank', True):
        return float(rank)
    return 1.0


def checkpoint_interval(steps: int, settings: dict[str, Any] | None = None) -> int:
    """Keep the count of intermediates bounded rather than the gap between them.

    A fixed gap costs a fixed amount per step, so raising the step ceiling raises
    the disk bill with it — at rank 32 an intermediate is 220 MB, and a long run
    would write hundreds of them.
    """
    values = settings if settings is not None else load_config()
    every = int(values.get('checkpoint_every', 0))
    if not every:
        return 0
    most = int(values.get('checkpoints_per_run', 8))
    if most < 1:
        return every
    return max(every, -(-steps // most))


def is_transformer_block(name: str) -> bool:
    parts = name.split('.')
    return any(part in TRANSFORMER_PARTS for part in parts)


def is_output_module(name: str) -> bool:
    return any(marker in name for marker in OUTPUT_MARKERS)


def is_modulation(name: str) -> bool:
    return any(part in MODULATION_PARTS for part in name.split('.'))


def trains(name: str, config: dict[str, Any] | None = None) -> bool:
    """Whether a module gets a LoRA adapter.

    Modulation layers emit the shift, scale, and gate every block applies to its
    whole activation, so perturbing them moves the conditioning path rather than
    the subject and destabilises training long before it teaches an identity.
    """
    if not is_transformer_block(name):
        return False
    settings = config if config is not None else load_config()
    if not settings.get('train_modulation', False) and is_modulation(name):
        return False
    return True
