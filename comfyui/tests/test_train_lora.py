from __future__ import annotations

import json
import os
import sys
import tempfile
import unittest
import uuid
from contextlib import contextmanager
from pathlib import Path

COMFYUI = Path(__file__).parents[1]
if str(COMFYUI) not in sys.path:
    sys.path.insert(0, str(COMFYUI))

import train_lora  # noqa: E402

VARIABLES = (
    'COMFYUI_MODELS_DIR',
    'ZONE_COMFY_INPUT',
    'ZONE_TRAIN_ARCHITECTURE',
    'ZONE_TRAIN_CHECKPOINT',
    'ZONE_TRAIN_CLIP',
    'ZONE_TRAIN_DEFER_CLEANUP',
    'ZONE_TRAIN_FOLDER',
    'ZONE_TRAIN_ARTIFACT',
    'ZONE_TRAIN_STEPS',
    'ZONE_TRAIN_UNET',
    'ZONE_TRAIN_VAE',
)
DATASETS = (8, 24, 100, 300)
HEALTHY_PASSES = (17.0, 19.0)


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


def flux() -> train_lora.TrainingModel:
    return train_lora.TrainingModel(architecture='flux', checkpoint='flux.safetensors')


def qwen() -> train_lora.TrainingModel:
    return train_lora.TrainingModel(
        architecture='qwen_edit',
        unet='qwen-unet.safetensors',
        clip='qwen-clip.safetensors',
        vae='qwen-vae.safetensors',
    )


class TrainingModelTests(unittest.TestCase):
    def test_model_family_is_required_and_never_inferred(self) -> None:
        with environment():
            with self.assertRaises(SystemExit):
                train_lora.TrainingModel.from_environment()

    def test_prompt_response_must_have_the_exact_comfy_shape(self) -> None:
        identifier = str(uuid.uuid4())
        self.assertEqual(
            train_lora.prompt_id(
                {'prompt_id': identifier, 'number': 0, 'node_errors': {}}
            ),
            identifier,
        )
        with self.assertRaises(SystemExit):
            train_lora.prompt_id(
                {
                    'prompt_id': identifier,
                    'number': 0,
                    'node_errors': {},
                    'unexpected': True,
                }
            )

    def test_qwen_requires_every_explicit_component(self) -> None:
        with environment(
            ZONE_TRAIN_ARCHITECTURE='qwen_edit',
            ZONE_TRAIN_UNET='qwen-unet.safetensors',
            ZONE_TRAIN_CLIP='qwen-clip.safetensors',
        ):
            with self.assertRaises(SystemExit):
                train_lora.TrainingModel.from_environment()

    def test_flux_does_not_fall_back_to_a_global_checkpoint(self) -> None:
        with environment(ZONE_TRAIN_ARCHITECTURE='flux'):
            with self.assertRaises(SystemExit):
                train_lora.TrainingModel.from_environment()

    def test_weight_names_are_confined_to_one_model_file(self) -> None:
        with environment(
            ZONE_TRAIN_ARCHITECTURE='flux', ZONE_TRAIN_CHECKPOINT='../outside.safetensors'
        ):
            with self.assertRaises(SystemExit):
                train_lora.TrainingModel.from_environment()


class GraphTests(unittest.TestCase):
    def config(self) -> dict:
        return train_lora.load_config()

    def test_flux_uses_native_vae_and_clip_encoders(self) -> None:
        graph = train_lora.train_graph(flux(), 'folder', '{}', 'artifact', self.config(), 12)
        self.assertEqual(graph['1']['class_type'], 'CheckpointLoaderSimple')
        self.assertEqual(graph['3']['class_type'], 'VAEEncode')
        self.assertEqual(graph['4']['class_type'], 'CLIPTextEncode')
        self.assertEqual(graph['5']['inputs']['positive'], ['4', 0])
        self.assertNotIn('MakeTrainingDataset', json.dumps(graph))

    def test_qwen_uses_the_edit_reference_and_target_at_the_same_index(self) -> None:
        graph = train_lora.train_graph(qwen(), 'folder', '{}', 'artifact', self.config(), 12)
        self.assertEqual(graph['1']['class_type'], 'UNETLoader')
        self.assertEqual(graph['1']['inputs']['unet_name'], 'qwen-unet.safetensors')
        self.assertEqual(graph['2']['class_type'], 'CLIPLoader')
        self.assertEqual(graph['2']['inputs']['type'], 'qwen_image')
        self.assertEqual(graph['2']['inputs']['clip_name'], 'qwen-clip.safetensors')
        self.assertEqual(graph['3']['class_type'], 'VAELoader')
        self.assertEqual(graph['3']['inputs']['vae_name'], 'qwen-vae.safetensors')
        self.assertEqual(graph['5']['class_type'], 'VAEEncode')
        self.assertEqual(graph['6']['class_type'], 'TextEncodeQwenImageEditPlus')
        self.assertEqual(graph['5']['inputs']['pixels'], ['4', 0])
        self.assertEqual(graph['6']['inputs']['prompt'], ['4', 2])
        self.assertEqual(graph['6']['inputs']['image1'], ['4', 1])
        self.assertEqual(graph['7']['inputs']['positive'], ['6', 0])
        self.assertNotIn('CheckpointLoaderSimple', json.dumps(graph))
        self.assertNotIn('MakeTrainingDataset', json.dumps(graph))


