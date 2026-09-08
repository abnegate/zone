from __future__ import annotations

import ast
import importlib.util
import json
import unittest
from dataclasses import dataclass
from pathlib import Path
from types import ModuleType

ROOT = Path(__file__).parents[2]
DRIVER = ROOT / 'comfyui' / 'train_lora.py'
NODES = ROOT / 'comfyui' / 'custom_nodes' / 'zone_lora' / 'train_node.py'
CONFIG = ROOT / 'comfyui' / 'custom_nodes' / 'zone_lora' / 'train_config.json'
SERVER = ROOT / 'runner' / 'zone_comfy' / 'src' / 'train.rs'
PACKAGED = ('ZoneLoadTrainFolder', 'ZoneTrainLoRA')
ESCAPES = {'n': '\n', 'r': '\r', 't': '\t', '0': '\0'}


@dataclass(frozen=True)
class Expression:
    text: str


@dataclass
class Node:
    class_type: str
    inputs: frozenset[str]
    links: dict[str, tuple[str, int]]


class Rust:
    """Recursive-descent reader for the `json!` graph literal in train.rs.

    Values that are not literals stay opaque: the contract is the shape of the
    graph, and the Rust suite owns what those expressions evaluate to.
    """

    def __init__(self, source: str, index: int) -> None:
        self.source = source
        self.index = index

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
                f'{SERVER}: expected {character!r} at offset {self.index}, found {found!r}'
            )
        self.index += 1

    def string(self) -> str:
        self.take('"')
        characters: list[str] = []
        while self.index < len(self.source):
            character = self.source[self.index]
            self.index += 1
            if character == '"':
                return ''.join(characters)
            if character == '\\':
                escaped = self.source[self.index]
                characters.append(ESCAPES.get(escaped, escaped))
                self.index += 1
            else:
                characters.append(character)
        raise AssertionError(f'{SERVER}: unterminated string literal')

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
        character = self.peek()
        if not character:
            raise AssertionError(f'{SERVER}: graph literal ends before {closing!r}')
        if character == closing:
            self.take(closing)
            return False
        return True

    def array(self) -> list[object]:
        self.take('[')
        items: list[object] = []
        while self.more(']'):
            items.append(self.value())
        return items

    def object(self) -> dict[str, object]:
        self.take('{')
        fields: dict[str, object] = {}
        while self.more('}'):
            key = self.string()
            self.take(':')
            fields[key] = self.value()
        return fields

    def value(self) -> object:
        character = self.peek()
        if character == '"':
            return self.string()
        if character == '{':
            return self.object()
        if character == '[':
            return self.array()
        return self.expression()


def require(path: Path) -> Path:
    if not path.is_file():
        raise AssertionError(f'graph contract source is missing: {path}')
    return path


def source(path: Path) -> str:
    return require(path).read_text()


def link(value: object) -> tuple[str, int] | None:
    if not isinstance(value, list) or len(value) != 2 or not isinstance(value[0], str):
        return None
    slot = value[1]
    if isinstance(slot, Expression):
        slot = int(slot.text) if slot.text.isdigit() else None
    if not isinstance(slot, int) or isinstance(slot, bool):
        return None
    return value[0], slot


def wiring(supplied: dict[str, object]) -> dict[str, tuple[str, int]]:
    linked = ((name, link(value)) for name, value in supplied.items())
    return {name: target for name, target in linked if target is not None}


def graph(raw: object, path: Path) -> dict[str, Node]:
    if not isinstance(raw, dict) or not raw:
        raise AssertionError(f'no train graph nodes extracted from {path}')
    nodes: dict[str, Node] = {}
    for identifier, body in raw.items():
        if not isinstance(body, dict):
            raise AssertionError(f'{path}: node {identifier} is not an object')
        class_type = body.get('class_type')
        supplied = body.get('inputs')
        if not isinstance(class_type, str) or not isinstance(supplied, dict):
            raise AssertionError(f'{path}: node {identifier} has no class_type and inputs pair')
        nodes[identifier] = Node(class_type, frozenset(supplied), wiring(supplied))
    return nodes


def server() -> dict[str, Node]:
    text = source(SERVER)
    definition = text.find('fn train_graph(')
    if definition < 0:
        raise AssertionError(f'{SERVER}: fn train_graph not found')
    literal = text.find('json!(', definition)
    if literal < 0:
        raise AssertionError(f'{SERVER}: fn train_graph has no json! literal')
    reader = Rust(text, literal + len('json!('))
    return graph(reader.object(), SERVER)


def driver() -> ModuleType:
    specification = importlib.util.spec_from_file_location('zone_train_driver', require(DRIVER))
    if specification is None or specification.loader is None:
        raise AssertionError(f'{DRIVER}: cannot be loaded as a module')
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


def standalone() -> dict[str, Node]:
    built = driver().train_graph(
        checkpoint='contract.safetensors',
        folder='zone-train-contract',
        captions={'contract.png': 'a photo of contract'},
        save_name='contract',
        config=json.loads(source(CONFIG)),
        steps=1,
    )
    return graph(built, DRIVER)


