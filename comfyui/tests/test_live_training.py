"""Train a real adapter against a live ComfyUI and prove it beat its own base.

Every offline test passed while the backward pass was severed: the endpoint
returned 200, a .safetensors appeared, and it had learned nothing. Only a
measurement can tell those apart, so this one descends on a fixed batch, trains,
and scores the adapter against the base over the images it trained on.
"""

from __future__ import annotations

import json
import math
import os
import random
import shutil
import struct
import sys
import tempfile
import time
import unittest
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from types import ModuleType
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from PIL import Image

COMFYUI = Path(__file__).parents[1]
INSTALL = Path(
    os.environ.get(
        'COMFYUI_INSTALL_DIR',
        Path.home() / 'Library' / 'Application Support' / 'Zone' / 'ComfyUI',
    )
)
GATE = 'ZONE_LIVE_TRAIN'
CHECKPOINT = 'flux1-dev-fp8.safetensors'
TRIGGER = 'zqxtri'
RESOLUTION = 512
STEPS = 150
ITERATIONS = 40
TIMEOUT = 7200
MINIMUM_GRADIENT_NORM = 0.005
MINIMUM_IMPROVEMENT = 25.0
SUBJECT = (206, 42, 34)
BACKGROUNDS = (
    ('bone', (233, 227, 214)),
    ('slate blue', (203, 216, 232)),
    ('lilac', (222, 213, 233)),
    ('sage', (212, 230, 216)),
    ('sand', (236, 226, 200)),
    ('ash', (211, 212, 218)),
    ('clay', (232, 216, 205)),
    ('mint', (206, 231, 224)),
)
VARIABLES = (
    'ZONE_COMFY_INPUT',
    'ZONE_PROBE_CHECKPOINT',
    'ZONE_PROBE_FOLDER',
    'ZONE_PROBE_ITERATIONS',
    'ZONE_PROBE_MODE',
    'ZONE_PROBE_NAME',
    'ZONE_PROBE_RESOLUTION',
    'ZONE_PROBE_TIMEOUT',
    'ZONE_TRAIN_CHECKPOINT',
    'ZONE_TRAIN_ARCHITECTURE',
    'ZONE_TRAIN_DIR',
    'ZONE_TRAIN_OUTPUT',
    'ZONE_TRAIN_STEPS',
    'ZONE_TRAIN_TIMEOUT',
)


@contextmanager
def environment(**values: str) -> Iterator[None]:
    previous = {name: os.environ.get(name) for name in VARIABLES}
    for name in VARIABLES:
        os.environ.pop(name, None)
    os.environ.update(values)
    try:
        yield
    finally:
        for name in VARIABLES:
            os.environ.pop(name, None)
        os.environ.update({name: value for name, value in previous.items() if value is not None})


def setting(name: str, fallback: int | float | str) -> str:
    return os.environ.get(name) or str(fallback)


def corners(
    centre: tuple[float, float], radius: float, rotation: float
) -> list[tuple[float, float]]:
    x, y = centre
    return [
        (
            x + radius * math.cos(rotation + index * 2 * math.pi / 3),
            y + radius * math.sin(rotation + index * 2 * math.pi / 3),
        )
        for index in range(3)
    ]


def scenery(seed: int, tint: tuple[int, int, int]) -> Image.Image:
    """Texture, because flat colour encodes to a near-empty latent.

    A flat scene puts the loss an order of magnitude below anything training sees, and
    with it the gradient norm the severed backward pass has to be caught by.
    """
    from PIL import Image, ImageDraw, ImageFilter

    generator = random.Random(seed)
    noise = Image.frombytes(
        'RGB', (RESOLUTION, RESOLUTION), generator.randbytes(RESOLUTION * RESOLUTION * 3)
    ).filter(ImageFilter.GaussianBlur(1.4))
    image = Image.blend(Image.new('RGB', (RESOLUTION, RESOLUTION), tint), noise, 0.4)
    draw = ImageDraw.Draw(image)
    for _ in range(28):
        left = generator.randrange(-40, RESOLUTION)
        top = generator.randrange(-40, RESOLUTION)
        width = generator.randrange(30, 170)
        height = generator.randrange(20, 120)
        colour = tuple(
            min(255, max(0, channel + generator.randrange(-90, 91))) for channel in tint
        )
        draw.rectangle((left, top, left + width, top + height), fill=colour)
    return image.filter(ImageFilter.GaussianBlur(0.7))


