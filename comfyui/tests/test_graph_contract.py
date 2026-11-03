from __future__ import annotations

import asyncio
import functools
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib
import unittest
from dataclasses import dataclass
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).parents[2]
COMFYUI = Path(__file__).parents[1]
INSTALL = Path(os.environ.get('COMFYUI_INSTALL_DIR', ''))
PIN = '30bdda1ef13a3a34fce2cd2fec633f15d832122a'
ZONE_NODES = COMFYUI / 'custom_nodes' / 'zone_lora'
CARGO_LOCK = ROOT / 'runner/Cargo.lock'
CONTRACT = ROOT / 'runner/zone_server/src/services/comfy/contract.rs'
OVERLAY_CATALOG = COMFYUI / 'recipes/catalog.json'
CRATE = 'abnegate-comfy'
SOURCE_VARIABLE = 'ABNEGATE_COMFY_SOURCE'
PACKAGED_CATALOG = 'comfyui/recipes/catalog.json'
FACE_CHECKPOINT_NODE = '4'
FACE_FIRST_FREE_ID = 5
CONSTANT_NAME = re.compile(r'[A-Z][A-Z0-9_]*')
STRING_CONSTANT = re.compile(r'const (\w+): &str = "([^"\\]*)";')
FLOAT_CONSTANT = re.compile(r'const (\w+): f(?:32|64) = ([0-9][0-9_]*(?:\.[0-9_]*)?);')
INTEGER_CONSTANT = re.compile(r'const (\w+): [iu](?:8|16|32|64|128|size) = ([0-9][0-9_]*);')
CRATE_IMPORT = re.compile(
    r'^[ \t]*(?:pub(?:\([^)]*\))?\s+)?use crate::([\w:]+?)::(\{[^}]*\}|\w+(?:\s+as\s+\w+)?);',
    re.MULTILINE,
)

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

    def json(self) -> object:
        self.skip()
        if not self.source.startswith('json!(', self.index):
            raise AssertionError(f'{self.path}: expected json!( at {self.index}')
        self.index += len('json!(')
        value = self.value()
        self.take(')')
        return value

    def arguments(self) -> list:
        self.take('(')
        values = []
        while self.more(')'):
            self.skip()
            if self.source.startswith('json!(', self.index):
                values.append(self.json())
            else:
                values.append(self.expression())
        return values


@dataclass(frozen=True)
class RustFunction:
    module: str
    name: str
    path: Path
    source: str
    start: int
    signature: int
    end: int

    @property
    def body(self) -> str:
        return self.source[self.start : self.end]

    @property
    def label(self) -> str:
        return f'{self.path}: {self.module}::{self.name}'

    def reader(self, offset: int) -> RustLiteral:
        return RustLiteral(self.source, self.start + offset, self.path)

    def parameters(self) -> list[str]:
        reader = self.reader(self.signature)
        return [
            argument.text.split(':', 1)[0].strip()
            for argument in reader.arguments()
            if isinstance(argument, Expression)
        ]

    def literal(self) -> dict:
        offset = self.body.find('json!({')
        if offset < 0:
            raise AssertionError(f'{self.label} has no graph literal')
        return self.reader(offset).json()


def locked_version(package: str) -> str:
    if not CARGO_LOCK.is_file():
        raise AssertionError(f'{CARGO_LOCK} is missing, so the locked {package} is unknown')
    lock = tomllib.loads(CARGO_LOCK.read_text())
    versions = {
        entry['version'] for entry in lock.get('package', []) if entry.get('name') == package
    }
    if len(versions) != 1:
        raise AssertionError(
            f'{CARGO_LOCK} locks {sorted(versions) or "no"} {package}, expected exactly one version'
        )
    return versions.pop()


@functools.cache
def crate_source() -> Path:
    configured = os.environ.get(SOURCE_VARIABLE)
    if configured:
        source = Path(configured).expanduser()
        if not (source / 'src/lib.rs').is_file():
            raise AssertionError(f'{SOURCE_VARIABLE}={configured} holds no {CRATE} source')
        return source
    version = locked_version(CRATE)
    registry = Path(os.environ.get('CARGO_HOME') or Path.home() / '.cargo') / 'registry/src'
    for source in sorted(registry.glob(f'*/{CRATE}-{version}')):
        if (source / 'src/lib.rs').is_file():
            return source
    raise AssertionError(
        f'no {CRATE} {version} source: set {SOURCE_VARIABLE} to it, '
        f'or run `cargo fetch` in runner/ so it lands under {registry}'
    )


