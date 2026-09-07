"""Runs before ComfyUI imports torch. Caps MPS watermarks without editing Comfy core."""

from __future__ import annotations

import os
import sys
from collections.abc import MutableMapping


def apply_mps_watermarks(env: MutableMapping[str, str] | None = None) -> None:
    values = env if env is not None else os.environ
    if sys.platform != 'darwin':
        return
    values.setdefault('PYTORCH_MPS_HIGH_WATERMARK_RATIO', '1.0')
    high = float(values.get('PYTORCH_MPS_HIGH_WATERMARK_RATIO', '1.0'))
    if high <= 0:
        return
    low = values.get('PYTORCH_MPS_LOW_WATERMARK_RATIO')
    if low is None or float(low) > high:
        values['PYTORCH_MPS_LOW_WATERMARK_RATIO'] = '0.8' if high >= 0.8 else '0.0'


apply_mps_watermarks()