class DatasetTests(unittest.TestCase):
    def test_qwen_manifest_and_files_preserve_two_pair_order(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'source'
            destination = root / 'input' / train_lora.Run.create().folder
            (source / 'targets').mkdir(parents=True)
            (source / 'control_1').mkdir()
            for index in range(2):
                (source / f'targets/{index:04}.png').write_bytes(bytes([index]))
                (source / f'targets/{index:04}.txt').write_text(f'instruction {index}')
                (source / f'control_1/{index:04}.png').write_bytes(bytes([index + 10]))
            count, manifest_json = train_lora.stage_dataset(source, destination, qwen())
            manifest = json.loads(manifest_json)
            self.assertEqual(count, 2)
            self.assertEqual(manifest['architecture'], 'qwen_edit')
            self.assertEqual(
                manifest['pairs'],
                [
                    {
                        'index': 0,
                        'target': 'targets/0000.png',
                        'reference': 'control_1/0000.png',
                        'instruction': 'instruction 0',
                    },
                    {
                        'index': 1,
                        'target': 'targets/0001.png',
                        'reference': 'control_1/0001.png',
                        'instruction': 'instruction 1',
                    },
                ],
            )
            self.assertEqual((destination / 'targets/0001.png').read_bytes(), bytes([1]))
            self.assertEqual((destination / 'control_1/0001.png').read_bytes(), bytes([11]))

    def test_qwen_refuses_a_missing_reference(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'source/targets').mkdir(parents=True)
            (root / 'source/control_1').mkdir()
            (root / 'source/targets/0000.png').write_bytes(b'target')
            (root / 'source/targets/0000.txt').write_text('instruction')
            with self.assertRaises(SystemExit):
                train_lora.stage_dataset(root / 'source', root / 'input/run', qwen())

    def test_run_names_are_generated_uuid_names(self) -> None:
        first = train_lora.Run.create()
        second = train_lora.Run.create()
        first.validate()
        second.validate()
        self.assertNotEqual(first, second)
        self.assertNotEqual(
            first.folder.removeprefix('zone-train-'),
            first.artifact.removeprefix('zone-lora-'),
        )

    def test_host_supplied_run_names_are_validated_as_a_pair(self) -> None:
        generated = train_lora.Run.create()
        with environment(
            ZONE_TRAIN_FOLDER=generated.folder,
            ZONE_TRAIN_ARTIFACT=generated.artifact,
        ):
            self.assertEqual(train_lora.Run.from_environment(), generated)
        with environment(ZONE_TRAIN_FOLDER=generated.folder):
            with self.assertRaises(SystemExit):
                train_lora.Run.from_environment()


class TrainStepsTests(unittest.TestCase):
    def test_budget_is_monotonic_and_inside_the_measured_flux_band(self) -> None:
        with environment():
            settings = train_lora.load_config()
            passes = [train_lora.train_steps(count, settings) / count for count in DATASETS]
            self.assertEqual(passes, sorted(passes))
            low, high = HEALTHY_PASSES
            self.assertTrue(all(low <= value <= high for value in passes))

    def test_override_wins_over_the_budget(self) -> None:
        with environment(ZONE_TRAIN_STEPS='37'):
            self.assertEqual(train_lora.train_steps(8, train_lora.load_config()), 37)


class ComfyInputDirTests(unittest.TestCase):
    def test_override_wins(self) -> None:
        with environment(
            ZONE_COMFY_INPUT='/srv/comfy/input', COMFYUI_MODELS_DIR='/srv/comfy/models'
        ):
            self.assertEqual(train_lora.comfy_input_dir(), Path('/srv/comfy/input'))

    def test_derived_from_models_dir(self) -> None:
        with environment(COMFYUI_MODELS_DIR='/srv/comfy/models'):
            self.assertEqual(train_lora.comfy_input_dir(), Path('/srv/comfy/input'))

    def test_requires_a_location(self) -> None:
        with environment():
            with self.assertRaises(SystemExit):
                train_lora.comfy_input_dir()


if __name__ == '__main__':
    unittest.main()
