"""Tiled upscales must not hand the model a permuted view.

`ImageUpscaleWithModel` calls `image.movedim(-1, -3)`, whose strides interleave
the channel dimension ahead of width, and `tiled_scale` then slices that view.
A slice of it keeps the interleaved layout, and the MPS convolution reshapes its
input, which such a tensor cannot satisfy:

    view size is not compatible with input tensor's size and stride

The model itself upscales fine on MPS — a whole image works, and so does a tile
sliced from a contiguous tensor. Only the permuted layout fails, so every ESRGAN
pass over an image larger than one tile failed on Apple Silicon.
"""

from __future__ import annotations

import os
import sys
import unittest
from pathlib import Path

COMFYUI = Path(__file__).parents[1]

# `Path('')` is `PosixPath('.')`, which is a directory, so checking the path
# rather than the value leaves the guard below dead -- the same footgun that
# made `train_lora.py`'s own required-variable check unreachable.
_INSTALL = os.environ.get('COMFYUI_INSTALL_DIR', '').strip()
if not _INSTALL:
    raise RuntimeError('COMFYUI_INSTALL_DIR must point to the pinned ComfyUI checkout')
INSTALL = Path(_INSTALL)
if not INSTALL.is_dir():
    raise RuntimeError(f'COMFYUI_INSTALL_DIR is not a directory: {INSTALL}')
if str(COMFYUI) not in sys.path:
    sys.path.insert(0, str(COMFYUI))
if str(INSTALL) not in sys.path:
    sys.path.insert(0, str(INSTALL))

import torch  # noqa: E402

import comfy.utils  # noqa: E402

from custom_nodes.zone_lora.inference_hooks import install_contiguous_tiled_scale  # noqa: E402


def interleaved(tensor: torch.Tensor) -> bool:
    """Whether a later dimension is stored more densely than an earlier one.

    This is the layout `movedim` leaves and the one the MPS convolution refuses.
    A slice of a contiguous NCHW tensor has non-increasing strides however many
    gaps it has, and is accepted.
    """
    strides = tensor.stride()
    return any(later > earlier for earlier, later in zip(strides, strides[1:]))


def image_batch() -> torch.Tensor:
    """What the upscale node passes on: NHWC moved to NCHW without a copy."""
    return torch.rand(1, 96, 96, 3).movedim(-1, -3)


def pristine(function):
    """The unpatched function. Importing the node package installs the hook, so
    a test that wants the unpatched behaviour has to unwind it first."""
    while hasattr(function, '__wrapped__'):
        function = function.__wrapped__
    return function


class UpscaleContiguity(unittest.TestCase):
    def setUp(self) -> None:
        installed = comfy.utils.tiled_scale_multidim
        self.addCleanup(setattr, comfy.utils, 'tiled_scale_multidim', installed)
        self.original = pristine(installed)
        comfy.utils.tiled_scale_multidim = self.original

    def tiles(self) -> list[torch.Tensor]:
        seen: list[torch.Tensor] = []

        def record(tile: torch.Tensor) -> torch.Tensor:
            seen.append(tile)
            return tile

        comfy.utils.tiled_scale(image_batch(), record, tile_x=32, tile_y=32, overlap=8)
        self.assertGreater(len(seen), 1, 'the input has to be tiled for this to mean anything')
        return seen

    def test_the_node_layout_is_interleaved_without_the_hook(self) -> None:
        for tile in self.tiles():
            self.assertTrue(
                interleaved(tile),
                f'unpatched tiles were already acceptable, strides {tile.stride()}',
            )

    def test_the_hook_hands_the_model_an_acceptable_layout(self) -> None:
        install_contiguous_tiled_scale()
        self.assertIsNot(comfy.utils.tiled_scale_multidim, self.original)

        for tile in self.tiles():
            self.assertFalse(
                interleaved(tile),
                f'a tile still carries the permuted layout, strides {tile.stride()}',
            )

    def test_installing_twice_does_not_stack_wrappers(self) -> None:
        install_contiguous_tiled_scale()
        once = comfy.utils.tiled_scale_multidim
        install_contiguous_tiled_scale()
        self.assertIs(comfy.utils.tiled_scale_multidim, once)
        self.assertIs(once.__wrapped__, self.original)

    def test_an_already_contiguous_input_is_not_copied(self) -> None:
        install_contiguous_tiled_scale()
        contiguous = torch.rand(1, 3, 96, 96)
        seen: list[int] = []

        def record(tile: torch.Tensor) -> torch.Tensor:
            seen.append(tile.data_ptr())
            return tile

        comfy.utils.tiled_scale(contiguous, record, tile_x=32, tile_y=32, overlap=8)
        self.assertTrue(seen)
        base = contiguous.data_ptr()
        span = contiguous.numel() * contiguous.element_size()
        for pointer in seen:
            self.assertTrue(
                base <= pointer < base + span,
                'tiles came from a copy of an input that was already contiguous',
            )


if __name__ == '__main__':
    unittest.main()
