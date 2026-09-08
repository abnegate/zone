from __future__ import annotations

import asyncio
import json
import os
import subprocess
import sys
import tempfile
import unittest
from dataclasses import dataclass
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).parents[2]
COMFYUI = Path(__file__).parents[1]
INSTALL = Path(os.environ.get('COMFYUI_INSTALL_DIR', ''))
PIN = '30bdda1ef13a3a34fce2cd2fec633f15d832122a'
ZONE_NODES = COMFYUI / 'custom_nodes' / 'zone_lora'
TRAIN_SERVER = ROOT / 'runner/zone_comfy/src/train.rs'
QUALITY_SERVER = ROOT / 'runner/zone_comfy/src/quality.rs'

if not INSTALL.is_dir():
    raise RuntimeError('COMFYUI_INSTALL_DIR must point to the pinned ComfyUI checkout')
if str(COMFYUI) not in sys.path:
    sys.path.insert(0, str(COMFYUI))
if str(INSTALL) not in sys.path:
    sys.path.insert(0, str(INSTALL))

import probe_lora  # noqa: E402
import train_lora  # noqa: E402


@dataclass(frozen=True)
class Expression:
    text: str


class RustLiteral:
    def __init__(self, source: str, index: int, path: Path) -> None:
        self.source = source
        self.index = index
        self.path = path

    def skip(self) -> None:
        while self.index < len(self.source):
            if self.source[self.index].isspace():
                self.index += 1
            elif self.source.startswith('//', self.index):
                end = self.source.find('\n', self.index)
                self.index = len(self.source) if end < 0 else end + 1
            else:
                return

    def peek(self) -> str:
        self.skip()
        return self.source[self.index : self.index + 1]

    def take(self, character: str) -> None:
        found = self.peek()
        if found != character:
            raise AssertionError(
                f'{self.path}: expected {character!r} at {self.index}, found {found!r}'
            )
        self.index += 1

    def string(self) -> str:
        self.take('"')
        characters = []
        while self.index < len(self.source):
            character = self.source[self.index]
            self.index += 1
            if character == '"':
                return ''.join(characters)
            if character == '\\':
                characters.append(self.source[self.index])
                self.index += 1
            else:
                characters.append(character)
        raise AssertionError(f'{self.path}: unterminated Rust string')

    def expression(self) -> Expression:
        start = self.index
        depth = 0
        while self.index < len(self.source):
            character = self.source[self.index]
            if character == '"':
                self.string()
                continue
            if character in '([{':
                depth += 1
            elif character in ')]}':
                if depth == 0:
                    break
                depth -= 1
            elif character == ',' and depth == 0:
                break
            self.index += 1
        return Expression(self.source[start : self.index].strip())

    def more(self, closing: str) -> bool:
        if self.peek() == ',':
            self.take(',')
        if self.peek() == closing:
            self.take(closing)
            return False
        return True

    def array(self) -> list:
        self.take('[')
        values = []
        while self.more(']'):
            values.append(self.value())
        return values

    def object(self) -> dict:
        self.take('{')
        values = {}
        while self.more('}'):
            name = self.string()
            self.take(':')
            values[name] = self.value()
        return values

    def value(self):
        character = self.peek()
        if character == '"':
            return self.string()
        if character == '{':
            return self.object()
        if character == '[':
            return self.array()
        return self.expression()


def rust_graph(path: Path, function: str) -> dict:
    source = path.read_text()
    start = source.find(f'fn {function}(')
    if start < 0:
        raise AssertionError(f'{path}: {function} is missing')
    literal = source.find('json!({', start)
    if literal < 0:
        raise AssertionError(f'{path}: {function} has no graph literal')
    return RustLiteral(source, literal + len('json!('), path).object()


def rust_graphs() -> dict[str, dict]:
    flux_train = rust_graph(TRAIN_SERVER, 'flux_graph')
    qwen_train = rust_graph(TRAIN_SERVER, 'qwen_graph')
    trainer = rust_graph(TRAIN_SERVER, 'trainer')
    trainer['inputs']['model'] = ['1', 0]
    trainer['inputs']['latents'] = ['3', 0]
    trainer['inputs']['positive'] = ['4', 0]
    flux_train['5'] = trainer
    trainer = rust_graph(TRAIN_SERVER, 'trainer')
    trainer['inputs']['model'] = ['1', 0]
    trainer['inputs']['latents'] = ['5', 0]
    trainer['inputs']['positive'] = ['6', 0]
    qwen_train['7'] = trainer
    return {
        'Rust flux train': flux_train,
        'Rust qwen train': qwen_train,
        'Rust flux probe': rust_graph(QUALITY_SERVER, 'flux_graph'),
        'Rust qwen probe': rust_graph(QUALITY_SERVER, 'qwen_graph'),
    }