def module_files(module: str, source: Path | None = None) -> list[Path]:
    directory = (source or crate_source()) / 'src'
    files = [directory / f'{module}.rs', *sorted((directory / module).rglob('*.rs'))]
    present = [path for path in files if path.is_file()]
    if not present:
        raise AssertionError(f'{directory}: {CRATE} has no {module} module')
    return present


def rust_function(module: str, name: str) -> RustFunction:
    definition = re.compile(
        rf'^([ \t]*)(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+{name}\s*[<(]', re.MULTILINE
    )
    found = [
        (path, source, match)
        for path in module_files(module)
        for source in [path.read_text()]
        for match in definition.finditer(source)
    ]
    if len(found) != 1:
        locations = [f'{path}:{line(source, match.start())}' for path, source, match in found]
        raise AssertionError(
            f'{CRATE} {module}::{name}: expected one definition, found {len(found)} {locations}'
        )
    path, source, match = found[0]
    closing = source.find(f'\n{match.group(1)}}}\n', match.start())
    if closing < 0:
        raise AssertionError(f'{path}: {module}::{name} has no closing brace')
    signature = source.index('(', match.end() - 1) - match.start()
    return RustFunction(module, name, path, source, match.start(), signature, closing + 1)


def line(source: str, index: int) -> int:
    return source.count('\n', 0, index) + 1


@functools.cache
def contract_constants() -> dict[str, str]:
    if not CONTRACT.is_file():
        raise AssertionError(f'{CONTRACT} is missing, so no contract.<field> can be resolved')
    return dict(re.findall(r'pub const (\w+): &str = "([^"\\]*)";', CONTRACT.read_text()))


def contract_constant(field: str) -> str:
    constants = contract_constants()
    name = field.upper()
    if name not in constants:
        raise AssertionError(f'{CONTRACT}: no `pub const {name}: &str` for contract.{field}')
    return constants[name]


def resolve(value: object, substitutions: dict[str, object]) -> object:
    if isinstance(value, Expression):
        if value.text in substitutions:
            return substitutions[value.text]
        field = re.fullmatch(r'&?(?:[\w.]+\.)?contract\.(\w+)', value.text)
        return contract_constant(field.group(1)) if field else value
    if isinstance(value, list):
        return [resolve(item, substitutions) for item in value]
    if isinstance(value, dict):
        return {name: resolve(item, substitutions) for name, item in value.items()}
    return value


@dataclass(frozen=True)
class Import:
    module: str
    name: str


@functools.cache
def declared_constants(source: Path, module: str) -> dict[str, str | float | int]:
    constants = {}
    for path in module_files(module, source):
        text = path.read_text()
        constants |= dict(STRING_CONSTANT.findall(text))
        constants |= {
            name: float(value.replace('_', '')) for name, value in FLOAT_CONSTANT.findall(text)
        }
        constants |= {
            name: int(value.replace('_', '')) for name, value in INTEGER_CONSTANT.findall(text)
        }
    return constants


@functools.cache
def imported_constants(source: Path, module: str) -> dict[str, Import]:
    imports = {}
    for path in module_files(module, source):
        for origin, items in CRATE_IMPORT.findall(path.read_text()):
            for item in items.strip('{}').split(','):
                name, _, alias = (part.strip() for part in item.strip().partition(' as '))
                if CONSTANT_NAME.fullmatch(name):
                    imports[alias or name] = Import(origin.replace('::', '/'), name)
    return imports


def imported_constant(
    source: Path, origin: Import, visited: frozenset[str]
) -> str | float | int | None:
    declared = declared_constants(source, origin.module)
    if origin.name in declared:
        return declared[origin.name]
    onward = imported_constants(source, origin.module).get(origin.name)
    if onward is None or onward.module in visited:
        return None
    return imported_constant(source, onward, visited | {origin.module})


@functools.cache
def crate_constants(source: Path, module: str) -> dict[str, str | float | int]:
    imported = {
        name: value
        for name, origin in imported_constants(source, module).items()
        if (value := imported_constant(source, origin, frozenset({module}))) is not None
    }
    return imported | declared_constants(source, module)


def module_constants(module: str, source: Path | None = None) -> dict[str, str | float | int]:
    return crate_constants(source or crate_source(), module)


def expressions(value: object) -> list[Expression]:
    if isinstance(value, Expression):
        return [value]
    if isinstance(value, list):
        return [found for item in value for found in expressions(item)]
    if isinstance(value, dict):
        return [found for item in value.values() for found in expressions(item)]
    return []


