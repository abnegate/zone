from __future__ import annotations

import json
import os
import sys
import unittest
from contextlib import contextmanager
from pathlib import Path

COMFYUI = Path(__file__).parents[1]
if str(COMFYUI) not in sys.path:
    sys.path.insert(0, str(COMFYUI))

import probe_lora  # noqa: E402
from train_lora import TrainingModel  # noqa: E402

VARIABLES = ('ZONE_PROBE_RANK', 'ZONE_PROBE_STRENGTH')


@contextmanager
def environment(**values: str):
    previous = {name: os.environ.get(name) for name in VARIABLES}
    for name in VARIABLES:
        os.environ.pop(name, None)
    os.environ.update(values)
    try:
        yield
    finally:
        for name in VARIABLES:
            os.environ.pop(name, None)
            if previous[name] is not None:
                os.environ[name] = previous[name]


def flux() -> TrainingModel:
    return TrainingModel(architecture='flux', checkpoint='flux.safetensors')


def qwen() -> TrainingModel:
    return TrainingModel(
        architecture='qwen_edit',
        unet='qwen-unet.safetensors',
        clip='qwen-clip.safetensors',
        vae='qwen-vae.safetensors',
    )


def config() -> dict:
    return json.loads(
        (COMFYUI / 'custom_nodes' / 'zone_lora' / 'train_config.json').read_text()
    )


class ProbeGraphTests(unittest.TestCase):
    def test_flux_loss_graph_uses_native_encoders(self) -> None:
        graph = probe_lora.loss_graph(flux(), 'folder', '{}', 512, '')
        self.assertEqual(graph['3']['class_type'], 'VAEEncode')
        self.assertEqual(graph['4']['class_type'], 'CLIPTextEncode')
        self.assertEqual(graph['5']['inputs']['positive'], ['4', 0])
        self.assertNotIn('MakeTrainingDataset', json.dumps(graph))

    def test_qwen_loss_graph_encodes_reference_and_instruction_together(self) -> None:
        graph = probe_lora.loss_graph(qwen(), 'folder', '{}', 512, '')
        self.assertEqual(graph['2']['inputs']['type'], 'qwen_image')
        self.assertEqual(graph['5']['inputs']['pixels'], ['4', 0])
        self.assertEqual(graph['6']['inputs']['image1'], ['4', 1])
        self.assertEqual(graph['6']['inputs']['prompt'], ['4', 2])
        self.assertEqual(graph['7']['inputs']['positive'], ['6', 0])
        self.assertNotIn('MakeTrainingDataset', json.dumps(graph))

    def test_each_adapter_probe_uses_a_fresh_loader_identity(self) -> None:
        first = probe_lora.loss_graph(qwen(), 'folder', '{}', 512, 'adapter.safetensors')
        second = probe_lora.loss_graph(qwen(), 'folder', '{}', 512, 'adapter.safetensors')
        first_loader = next(
            name for name, node in first.items() if node['class_type'] == 'LoraLoaderModelOnly'
        )
        second_loader = next(
            name for name, node in second.items() if node['class_type'] == 'LoraLoaderModelOnly'
        )
        self.assertNotEqual(first_loader, second_loader)


class GradientGraphTests(unittest.TestCase):
    def test_rank_defaults_to_the_rank_training_uses(self) -> None:
        with environment():
            graph = probe_lora.gradient_graph(qwen(), 'folder', '{}', 512)
        self.assertEqual(graph['7']['inputs']['rank'], int(config()['rank']))

    def test_rank_override_still_wins(self) -> None:
        with environment(ZONE_PROBE_RANK='4'):
            graph = probe_lora.gradient_graph(flux(), 'folder', '{}', 512)
        self.assertEqual(graph['5']['inputs']['rank'], 4)


if __name__ == '__main__':
    unittest.main()