def synthesise(targets: Path) -> None:
    """One unambiguous subject across varied scenes, so there is a subject to learn."""
    from PIL import ImageDraw

    targets.mkdir(parents=True, exist_ok=True)
    for index, (name, tint) in enumerate(BACKGROUNDS):
        image = scenery(index, tint)
        centre = (
            RESOLUTION / 2 + (index % 3 - 1) * 42,
            RESOLUTION / 2 + (index // 3 - 1) * 38,
        )
        ImageDraw.Draw(image).polygon(
            corners(centre, 150 + (index % 4) * 24, index * math.pi / 7), fill=SUBJECT
        )
        image.save(targets / f'{index:04d}.png')
        (targets / f'{index:04d}.txt').write_text(
            f'a photo of {TRIGGER}, a solid red triangle on a {name} background'
        )


def adapters(path: Path) -> int:
    """Count the modules the file actually adapts, not the bytes it happens to contain."""
    with path.open('rb') as handle:
        header = json.loads(handle.read(struct.unpack('<Q', handle.read(8))[0]))
    up = {key.rsplit('.lora_up', 1)[0] for key in header if '.lora_up' in key}
    down = {key.rsplit('.lora_down', 1)[0] for key in header if '.lora_down' in key}
    return len(up & down)


@unittest.skipUnless(
    os.environ.get(GATE) == '1',
    f'set {GATE}=1 to train against a live ComfyUI serving {CHECKPOINT}',
)
class LiveTrainingTests(unittest.TestCase):
    probe: ModuleType
    trainer: ModuleType
    config: dict
    name: str
    workspace: Path
    staging: Path
    output: Path
    gradient: dict | None = None
    adapter: Path | None = None

    @classmethod
    def setUpClass(cls) -> None:
        if str(COMFYUI) not in sys.path:
            sys.path.insert(0, str(COMFYUI))
        import probe_lora
        import train_lora

        cls.probe = probe_lora
        cls.trainer = train_lora
        cls.config = train_lora.load_config()
        cls.name = f'zone-live-{int(time.time())}'
        cls.workspace = Path(tempfile.mkdtemp(prefix='zone-live-'))
        cls.staging = INSTALL / 'input'
        cls.output = INSTALL / 'models' / 'loras' / f'{cls.name}.safetensors'
        synthesise(cls.workspace / 'targets')

    @classmethod
    def tearDownClass(cls) -> None:
        shutil.rmtree(cls.workspace, ignore_errors=True)
        for folder in cls.staging.glob(f'zone-*-{cls.name}'):
            shutil.rmtree(folder, ignore_errors=True)
        cls.output.unlink(missing_ok=True)
        for checkpoint in (INSTALL / 'output' / 'loras').glob(f'{cls.name}*.safetensors'):
            checkpoint.unlink()

    def shared(self) -> dict[str, str]:
        return {
            'ZONE_COMFY_INPUT': str(self.staging),
            'ZONE_TRAIN_DIR': str(self.workspace),
            'ZONE_TRAIN_ARCHITECTURE': 'flux',
            'ZONE_TRAIN_CHECKPOINT': CHECKPOINT,
            'ZONE_PROBE_RESOLUTION': str(RESOLUTION),
            'ZONE_PROBE_TIMEOUT': setting('ZONE_PROBE_TIMEOUT', TIMEOUT),
        }

    def measure(self, graph: dict) -> dict:
        return self.probe.report(
            self.probe.base_url(), graph, int(os.environ['ZONE_PROBE_TIMEOUT'])
        )

    def announce(self, summary: str) -> str:
        print(summary, file=sys.stderr, flush=True)
        return summary

    def test_1_gradient_reaches_the_adapters(self) -> None:
        """A severed backward pass measures 0.0004 here and an intact one 0.026."""
        with environment(
            **self.shared(),
            ZONE_PROBE_MODE='gradient',
            ZONE_PROBE_ITERATIONS=setting('ZONE_PROBE_ITERATIONS', ITERATIONS),
        ):
            model = self.trainer.TrainingModel.from_environment()
            run, manifest, _ = self.probe.dataset(model)
            try:
                report = self.measure(
                    self.probe.gradient_graph(model, run.folder, manifest, RESOLUTION)
                )
            finally:
                shutil.rmtree(self.staging / run.folder, ignore_errors=True)
        summary = self.announce(
            f'gradient norm={report["gradient_norms"][0]:.6f} '
            f'adapters={report["adapters"]} '
            f'loss {report["losses"][0]:.5f} -> {report["losses"][-1]:.5f}'
        )
        self.assertEqual(report['nonfinite'], [], summary)
        self.assertGreaterEqual(report['adapters'], int(self.config['min_adapters']), summary)
        self.assertGreaterEqual(report['gradient_norms'][0], MINIMUM_GRADIENT_NORM, summary)
        self.assertLess(report['losses'][-1], report['losses'][0], summary)
        type(self).gradient = report

    def test_2_training_writes_an_adapter_that_adapts(self) -> None:
        if self.gradient is None:
            self.skipTest('the gradient gate has to pass before a run is worth the hardware')
        with environment(
            **self.shared(),
            ZONE_TRAIN_OUTPUT=str(self.output),
            ZONE_TRAIN_CHECKPOINT=CHECKPOINT,
            ZONE_TRAIN_STEPS=setting('ZONE_TRAIN_STEPS', STEPS),
            ZONE_TRAIN_TIMEOUT=setting('ZONE_TRAIN_TIMEOUT', TIMEOUT),
        ):
            self.trainer.main()
        self.assertTrue(self.output.is_file(), str(self.output))
        self.assertGreater(self.output.stat().st_size, 10_000, str(self.output))
        self.assertGreaterEqual(
            adapters(self.output), int(self.config['min_adapters']), str(self.output)
        )
        type(self).adapter = self.output

    def test_3_the_adapter_beats_its_base(self) -> None:
        if self.adapter is None:
            self.skipTest('training has to produce an adapter before it can be scored')
        minimum = float(setting('ZONE_LIVE_TRAIN_MIN_IMPROVEMENT', MINIMUM_IMPROVEMENT))
        with environment(**self.shared()):
            model = self.trainer.TrainingModel.from_environment()
            run, manifest, _ = self.probe.dataset(model)
            try:
                base = self.measure(
                    self.probe.loss_graph(model, run.folder, manifest, RESOLUTION, '')
                )
                trained = self.measure(
                    self.probe.loss_graph(
                        model, run.folder, manifest, RESOLUTION, self.adapter.name
                    )
                )
            finally:
                shutil.rmtree(self.staging / run.folder, ignore_errors=True)
        improvement = (base['mean'] - trained['mean']) / base['mean'] * 100
        summary = self.announce(
            f'base={base["mean"]:.5f} adapter={trained["mean"]:.5f} '
            f'improvement={improvement:.2f}% minimum={minimum:.2f}% '
            f'base_by_percent={base["by_percent"]} adapter_by_percent={trained["by_percent"]}'
        )
        self.assertGreaterEqual(improvement, minimum, summary)


if __name__ == '__main__':
    unittest.main()