def unresolved(module: str, name: str, source: Path) -> str:
    origin = imported_constants(source, module).get(name)
    if origin is None:
        return (
            f'{CRATE} {module} neither declares nor imports a &str, float or integer '
            f'`const {name}`'
        )
    return (
        f'{CRATE} {module} imports it from {origin.module}, which declares no &str, '
        f'float or integer `const {origin.name}`'
    )


def resolve_graph(
    label: str,
    module: str,
    graph: dict,
    substitutions: dict[str, object] | None = None,
    source: Path | None = None,
) -> dict:
    crate = source or crate_source()
    resolved = resolve(graph, module_constants(module, crate) | (substitutions or {}))
    for expression in expressions(resolved):
        if CONSTANT_NAME.fullmatch(expression.text):
            raise AssertionError(
                f'{label}: cannot resolve {expression.text}: '
                f'{unresolved(module, expression.text, crate)}'
            )
    for identifier, node in resolved.items():
        class_type = node.get('class_type') if isinstance(node, dict) else None
        if not isinstance(class_type, str):
            raise AssertionError(f'{label}: node {identifier} has class_type {class_type!r}')
    return resolved


def inline(caller: RustFunction, callee: RustFunction) -> dict[str, dict]:
    nodes = {}
    for call in re.finditer(rf'graph\["(\w+)"\]\s*=\s*{callee.name}\(', caller.body):
        arguments = caller.reader(call.end() - 1).arguments()
        parameters = callee.parameters()
        if len(arguments) != len(parameters):
            raise AssertionError(
                f'{caller.label} passes {len(arguments)} arguments to '
                f'{callee.name}, which takes {len(parameters)}'
            )
        bound = dict(zip(parameters, arguments, strict=True))
        nodes[call.group(1)] = resolve(callee.literal(), module_constants(callee.module) | bound)
    if not nodes:
        raise AssertionError(f'{caller.label} never calls {callee.name}')
    return nodes


def rust_graph(module: str, name: str) -> dict:
    function = rust_function(module, name)
    graph = function.literal()
    if module == 'train':
        graph |= inline(function, rust_function('train', 'trainer'))
    return resolve_graph(function.label, module, graph)


def rust_graphs() -> dict[str, dict]:
    return {
        'Rust flux train': rust_graph('train', 'flux_graph'),
        'Rust qwen train': rust_graph('train', 'qwen_graph'),
        'Rust flux probe': rust_graph('quality', 'flux_graph'),
        'Rust qwen probe': rust_graph('quality', 'qwen_graph'),
    }


def module_strings(module: str) -> dict[str, str]:
    return {
        name: value for name, value in module_constants(module).items() if isinstance(value, str)
    }


def module_numbers(module: str) -> dict[str, float]:
    return {
        name: value for name, value in module_constants(module).items() if isinstance(value, float)
    }


def catalogs() -> list[Path]:
    return [crate_source() / PACKAGED_CATALOG, OVERLAY_CATALOG]


def face_filenames(directory: str) -> set[str]:
    filenames = set()
    for path in catalogs():
        if not path.is_file():
            raise AssertionError(f'{path} is missing')
        for recipe in json.loads(path.read_text())['recipes']:
            for required in recipe.get('face_required_files', []):
                if required['directory'] == directory:
                    filenames.add(required['filename'])
    return filenames


def face_files(function: RustFunction) -> dict[str, str]:
    directories = module_strings('recipe')
    files = {}
    for binding in re.finditer(r'let (\w+) = recipe\s*\.face_file\((\w+)\)', function.body):
        name, constant = binding.groups()
        if constant not in directories:
            raise AssertionError(f'{CRATE} recipe module has no &str constant {constant}')
        filenames = face_filenames(directories[constant])
        if len(filenames) != 1:
            raise AssertionError(
                f'{[str(path) for path in catalogs()]} name {sorted(filenames)} as the face '
                f'file in {directories[constant]}/, expected exactly one'
            )
        files[f'{name}.filename'] = filenames.pop()
    if not files:
        raise AssertionError(f'{function.label} reads no face file from its recipe')
    return files


