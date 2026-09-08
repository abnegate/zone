from __future__ import annotations

import unittest
from typing import TYPE_CHECKING

from test_zone_lora_training import AVAILABLE, COMFY_DIR, load_module

if TYPE_CHECKING:
    from PIL.Image import Image

RESOLUTION = 512
DARKEST = 48
BRIGHTEST = 240
BACKGROUND_BLUE = 64
MARKER = (255, 0, 255)
MARKER_DISTANCE = 96
TOLERANCE = 1.5


def scene(width: int, height: int, marker: tuple[float, float]) -> Image:
    import numpy
    from PIL import Image as pillow

    side = min(width, height)
    left = (width - side) // 2
    top = (height - side) // 2
    ramp = numpy.linspace(DARKEST, BRIGHTEST, side).astype(numpy.uint8)

    subject = numpy.empty((side, side, 3), dtype=numpy.uint8)
    subject[:, :, 0] = ramp[None, :]
    subject[:, :, 1] = ramp[:, None]
    subject[:, :, 2] = BACKGROUND_BLUE

    half = side // 40
    x = round(marker[0] * side)
    y = round(marker[1] * side)
    subject[y - half : y + half, x - half : x + half] = MARKER

    pixels = numpy.zeros((height, width, 3), dtype=numpy.uint8)
    pixels[top : top + side, left : left + side] = subject
    return pillow.fromarray(pixels, 'RGB')


def marker_centre(image: Image) -> tuple[float, float]:
    import numpy

    pixels = numpy.asarray(image).astype(numpy.int16)
    distance = numpy.abs(pixels - numpy.array(MARKER, dtype=numpy.int16)).sum(axis=2)
    rows, columns = numpy.nonzero(distance < MARKER_DISTANCE)
    if rows.size == 0:
        raise AssertionError('the centre marker did not survive the resize')
    return float(columns.mean()) + 0.5, float(rows.mean()) + 0.5


@unittest.skipUnless(AVAILABLE, f'ComfyUI runtime not installed at {COMFY_DIR}')
class CentreCropTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.node = load_module('train_node')

    def assertNoFlatBorder(self, image: Image) -> None:
        import numpy

        black = int((numpy.asarray(image).max(axis=2) == 0).sum())
        self.assertEqual(
            black,
            0,
            f'{black} pure black pixels survived, so the frame was padded rather than cropped',
        )

    def assertMarkerAt(self, image: Image, relative: tuple[float, float]) -> None:
        x, y = marker_centre(image)
        width, height = image.size
        self.assertAlmostEqual(
            x, relative[0] * width, delta=TOLERANCE, msg='the marker drifted horizontally'
        )
        self.assertAlmostEqual(
            y, relative[1] * height, delta=TOLERANCE, msg='the marker drifted vertically'
        )

    def test_landscape_keeps_only_its_centre_square(self) -> None:
        output = self.node.square(scene(1600, 900, (0.5, 0.5)), RESOLUTION)
        self.assertEqual(output.size, (RESOLUTION, RESOLUTION))
        self.assertNoFlatBorder(output)
        self.assertMarkerAt(output, (0.5, 0.5))

    def test_portrait_keeps_only_its_centre_square(self) -> None:
        output = self.node.square(scene(900, 1600, (0.5, 0.5)), RESOLUTION)
        self.assertEqual(output.size, (RESOLUTION, RESOLUTION))
        self.assertNoFlatBorder(output)
        self.assertMarkerAt(output, (0.5, 0.5))

    def test_square_input_keeps_its_framing(self) -> None:
        output = self.node.square(scene(800, 800, (0.25, 0.75)), RESOLUTION)
        self.assertEqual(output.size, (RESOLUTION, RESOLUTION))
        self.assertNoFlatBorder(output)
        self.assertMarkerAt(output, (0.25, 0.75))

    def test_requested_resolution_is_honoured(self) -> None:
        output = self.node.square(scene(1600, 900, (0.5, 0.5)), 256)
        self.assertEqual(output.size, (256, 256))
        self.assertNoFlatBorder(output)
        self.assertMarkerAt(output, (0.5, 0.5))


if __name__ == '__main__':
    unittest.main()