def inputs(declared: ast.expr | None) -> dict[str, bool]:
    if not isinstance(declared, ast.List):
        raise AssertionError(f'{NODES}: an io.Schema declares no inputs list')
    names: dict[str, bool] = {}
    for element in declared.elts:
        if not isinstance(element, ast.Call) or not isinstance(element.func, ast.Attribute):
            continue
        if element.func.attr != 'Input':
            continue
        name = element.args[0] if element.args else None
        if not isinstance(name, ast.Constant) or not isinstance(name.value, str):
            raise AssertionError(f'{NODES}: an io Input has no literal name')
        names[name.value] = any(keyword.arg == 'default' for keyword in element.keywords)
    return names


def declaration(function: ast.FunctionDef) -> tuple[str, dict[str, bool]] | None:
    for call in ast.walk(function):
        if not isinstance(call, ast.Call) or not isinstance(call.func, ast.Attribute):
            continue
        if call.func.attr != 'Schema':
            continue
        keywords = {keyword.arg: keyword.value for keyword in call.keywords}
        identifier = keywords.get('node_id')
        if not isinstance(identifier, ast.Constant) or not isinstance(identifier.value, str):
            raise AssertionError(f'{NODES}: an io.Schema has no literal node_id')
        return identifier.value, inputs(keywords.get('inputs'))
    return None


def schema() -> dict[str, dict[str, bool]]:
    tree = ast.parse(source(NODES), filename=str(NODES))
    nodes: dict[str, dict[str, bool]] = {}
    for definition in ast.walk(tree):
        if not isinstance(definition, ast.ClassDef):
            continue
        for member in definition.body:
            if not isinstance(member, ast.FunctionDef) or member.name != 'define_schema':
                continue
            declared = declaration(member)
            if declared is not None:
                identifier, names = declared
                nodes[identifier] = names
    if not nodes:
        raise AssertionError(f'{NODES}: no io.Schema node definition found')
    return nodes


class GraphContractTests(unittest.TestCase):
    """The server and the standalone driver must post the same prompt graph.

    Neither side imports the other, so only this test stops them drifting.
    """

    server: dict[str, Node]
    standalone: dict[str, Node]
    schema: dict[str, dict[str, bool]]

    @classmethod
    def setUpClass(cls) -> None:
        cls.server = server()
        cls.standalone = standalone()
        cls.schema = schema()

    def shared(self) -> list[str]:
        return sorted(set(self.server) & set(self.standalone))

    def test_both_graphs_declare_the_same_nodes(self) -> None:
        self.assertEqual(
            frozenset(self.server),
            frozenset(self.standalone),
            f'{SERVER} and {DRIVER} build different node ids',
        )
        self.assertTrue(self.shared(), f'{SERVER} and {DRIVER} share no nodes')

    def test_every_node_runs_the_same_class(self) -> None:
        for identifier in self.shared():
            with self.subTest(node=identifier):
                self.assertEqual(
                    self.server[identifier].class_type,
                    self.standalone[identifier].class_type,
                    f'node {identifier} has a different class_type in {SERVER} and {DRIVER}',
                )

    def test_every_node_supplies_the_same_inputs(self) -> None:
        for identifier in self.shared():
            with self.subTest(node=identifier):
                self.assertEqual(
                    self.server[identifier].inputs,
                    self.standalone[identifier].inputs,
                    f'node {identifier} has different input keys in {SERVER} and {DRIVER}',
                )

    def test_wiring_between_nodes_is_identical(self) -> None:
        for identifier in self.shared():
            with self.subTest(node=identifier):
                self.assertEqual(
                    self.server[identifier].links,
                    self.standalone[identifier].links,
                    f'node {identifier} is wired differently in {SERVER} and {DRIVER}',
                )

    def test_packaged_nodes_are_used_by_both_graphs(self) -> None:
        for name in PACKAGED:
            with self.subTest(node=name):
                self.assertIn(name, self.schema, f'{NODES} no longer defines {name}')
                for path, nodes in ((SERVER, self.server), (DRIVER, self.standalone)):
                    self.assertIn(
                        name,
                        {node.class_type for node in nodes.values()},
                        f'{path} no longer builds a {name} node',
                    )

    def test_graph_inputs_are_declared_by_the_packaged_schema(self) -> None:
        for path, nodes in ((SERVER, self.server), (DRIVER, self.standalone)):
            for identifier, node in sorted(nodes.items()):
                if node.class_type not in self.schema:
                    continue
                with self.subTest(source=path.name, node=identifier):
                    self.assertLessEqual(
                        node.inputs,
                        frozenset(self.schema[node.class_type]),
                        f'{path} sends inputs {node.class_type} does not declare in {NODES}',
                    )

    def test_schema_inputs_without_defaults_are_always_supplied(self) -> None:
        for name in PACKAGED:
            required = frozenset(
                key for key, has_default in self.schema.get(name, {}).items() if not has_default
            )
            for path, nodes in ((SERVER, self.server), (DRIVER, self.standalone)):
                for identifier, node in sorted(nodes.items()):
                    if node.class_type != name:
                        continue
                    with self.subTest(source=path.name, node=identifier):
                        self.assertLessEqual(
                            required,
                            node.inputs,
                            f'{path} omits {name} inputs that {NODES} gives no default',
                        )


if __name__ == '__main__':
    unittest.main()