def face_node_ids(function: RustFunction) -> dict[str, str]:
    allocations = re.findall(r'let \[([\w\s,]+)\]\s*=\s*\[([\d\s,]+)\]\s*\.map', function.body)
    if len(allocations) != 1:
        raise AssertionError(f'{function.label}: expected one node id allocation')
    names, offsets = allocations[0]
    return {
        name.strip(): str(FACE_FIRST_FREE_ID + int(offset))
        for name, offset in zip(names.split(','), offsets.split(','), strict=True)
    }


def face_node_class(function: RustFunction) -> str:
    position = function.parameters().index('node')
    calls = []
    for path in crate_source().joinpath('src').rglob('*.rs'):
        source = path.read_text()
        for call in re.finditer(rf'(?<!fn ){function.name}\(', source):
            arguments = RustLiteral(source, call.end() - 1, path).arguments()
            node = arguments[position] if len(arguments) > position else None
            if isinstance(node, Expression) and 'contract.' in node.text:
                calls.append((path, node))
    if len(calls) != 1:
        raise AssertionError(
            f'{CRATE}: expected one {function.name} call naming its node from the contract, '
            f'found {[str(path) for path, _ in calls]}'
        )
    resolved = resolve(calls[0][1], {})
    if not isinstance(resolved, str):
        raise AssertionError(f'{calls[0][0]}: {calls[0][1].text} is not a contract field')
    return resolved


def rust_face_graph() -> dict:
    function = rust_function('people', 'attach_face')
    graph = {
        FACE_CHECKPOINT_NODE: {
            'class_type': 'CheckpointLoaderSimple',
            'inputs': {'ckpt_name': 'model.safetensors'},
        }
    }
    ids = face_node_ids(function)
    substitutions = (
        face_files(function)
        | ids
        | {'node': face_node_class(function), 'model': [FACE_CHECKPOINT_NODE, 0]}
    )
    for insert in re.finditer(
        r'nodes\.insert\(\s*(\w+)(?:\.clone\(\))?,\s*json!\(', function.body
    ):
        key = insert.group(1)
        if key not in ids:
            raise AssertionError(f'{function.label}: node key {key} is not an allocated id')
        node = function.reader(insert.end() - len('json!(')).json()
        graph[ids[key]] = node
    return resolve_graph(function.label, 'people', graph, substitutions)


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

        # ComfyUI picks its device while `comfy.model_management` is being
        # imported, and defaults to CUDA. CI has CPU-only torch, where that
        # import raises before a single contract can be read, so the choice
        # has to be made before `nodes` pulls it in.
        from comfy.cli_args import args

        args.cpu = True
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
                        'control_net_name',
                        'image',
                        'ipadapter_name',
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
            *module_files('train'),
            *module_files('quality'),
        ]
        for source in sources:
            with self.subTest(source=str(source)):
                self.assertNotIn('MakeTrainingDataset', source.read_text())

    def test_repository_extension_is_part_of_the_loaded_registry(self) -> None:
        expected = {
            'ZoneCleanupTrainingRun',
            'ZoneIPAdapterFace',
            'ZoneLoadTrainDataset',
            'ZoneLoadTrainFolder',
            'ZoneProbeGradient',
            'ZoneProbeLoss',
            'ZoneStageTrainingArtifact',
            'ZoneTrainLoRA',
        }
        self.assertLessEqual(expected, set(self.nodes.NODE_CLASS_MAPPINGS))

    def test_rust_face_graph_matches_the_pinned_registry(self) -> None:
        graph = rust_face_graph()
        self.assertEqual(
            sorted(node['class_type'] for node in graph.values()),
            ['CLIPVisionLoader', 'CheckpointLoaderSimple', 'LoadImage', 'ZoneIPAdapterFace'],
        )
        self.validate_graph('Rust face', graph)

    def test_rust_face_constants_fit_the_face_node(self) -> None:
        node_class = self.nodes.NODE_CLASS_MAPPINGS['ZoneIPAdapterFace']
        inputs, _, _ = schema(node_class)
        bounds = inputs['strength'][1]
        strength = module_numbers('people')['IPADAPTER_STRENGTH']
        self.assertGreaterEqual(strength, bounds['min'], f'{CRATE} people::IPADAPTER_STRENGTH')
        self.assertLessEqual(strength, bounds['max'], f'{CRATE} people::IPADAPTER_STRENGTH')
        directory = module_strings('recipe')['IPADAPTER_DIRECTORY']
        self.assertEqual(
            face_filenames(directory),
            {sys.modules[node_class.__module__].PLUS_FACE},
            f'{[str(path) for path in catalogs()]}: face_required_files in {directory}/',
        )

    def test_face_node_at_zero_strength_returns_the_model_untouched(self) -> None:
        from execution import _async_map_node_over_list

        node_class = self.nodes.NODE_CLASS_MAPPINGS['ZoneIPAdapterFace']
        model = object()
        results = asyncio.run(
            _async_map_node_over_list(
                'contract',
                'face',
                node_class,
                {
                    'model': [model],
                    'clip_vision': [None],
                    'image': [None],
                    'ipadapter_name': ['missing.safetensors'],
                    'strength': [0.0],
                },
                node_class.FUNCTION,
                v3_data={},
            )
        )
        self.assertEqual(len(results), 1)
        self.assertIs(results[0].result[0], model)

    def test_sdxl_people_graphs_match_the_pinned_registry(self) -> None:
        for name in (
            'sdxl-api.json',
            'sdxl-img2img-api.json',
            'sdxl-adapter-api.json',
            'sdxl-adapter-img2img-api.json',
        ):
            graph = json.loads((COMFYUI / 'workflows' / name).read_text())
            with self.subTest(graph=name):
                self.validate_graph(name, graph)

    def test_wan_a14b_graphs_match_the_pinned_registry(self) -> None:
        for name in ('wan2.2-a14b-t2v-api.json', 'wan2.2-a14b-i2v-api.json'):
            graph = json.loads((COMFYUI / 'workflows' / name).read_text())
            with self.subTest(graph=name):
                self.validate_graph(name, graph)

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
        progress = f'{run.artifact}-progress.json'
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
            (output_root / 'loras' / progress).write_text('{"step":1,"total":400}')
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
            self.assertFalse((output_root / 'loras' / progress).exists())
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