def flux() -> train_lora.TrainingModel:
    return train_lora.TrainingModel(architecture='flux', checkpoint='flux.safetensors')


def qwen() -> train_lora.TrainingModel:
    return train_lora.TrainingModel(
        architecture='qwen_edit',
        unet='qwen-unet.safetensors',
        clip='qwen-clip.safetensors',
        vae='qwen-vae.safetensors',
    )


def graphs() -> dict[str, dict]:
    config = train_lora.load_config()
    manifest = json.dumps(
        {
            'schema_version': 1,
            'architecture': 'flux',
            'pairs': [
                {
                    'index': 0,
                    'target': 'targets/0000.png',
                    'reference': None,
                    'instruction': 'portrait',
                }
            ],
        }
    )
    qwen_manifest = manifest.replace('"flux"', '"qwen_edit"').replace(
        '"reference": null', '"reference": "control_1/0000.png"'
    )
    return {
        'flux train': train_lora.train_graph(flux(), 'folder', manifest, 'artifact', config, 12),
        'qwen train': train_lora.train_graph(
            qwen(), 'folder', qwen_manifest, 'artifact', config, 12
        ),
        'flux loss probe': probe_lora.loss_graph(flux(), 'folder', manifest, 512, ''),
        'qwen loss probe': probe_lora.loss_graph(
            qwen(), 'folder', qwen_manifest, 512, ''
        ),
        'flux gradient probe': probe_lora.gradient_graph(
            flux(), 'folder', manifest, 512
        ),
        'qwen gradient probe': probe_lora.gradient_graph(
            qwen(), 'folder', qwen_manifest, 512
        ),
    }


def schema(node_class) -> tuple[dict[str, tuple], set[str], tuple[str, ...]]:
    declared = node_class.INPUT_TYPES()
    inputs = {}
    required = set()
    for group in ('required', 'optional'):
        for name, specification in declared.get(group, {}).items():
            inputs[name] = specification
            if group == 'required':
                required.add(name)
    return inputs, required, tuple(node_class.RETURN_TYPES)


