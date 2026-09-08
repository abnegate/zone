from __future__ import annotations

import json
import os
import sys
import unittest
import uuid
from contextlib import contextmanager
from pathlib import Path
from unittest import mock

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

    def test_malformed_terminal_report_cancels_only_its_exact_prompt(self) -> None:
        identifier = str(uuid.uuid4())
        with (
            mock.patch.object(probe_lora, 'queue_prompt', return_value=identifier),
            mock.patch.object(
                probe_lora,
                'wait_prompt',
                return_value={
                    'status': {'completed': True, 'status_str': 'success'},
                    'outputs': {'1': {'text': ['{not-json']}},
                },
            ),
            mock.patch.object(probe_lora, 'cancel_prompt') as cancel,
        ):
            with self.assertRaises(SystemExit):
                probe_lora.report('http://comfy', {}, 10)
        cancel.assert_called_once_with('http://comfy', identifier, True)

    def test_main_retains_staged_inputs_when_probe_queue_state_is_unknown(self) -> None:
        run = probe_lora.Run.create('probe')
        with (
            mock.patch.object(
                probe_lora.TrainingModel,
                'from_environment',
                return_value=flux(),
            ),
            mock.patch.object(probe_lora, 'base_url', return_value='http://comfy'),
            mock.patch.object(probe_lora, 'load_config', return_value={'resolution': 512}),
            mock.patch.object(probe_lora, 'dataset', return_value=(run, '{}', True)),
            mock.patch.object(
                probe_lora,
                'report',
                side_effect=probe_lora.PromptFailure('unknown', False),
            ),
            mock.patch.object(probe_lora, 'remove_namespace') as remove,
        ):
            with self.assertRaises(probe_lora.PromptFailure):
                probe_lora.main()
        remove.assert_not_called()


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