class ConstantResolutionTests(unittest.TestCase):
    def crate(self, modules: dict[str, str]) -> Path:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        source = Path(directory.name)
        for name, text in ({'lib': ''} | modules).items():
            path = source / 'src' / f'{name}.rs'
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        return source

    @staticmethod
    def face(class_type: str) -> dict:
        return {
            '5': {
                'class_type': Expression(class_type),
                'inputs': {'image': Expression('face'), 'strength': Expression('STRENGTH')},
            }
        }

    def test_a_constant_declared_in_the_module_resolves(self) -> None:
        source = self.crate(
            {
                'people': 'const LOAD_IMAGE: &str = "LoadImage";\n'
                'const STRENGTH: f64 = 0.4;\n',
            }
        )
        graph = resolve_graph(
            'face', 'people', self.face('LOAD_IMAGE'), {'face': 'face.png'}, source
        )
        self.assertEqual(
            graph['5'],
            {'class_type': 'LoadImage', 'inputs': {'image': 'face.png', 'strength': 0.4}},
        )

    def test_a_constant_imported_from_another_module_resolves(self) -> None:
        source = self.crate(
            {
                'recipe': 'pub(crate) const LOAD_IMAGE: &str = "LoadImage";\n'
                'pub(crate) const STRENGTH: f64 = 0.4;\n'
                'pub struct Recipe;\n',
                'people': 'use crate::recipe::LOAD_IMAGE;\n'
                'use crate::recipe::{Recipe, STRENGTH};\n',
            }
        )
        graph = resolve_graph(
            'face', 'people', self.face('LOAD_IMAGE'), {'face': 'face.png'}, source
        )
        self.assertEqual(
            graph['5'],
            {'class_type': 'LoadImage', 'inputs': {'image': 'face.png', 'strength': 0.4}},
        )

    def test_an_import_whose_module_declares_no_constant_fails_naming_it(self) -> None:
        source = self.crate(
            {
                'recipe': 'pub(crate) const STRENGTH: f64 = 0.4;\n',
                'people': 'use crate::recipe::{LOAD_IMAGE, STRENGTH};\n',
            }
        )
        with self.assertRaisesRegex(AssertionError, r'face: cannot resolve LOAD_IMAGE: .* recipe'):
            resolve_graph('face', 'people', self.face('LOAD_IMAGE'), {'face': 'face.png'}, source)

    def test_a_constant_neither_declared_nor_imported_fails_naming_it(self) -> None:
        source = self.crate({'people': 'const STRENGTH: f64 = 0.4;\n'})
        with self.assertRaisesRegex(AssertionError, r'face: cannot resolve LOAD_IMAGE: '):
            resolve_graph('face', 'people', self.face('LOAD_IMAGE'), {'face': 'face.png'}, source)


if __name__ == '__main__':
    unittest.main()