class GraphContractTests(unittest.TestCase):
    nodes = None

    @classmethod
    def setUpClass(cls) -> None:
        revision = subprocess.check_output(
            ['git', '-C', str(INSTALL), 'rev-parse', 'HEAD'], text=True
        ).strip()
        if revision != PIN:
            raise AssertionError(f'ComfyUI checkout is {revision}, expected exact pin {PIN}')
        import nodes

        async def load_registry() -> bool:
            from server import PromptServer

            PromptServer(asyncio.get_running_loop())
            await nodes.init_extra_nodes(init_custom_nodes=True, init_api_nodes=False)
            return await nodes.load_custom_node(str(ZONE_NODES), set())

        loaded = asyncio.run(load_registry())
        if not loaded:
            raise AssertionError(f'failed to load repository extension {ZONE_NODES}')
        cls.nodes = nodes

    def test_all_train_and_probe_graphs_match_the_pinned_registry(self) -> None:
        for graph_name, graph in (graphs() | rust_graphs()).items():
            with self.subTest(graph=graph_name):
                self.assertTrue(graph)
                self.validate_graph(graph_name, graph)

    def test_rust_and_python_graph_contracts_are_identical(self) -> None:
        python = graphs()
        rust = rust_graphs()
        pairs = (
            ('flux train', 'Rust flux train'),
            ('qwen train', 'Rust qwen train'),
            ('flux loss probe', 'Rust flux probe'),
            ('qwen loss probe', 'Rust qwen probe'),
        )
        for python_name, rust_name in pairs:
            with self.subTest(python=python_name, rust=rust_name):
                self.assertEqual(
                    self.signature(python[python_name]), self.signature(rust[rust_name])
                )

    @classmethod
    def signature(cls, graph: dict) -> dict:
        return {
            identifier: (
                node['class_type'],
                frozenset(node['inputs']),
                {
                    name: linked
                    for name, value in node['inputs'].items()
                    if (linked := cls.link(value)) is not None
                },
            )
            for identifier, node in graph.items()
        }

    def validate_graph(self, graph_name: str, graph: dict) -> None:
        for identifier, node in graph.items():
            class_type = node.get('class_type')
            supplied = node.get('inputs')
            self.assertIn(
                class_type,
                self.nodes.NODE_CLASS_MAPPINGS,
                f'{graph_name} node {identifier}',
            )
            self.assertIsInstance(supplied, dict, f'{graph_name} node {identifier}')
            node_class = self.nodes.NODE_CLASS_MAPPINGS[class_type]
            inputs, required, _ = schema(node_class)
            self.assertLessEqual(required, set(supplied), f'{graph_name} node {identifier}')
            self.assertLessEqual(set(supplied), set(inputs), f'{graph_name} node {identifier}')
            for name, value in supplied.items():
                linked = self.link(value)
                if linked is not None:
                    source, slot = linked
                    self.assertIn(source, graph, f'{graph_name} node {identifier}.{name}')
                    source_class = self.nodes.NODE_CLASS_MAPPINGS[graph[source]['class_type']]
                    _, _, outputs = schema(source_class)
                    self.assertLess(slot, len(outputs), f'{graph_name} node {identifier}.{name}')
                    expected = inputs[name][0]
                    if getattr(expected, 'value', expected) != '*':
                        self.assertEqual(
                            outputs[slot],
                            expected,
                            f'{graph_name} node {identifier}.{name} link type',
                        )
                else:
                    choices = inputs[name][0]
                    if isinstance(value, Expression):
                        continue
                    if isinstance(choices, list) and name not in {
                        'ckpt_name',
                        'clip_name',
                        'lora_name',
                        'unet_name',
                        'vae_name',
                    }:
                        self.assertIn(value, choices, f'{graph_name} node {identifier}.{name}')

    @staticmethod
    def link(value) -> tuple[str, int] | None:
        slot = value[1] if isinstance(value, list) and len(value) == 2 else None
        if isinstance(slot, Expression) and slot.text.isdigit():
            slot = int(slot.text)
        if (
            isinstance(value, list)
            and len(value) == 2
            and isinstance(value[0], str)
            and isinstance(slot, int)
            and not isinstance(slot, bool)
        ):
            return value[0], slot
        return None

    def test_no_graph_uses_make_training_dataset(self) -> None:
        sources = [
            COMFYUI / 'train_lora.py',
            COMFYUI / 'probe_lora.py',
            ROOT / 'runner/zone_comfy/src/train.rs',
            ROOT / 'runner/zone_comfy/src/quality.rs',
        ]
        for source in sources:
            with self.subTest(source=source.name):
                self.assertNotIn('MakeTrainingDataset', source.read_text())

    def test_repository_extension_is_part_of_the_loaded_registry(self) -> None:
        expected = {
            'ZoneCleanupTrainingRun',
            'ZoneLoadTrainDataset',
            'ZoneProbeGradient',
            'ZoneProbeLoss',
            'ZoneStageTrainingArtifact',
            'ZoneTrainLoRA',
        }
        self.assertLessEqual(expected, set(self.nodes.NODE_CLASS_MAPPINGS))

    def test_two_pair_dataset_executes_with_native_zipped_list_mapping(self) -> None:
        node_class = self.nodes.NODE_CLASS_MAPPINGS['ZoneLoadTrainDataset']
        module = sys.modules[node_class.__module__]
        run = train_lora.Run.create()
        manifest = {
            'schema_version': 1,
            'architecture': 'qwen_edit',
            'pairs': [
                {
                    'index': index,
                    'target': f'targets/{index:04}.png',
                    'reference': f'control_1/{index:04}.png',
                    'instruction': f'instruction {index}',
                }
                for index in range(2)
            ],
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            folder = root / run.folder
            (folder / 'targets').mkdir(parents=True)
            (folder / 'control_1').mkdir()
            for index in range(2):
                Image.new('RGB', (8, 8), (index * 40, 0, 0)).save(
                    folder / f'targets/{index:04}.png'
                )
                Image.new('RGB', (8, 8), (0, index * 40, 0)).save(
                    folder / f'control_1/{index:04}.png'
                )
            original = module.folder_paths.get_input_directory
            module.folder_paths.get_input_directory = lambda: str(root)
            try:
                targets, references, instructions = node_class.execute(
                    run.folder, json.dumps(manifest), 64
                ).result
            finally:
                module.folder_paths.get_input_directory = original

        self.assertEqual(instructions, ['instruction 0', 'instruction 1'])
        self.assertEqual(len(targets), 2)
        self.assertEqual(len(references), 2)

        class Capture:
            INPUT_IS_LIST = False
            FUNCTION = 'capture'

            def capture(self, target, reference, instruction):
                return target, reference, instruction

        from execution import _async_map_node_over_list

        mapped = asyncio.run(
            _async_map_node_over_list(
                'contract',
                'capture',
                Capture(),
                {
                    'target': targets,
                    'reference': references,
                    'instruction': instructions,
                },
                'capture',
            )
        )
        self.assertEqual([item[2] for item in mapped], ['instruction 0', 'instruction 1'])
        self.assertIs(mapped[0][0], targets[0])
        self.assertIs(mapped[0][1], references[0])
        self.assertIs(mapped[1][0], targets[1])
        self.assertIs(mapped[1][1], references[1])

    def test_artifact_nodes_stage_and_clean_only_one_uuid_namespace(self) -> None:
        stage_class = self.nodes.NODE_CLASS_MAPPINGS['ZoneStageTrainingArtifact']
        cleanup_class = self.nodes.NODE_CLASS_MAPPINGS['ZoneCleanupTrainingRun']
        stage_module = sys.modules[stage_class.__module__]
        cleanup_module = sys.modules[cleanup_class.__module__]
        run = train_lora.Run.create()
        other = train_lora.Run.create()
        artifact = f'{run.artifact}.safetensors'
        checkpoint = f'{run.artifact}-step12.safetensors'
        unrelated = f'{other.artifact}.safetensors'

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            input_root = root / 'input'
            output_root = root / 'output'
            model_root = root / 'models/loras'
            (output_root / 'loras').mkdir(parents=True)
            model_root.mkdir(parents=True)
            (input_root / run.folder).mkdir(parents=True)
            (output_root / 'loras' / artifact).write_bytes(b'final')
            (output_root / 'loras' / checkpoint).write_bytes(b'checkpoint')
            (output_root / 'loras' / unrelated).write_bytes(b'unrelated')

            originals = {
                'stage_output': stage_module.folder_paths.get_output_directory,
                'stage_models': stage_module.folder_paths.get_folder_paths,
                'cleanup_input': cleanup_module.folder_paths.get_input_directory,
                'cleanup_output': cleanup_module.folder_paths.get_output_directory,
                'cleanup_models': cleanup_module.folder_paths.get_folder_paths,
            }
            stage_module.folder_paths.get_output_directory = lambda: str(output_root)
            stage_module.folder_paths.get_folder_paths = lambda _: [str(model_root)]
            cleanup_module.folder_paths.get_input_directory = lambda: str(input_root)
            cleanup_module.folder_paths.get_output_directory = lambda: str(output_root)
            cleanup_module.folder_paths.get_folder_paths = lambda _: [str(model_root)]
            try:
                stage_class.execute(artifact)
                self.assertEqual((model_root / artifact).read_bytes(), b'final')
                cleanup_class.execute(run.folder, run.artifact)
            finally:
                stage_module.folder_paths.get_output_directory = originals['stage_output']
                stage_module.folder_paths.get_folder_paths = originals['stage_models']
                cleanup_module.folder_paths.get_input_directory = originals['cleanup_input']
                cleanup_module.folder_paths.get_output_directory = originals['cleanup_output']
                cleanup_module.folder_paths.get_folder_paths = originals['cleanup_models']

            self.assertFalse((input_root / run.folder).exists())
            self.assertFalse((output_root / 'loras' / artifact).exists())
            self.assertFalse((output_root / 'loras' / checkpoint).exists())
            self.assertFalse((model_root / artifact).exists())
            self.assertTrue((output_root / 'loras' / unrelated).is_file())

    @unittest.skipUnless(hasattr(os, 'symlink'), 'symlink regression requires platform support')
    def test_dataset_node_refuses_a_symlinked_pair_file(self) -> None:
        node_class = self.nodes.NODE_CLASS_MAPPINGS['ZoneLoadTrainDataset']
        module = sys.modules[node_class.__module__]
        run = train_lora.Run.create()
        manifest = {
            'schema_version': 1,
            'architecture': 'flux',
            'pairs': [
                {
                    'index': 0,
                    'target': 'targets/0000.png',
                    'reference': None,
                    'instruction': 'portrait',
                }
            ],
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / run.folder / 'targets/0000.png'
            target.parent.mkdir(parents=True)
            victim = root / 'victim.png'
            Image.new('RGB', (8, 8), 'red').save(victim)
            target.symlink_to(victim)
            original = module.folder_paths.get_input_directory
            module.folder_paths.get_input_directory = lambda: str(root)
            try:
                with self.assertRaisesRegex(ValueError, 'symlink'):
                    node_class.execute(run.folder, json.dumps(manifest), 64)
            finally:
                module.folder_paths.get_input_directory = original
            self.assertTrue(victim.is_file())


if __name__ == '__main__':
    unittest.main()
