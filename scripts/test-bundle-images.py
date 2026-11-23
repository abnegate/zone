#!/usr/bin/env python3
"""Check scripts/bundle-images.txt against every image Compose and the Helm charts render."""

import json
import os
import re
import shutil
import subprocess
import unittest
from collections.abc import Iterator, Mapping
from dataclasses import dataclass, field
from enum import StrEnum
from functools import cache
from pathlib import Path
from tempfile import TemporaryDirectory

import yaml

ROOT = Path(__file__).resolve().parents[1]
LISTING = ROOT / 'scripts' / 'bundle-images.txt'
COMPOSE = ROOT / 'docker-compose.yml'
CHARTS = ROOT / 'helm'
VALUE_SETS = ('examples/*.yaml', 'ci/*.yaml', 'values-*.yaml')
NAMESPACE = 'zone'
DEFAULT_VALUES = 'default values'
RENDER_TIMEOUT = 300
PASSED_VARIABLES = ('PATH', 'HOME', 'DOCKER_CONFIG', 'DOCKER_HOST', 'XDG_CONFIG_HOME')
DOCKER_HUB = ('', 'docker.io', 'index.docker.io')
IMAGE_KEYS = ('image', 'imageName')
SENTINEL_HOST = 'bundle-images.invalid'
SENTINEL_TAG = 'sentinel'
SENTINEL = re.compile(rf'{re.escape(SENTINEL_HOST)}/consumer-(?P<index>[0-9]+)[:@]')

NAME = re.compile(r'[a-z0-9-]+')
TARGET = re.compile(r'[a-z0-9._/-]+')
ZONE_SOURCE = re.compile(r'zone:(?P<image>server|manager|litellm)')
REPOSITORY_PATH = re.compile(r'[a-z0-9]+(?:[._-]+[a-z0-9]+)*(?:/[a-z0-9]+(?:[._-]+[a-z0-9]+)*)*')
TAG = re.compile(r'[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}')
DIGEST = re.compile(r'sha256:[0-9a-f]{64}')
COMPOSE_CONSUMER = re.compile(r'compose:(?P<key>[a-zA-Z0-9][a-zA-Z0-9_.-]*)')
HELM_CONSUMER = re.compile(
    r'(?P<kind>helm-split|helm-string):(?P<chart>[a-z0-9-]+):(?P<key>[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)*)'
)
EXCLUSION = re.compile(r'#excluded[ \t]+(?P<service>[a-zA-Z0-9][a-zA-Z0-9_.-]*)[ \t]+(?P<reason>\S.*)')
EXCLUSION_PREFIX = '#excluded'

ZONE_COMPOSE_IMAGES = {
    'zone-manager': 'zone:server',
    'zone-console': 'zone:manager',
    'zone-litellm': 'zone:litellm',
}
ZONE_HELM_REPOSITORIES = {
    'zone/server': 'zone:server',
    'zone/manager': 'zone:manager',
    'zone/litellm': 'zone:litellm',
}

MISSING = object()


class Kind(StrEnum):
    COMPOSE = 'compose'
    SPLIT = 'helm-split'
    STRING = 'helm-string'


@dataclass(frozen=True)
class Consumer:
    kind: Kind
    chart: str
    key: str

    def __str__(self) -> str:
        if self.kind is Kind.COMPOSE:
            return f'{self.kind}:{self.key}'
        return f'{self.kind}:{self.chart}:{self.key}'


@dataclass(frozen=True)
class Entry:
    line: int
    name: str
    source: str
    target: str
    consumers: tuple[Consumer, ...]

    @property
    def zone(self) -> bool:
        return ZONE_SOURCE.fullmatch(self.source) is not None


@dataclass(frozen=True)
class Listing:
    entries: tuple[Entry, ...]
    exclusions: Mapping[str, str]
    problems: tuple[str, ...]

    def consumers(self, kind: Kind, chart: str = '') -> list[tuple[Entry, Consumer]]:
        return [
            (entry, consumer)
            for entry in self.entries
            for consumer in entry.consumers
            if consumer.kind is kind and consumer.chart == chart
        ]

    def helm_consumers(self, chart: str) -> list[tuple[Entry, Consumer]]:
        return self.consumers(Kind.SPLIT, chart) + self.consumers(Kind.STRING, chart)

    def by_source(self) -> dict[str, Entry]:
        return {entry.source: entry for entry in self.entries}


@dataclass(frozen=True)
class Reference:
    host: str
    path: str
    tag: str | None
    digest: str | None

    @classmethod
    def parse(cls, text: str) -> 'Reference':
        name, _, digest = text.partition('@')
        tag = None
        if name.rfind(':') > name.rfind('/'):
            name, _, tag = name.rpartition(':')
        first, _, rest = name.partition('/')
        if rest and ('.' in first or ':' in first or first == 'localhost'):
            return cls(first, rest, tag, digest or None)
        return cls('', name, tag, digest or None)

    @property
    def target(self) -> str:
        if self.host in DOCKER_HUB and '/' not in self.path:
            return f'library/{self.path}'
        return self.path


@dataclass(frozen=True)
class ComposeService:
    image: str
    built: bool
    profiles: tuple[str, ...]


@dataclass(frozen=True)
class HelmRender:
    chart: str
    label: str
    default: bool
    pairs: tuple[tuple[str, str], ...]
    consumers: tuple[tuple[Entry, Consumer], ...]


@dataclass(frozen=True)
class OperatorInput:
    chart: str
    value_set: str
    key: str
    value: object


OPERATOR_INPUTS = (
    OperatorInput('zone-apps', 'values-ha.yaml', 'highAvailability.externalValkey', True),
    OperatorInput('zone-apps', 'values-ha.yaml', 'server.agentState.persistence.storageClass', 'test-rwx'),
    OperatorInput('zone-apps', 'values-ha.yaml', 'server.trustedProxies', '10.244.0.0/16'),
    OperatorInput('zone-infra', 'values-ha.yaml', 'postgres.backup.objectStore', 'zone-backups'),
)


@dataclass
class Comparison:
    problems: list[str] = field(default_factory=list)
    used: set[str] = field(default_factory=set)


def source_problems(source: str) -> list[str]:
    if source.startswith('zone:'):
        if ZONE_SOURCE.fullmatch(source):
            return []
        return [f'source {source} is not zone:server, zone:manager or zone:litellm']
    reference = Reference.parse(source)
    problems = []
    if not REPOSITORY_PATH.fullmatch(reference.path):
        problems.append(f'source {source} has an invalid repository')
    if reference.tag is None:
        problems.append(f'source {source} has no tag')
    elif not TAG.fullmatch(reference.tag):
        problems.append(f'source {source} has an invalid tag')
    if reference.digest is None:
        problems.append(f'source {source} is not pinned by a sha256 digest')
    elif not DIGEST.fullmatch(reference.digest):
        problems.append(f'source {source} has a malformed digest')
    return problems


def expected_target(source: str) -> str:
    zone = ZONE_SOURCE.fullmatch(source)
    if zone:
        return f'zone/{zone.group("image")}'
    return Reference.parse(source).target


def parse_consumers(text: str) -> tuple[list[Consumer], list[str]]:
    consumers = []
    problems = []
    for item in text.split(','):
        compose = COMPOSE_CONSUMER.fullmatch(item)
        helm = HELM_CONSUMER.fullmatch(item)
        if compose:
            consumers.append(Consumer(Kind.COMPOSE, '', compose.group('key')))
        elif helm:
            consumers.append(Consumer(Kind(helm.group('kind')), helm.group('chart'), helm.group('key')))
        else:
            problems.append(f'consumer {item!r} is not compose:<service>, helm-split:<chart>:<path> or helm-string:<chart>:<path>')
    return consumers, problems


def parse(text: str) -> Listing:
    entries = []
    exclusions: dict[str, str] = {}
    problems = []
    names: set[str] = set()
    sources: set[str] = set()
    claimed: dict[Consumer, str] = {}
    for number, line in enumerate(text.splitlines(), start=1):
        if not line.strip():
            continue
        prefix = f'line {number}'
        if line[0] in ' \t':
            problems.append(f'{prefix}: starts with whitespace')
            continue
        if line.startswith(EXCLUSION_PREFIX):
            exclusion = EXCLUSION.fullmatch(line.rstrip())
            if exclusion is None:
                problems.append(f'{prefix}: an exclusion is #excluded <compose service> <reason words>')
            elif exclusion.group('service') in exclusions:
                problems.append(f'{prefix}: {exclusion.group("service")} is excluded twice')
            else:
                exclusions[exclusion.group('service')] = exclusion.group('reason')
            continue
        if line.startswith('#'):
            continue
        fields = line.split()
        if len(fields) != 4:
            problems.append(f'{prefix}: has {len(fields)} fields, not <name> <source> <target> <consumers>')
            continue
        name, source, target, consumer_text = fields
        line_problems = []
        if not NAME.fullmatch(name):
            line_problems.append(f'name {name} does not match [a-z0-9-]+')
        elif name in names:
            line_problems.append(f'name {name} is listed twice')
        names.add(name)
        if source in sources:
            line_problems.append(f'source {source} is listed twice')
        sources.add(source)
        source_issues = source_problems(source)
        line_problems.extend(source_issues)
        if not TARGET.fullmatch(target):
            line_problems.append(f'target {target} does not match [a-z0-9._/-]+')
        elif not source_issues and target != expected_target(source):
            line_problems.append(f'target {target} should be {expected_target(source)} for {source}')
        consumers, consumer_issues = parse_consumers(consumer_text)
        line_problems.extend(consumer_issues)
        for consumer in consumers:
            if consumer in claimed:
                line_problems.append(f'consumer {consumer} is already listed by {claimed[consumer]}')
            claimed[consumer] = name
        problems.extend(f'{prefix}: {problem}' for problem in line_problems)
        entries.append(Entry(number, name, source, target, tuple(consumers)))
    return Listing(tuple(entries), exclusions, tuple(problems))


def compose_source(service: ComposeService) -> str | None:
    zone = ZONE_COMPOSE_IMAGES.get(Reference.parse(service.image).path)
    if zone is not None:
        return zone
    if service.built:
        return None
    return service.image


def compare_compose(listing: Listing, services: Mapping[str, ComposeService]) -> Comparison:
    comparison = Comparison()
    listed = {consumer.key: entry for entry, consumer in listing.consumers(Kind.COMPOSE)}
    for name, service in sorted(services.items()):
        if name in listing.exclusions:
            comparison.problems.append(f'compose service {name} is excluded but renders with the bundle profiles')
            continue
        source = compose_source(service)
        entry = listed.get(name)
        if source is None:
            comparison.problems.append(f'compose service {name} builds {service.image or "an unnamed image"}, which maps to no Zone image')
        elif entry is None:
            comparison.problems.append(f'compose service {name} renders {service.image}, which no entry lists as compose:{name}')
        elif entry.source != source:
            comparison.problems.append(f'compose service {name} renders {service.image}, but entry {entry.name} has {entry.source}')
        else:
            comparison.used.add(entry.name)
    for name, entry in sorted(listed.items()):
        if name not in services:
            comparison.problems.append(f'entry {entry.name} lists compose:{name}, which does not render with the bundle profiles')
    return comparison


def stale_exclusions(listing: Listing, services: Mapping[str, ComposeService]) -> list[str]:
    return [
        f'exclusion {name} names no compose service'
        for name in sorted(listing.exclusions)
        if name not in services
    ]


def bundle_profiles(listing: Listing, services: Mapping[str, ComposeService]) -> tuple[str, ...]:
    profiles = {profile for service in services.values() for profile in service.profiles}
    excluded = {
        profile
        for profile in profiles
        if all(name in listing.exclusions for name, service in services.items() if profile in service.profiles)
    }
    return tuple(sorted(profiles - excluded))


def lookup(values: object, key: str) -> object:
    node = values
    for part in key.split('.'):
        if not isinstance(node, dict) or part not in node:
            return MISSING
        node = node[part]
    return node


def assign(values: dict, key: str, value: object) -> None:
    *parents, last = key.split('.')
    node = values
    for part in parents:
        node = node.setdefault(part, {})
    node[last] = value


def merge(base: dict, override: dict) -> dict:
    merged = dict(base)
    for key, value in override.items():
        if value is None:
            merged.pop(key, None)
        elif isinstance(value, dict) and isinstance(merged.get(key), dict):
            merged[key] = merge(merged[key], value)
        else:
            merged[key] = value
    return merged


def consumer_path_problems(listing: Listing, defaults: Mapping[str, dict]) -> list[str]:
    problems = []
    for kind in (Kind.SPLIT, Kind.STRING):
        for entry in listing.entries:
            for consumer in entry.consumers:
                if consumer.kind is not kind:
                    continue
                if consumer.chart not in defaults:
                    problems.append(f'entry {entry.name} lists {consumer}, but helm/{consumer.chart}/Chart.yaml does not exist')
                    continue
                node = lookup(defaults[consumer.chart], consumer.key)
                if node is MISSING:
                    problems.append(f'entry {entry.name} lists {consumer}, but {consumer.key} is not in helm/{consumer.chart}/values.yaml')
                elif kind is Kind.SPLIT and not (isinstance(node, dict) and isinstance(node.get('repository'), str) and 'tag' in node):
                    problems.append(f'entry {entry.name} lists {consumer}, but {consumer.key} has no repository and tag')
                elif kind is Kind.STRING and not isinstance(node, str):
                    problems.append(f'entry {entry.name} lists {consumer}, but {consumer.key} is not a string')
    return problems


def operator_values(inputs: tuple[OperatorInput, ...], chart: str, value_set: str) -> dict:
    values: dict = {}
    for required in inputs:
        if (required.chart, required.value_set) == (chart, value_set):
            assign(values, required.key, required.value)
    return values


def operator_input_problems(inputs: tuple[OperatorInput, ...], defaults: Mapping[str, dict]) -> list[str]:
    problems = []
    for required in inputs:
        if not (CHARTS / required.chart / required.value_set).is_file():
            problems.append(f'operator input {required.key} names helm/{required.chart}/{required.value_set}, which does not exist')
        elif lookup(defaults.get(required.chart, {}), required.key) is MISSING:
            problems.append(f'operator input {required.key} is not in helm/{required.chart}/values.yaml')
    return problems


def sentinel_values(consumers: tuple[tuple[Entry, Consumer], ...], values: dict) -> dict:
    override: dict = {}
    for index, (_, consumer) in enumerate(consumers):
        repository = f'{SENTINEL_HOST}/consumer-{index}'
        if consumer.kind is Kind.STRING:
            assign(override, consumer.key, f'{repository}:{SENTINEL_TAG}')
            continue
        assign(override, f'{consumer.key}.repository', repository)
        assign(override, f'{consumer.key}.tag', SENTINEL_TAG)
        if lookup(values, f'{consumer.key}.digest') is not MISSING:
            assign(override, f'{consumer.key}.digest', '')
    return override


def helm_source(image: str) -> str:
    return ZONE_HELM_REPOSITORIES.get(Reference.parse(image).path, image)


def compare_helm(listing: Listing, render: HelmRender) -> Comparison:
    comparison = Comparison()
    where = f'{render.chart} ({render.label})'
    listed = listing.by_source()
    for image, sentinel in render.pairs:
        controlled = SENTINEL.search(sentinel)
        if controlled:
            entry, consumer = render.consumers[int(controlled.group('index'))]
            comparison.used.add(entry.name)
            source = helm_source(image)
            if (render.default or not entry.zone) and source != entry.source:
                comparison.problems.append(f'{where} renders {image} through {consumer}, but entry {entry.name} has {entry.source}')
        elif image != sentinel:
            comparison.problems.append(f'{where} renders {image}, which changes to {sentinel} although no consumer sets it')
        elif image in listed:
            comparison.used.add(listed[image].name)
        else:
            comparison.problems.append(f'{where} renders {image}, which no entry lists')
    return comparison


def unused_entries(listing: Listing, used: set[str]) -> list[str]:
    return [
        f'entry {entry.name} on line {entry.line} is used by no rendered image'
        for entry in listing.entries
        if entry.name not in used
    ]


def idle_consumers(listing: Listing, renders: list[HelmRender]) -> list[str]:
    controlling = set()
    for render in renders:
        for _, sentinel in render.pairs:
            controlled = SENTINEL.search(sentinel)
            if controlled:
                controlling.add(render.consumers[int(controlled.group('index'))][1])
    return [
        f'entry {entry.name} lists {consumer}, but setting it changes no image helm/{consumer.chart} renders'
        for entry in listing.entries
        for consumer in entry.consumers
        if consumer.kind is not Kind.COMPOSE and consumer not in controlling
    ]


def images(node: object) -> Iterator[str]:
    if isinstance(node, dict):
        for key, value in node.items():
            if key in IMAGE_KEYS and isinstance(value, str):
                yield value
            else:
                yield from images(value)
    elif isinstance(node, list):
        for value in node:
            yield from images(value)


def isolated_environment() -> dict[str, str]:
    return {name: os.environ[name] for name in PASSED_VARIABLES if name in os.environ}


def run(command: list[str]) -> str:
    result = subprocess.run(
        command, cwd=ROOT, env=isolated_environment(), capture_output=True, text=True, timeout=RENDER_TIMEOUT,
    )
    if result.returncode != 0:
        raise AssertionError(f'{" ".join(command)} exited {result.returncode}:\n{result.stderr}')
    return result.stdout


def require(*command: str) -> None:
    try:
        available = shutil.which(command[0]) is not None and subprocess.run(
            list(command), capture_output=True, timeout=60,
        ).returncode == 0
    except subprocess.TimeoutExpired:
        available = False
    if available:
        return
    if os.environ.get('CI'):
        raise AssertionError(f'{" ".join(command)} must work in CI, or the drift check proves nothing')
    raise unittest.SkipTest(f'{" ".join(command)} is not available')


def compose(*arguments: str) -> str:
    return run(['docker', 'compose', '--env-file', os.devnull, '--project-directory', str(ROOT), '-f', str(COMPOSE), *arguments])


@cache
def listing() -> Listing:
    return parse(LISTING.read_text())


@cache
def compose_services(profiles: tuple[str, ...]) -> dict[str, ComposeService]:
    flags = [argument for profile in profiles for argument in ('--profile', profile)]
    rendered = json.loads(compose(*flags, 'config', '--format', 'json'))['services']
    return {
        name: ComposeService(service.get('image', ''), 'build' in service, tuple(service.get('profiles', ())))
        for name, service in rendered.items()
    }


@cache
def every_compose_service() -> dict[str, ComposeService]:
    return compose_services(tuple(compose('config', '--profiles').split()))


@cache
def bundle_compose_services() -> dict[str, ComposeService]:
    return compose_services(bundle_profiles(listing(), every_compose_service()))


@cache
def charts() -> tuple[Path, ...]:
    return tuple(sorted(path.parent for path in CHARTS.glob('*/Chart.yaml')))


def value_sets(chart: Path) -> tuple[Path | None, ...]:
    return (None, *sorted({path for pattern in VALUE_SETS for path in chart.glob(pattern)}))


def chart_defaults() -> dict[str, dict]:
    return {chart.name: yaml.safe_load((chart / 'values.yaml').read_text()) or {} for chart in charts()}


def helm_images(chart: Path, files: tuple[Path, ...]) -> list[str]:
    flags = [argument for path in files for argument in ('-f', str(path))]
    output = run(['helm', 'template', chart.name, str(chart), '--namespace', NAMESPACE, *flags])
    return list(images([document for document in yaml.safe_load_all(output) if document]))


@cache
def helm_render(chart: Path, value_set: Path | None) -> HelmRender:
    label = DEFAULT_VALUES if value_set is None else str(value_set.relative_to(chart))
    required = operator_values(OPERATOR_INPUTS, chart.name, label)
    with TemporaryDirectory() as folder:
        files: tuple[Path, ...] = () if value_set is None else (value_set,)
        if required:
            inputs = Path(folder) / 'operator.yaml'
            inputs.write_text(yaml.safe_dump(required))
            files = (*files, inputs)
        values = chart_defaults()[chart.name]
        for path in files:
            values = merge(values, yaml.safe_load(path.read_text()) or {})
        consumers = tuple(listing().helm_consumers(chart.name))
        override = Path(folder) / 'sentinel.yaml'
        override.write_text(yaml.safe_dump(sentinel_values(consumers, values)))
        sentinels = helm_images(chart, (*files, override))
        rendered = helm_images(chart, files)
    if len(rendered) != len(sentinels):
        raise AssertionError(f'{chart.name}: setting every consumer changed the number of rendered images')
    return HelmRender(chart.name, label, value_set is None, tuple(zip(rendered, sentinels)), consumers)


def every_helm_render() -> list[HelmRender]:
    return [helm_render(chart, value_set) for chart in charts() for value_set in value_sets(chart)]


VALID_SOURCE = 'traefik:v3.7.12@sha256:' + 'a' * 64
VALID = f'traefik {VALID_SOURCE} library/traefik compose:traefik'


def problems_of(text: str) -> str:
    return '\n'.join(parse(text).problems)


class ListingTests(unittest.TestCase):
    def test_listing_follows_the_grammar(self) -> None:
        self.assertEqual(listing().problems, ())
        self.assertTrue(listing().entries)

    def test_comfyui_services_are_excluded(self) -> None:
        self.assertEqual(sorted(listing().exclusions), ['comfyui', 'comfyui-model-setup'])

    def test_zone_images_are_the_lts_repositories(self) -> None:
        zone = {entry.source: entry.target for entry in listing().entries if entry.zone}
        self.assertEqual(zone, {'zone:server': 'zone/server', 'zone:manager': 'zone/manager', 'zone:litellm': 'zone/litellm'})

    def test_compose_maps_zone_services_to_the_contract(self) -> None:
        sources = {consumer.key: entry.source for entry, consumer in listing().consumers(Kind.COMPOSE) if entry.zone}
        self.assertEqual(sources, {'manager': 'zone:server', 'console': 'zone:manager', 'litellm': 'zone:litellm'})


class GrammarTests(unittest.TestCase):
    def test_valid_line_passes(self) -> None:
        parsed = parse(VALID)
        self.assertEqual(parsed.problems, ())
        self.assertEqual(parsed.entries[0].consumers, (Consumer(Kind.COMPOSE, '', 'traefik'),))

    def test_tabs_and_runs_of_spaces_separate_fields(self) -> None:
        self.assertEqual(parse(f'traefik\t\t{VALID_SOURCE}   library/traefik \tcompose:traefik').problems, ())

    def test_comments_and_blank_lines_are_ignored(self) -> None:
        parsed = parse(f'# a comment\n\n   \n{VALID}\n')
        self.assertEqual(parsed.problems, ())
        self.assertEqual(len(parsed.entries), 1)

    def test_all_consumer_kinds_parse(self) -> None:
        line = f'traefik {VALID_SOURCE} library/traefik compose:traefik,helm-split:zone-apps:server.image,helm-string:zone-infra:postgres.image'
        parsed = parse(line)
        self.assertEqual(parsed.problems, ())
        self.assertEqual(
            [str(consumer) for consumer in parsed.entries[0].consumers],
            ['compose:traefik', 'helm-split:zone-apps:server.image', 'helm-string:zone-infra:postgres.image'],
        )

    def test_untagged_source_is_refused(self) -> None:
        self.assertIn('has no tag', problems_of(f'traefik traefik@sha256:{"a" * 64} library/traefik compose:traefik'))

    def test_registry_port_is_not_a_tag(self) -> None:
        self.assertIn('has no tag', problems_of(f'traefik localhost:5000/traefik@sha256:{"a" * 64} traefik compose:traefik'))

    def test_undigested_source_is_refused(self) -> None:
        self.assertIn('not pinned by a sha256 digest', problems_of('traefik traefik:v3.7.12 library/traefik compose:traefik'))

    def test_short_digest_is_refused(self) -> None:
        self.assertIn('malformed digest', problems_of('traefik traefik:v3.7.12@sha256:abc library/traefik compose:traefik'))

    def test_unknown_zone_source_is_refused(self) -> None:
        self.assertIn('is not zone:server', problems_of('console zone:console zone/console compose:console'))

    def test_bad_name_is_refused(self) -> None:
        self.assertIn('does not match [a-z0-9-]+', problems_of(VALID.replace('traefik ', 'Traefik_ ', 1)))

    def test_duplicate_name_is_refused(self) -> None:
        other = f'traefik {VALID_SOURCE.replace("a" * 64, "b" * 64)} library/traefik compose:edge'
        self.assertIn('name traefik is listed twice', problems_of(f'{VALID}\n{other}'))

    def test_duplicate_source_is_refused(self) -> None:
        other = f'edge {VALID_SOURCE} library/traefik compose:edge'
        self.assertIn('is listed twice', problems_of(f'{VALID}\n{other}'))

    def test_wrong_field_count_is_refused(self) -> None:
        self.assertIn('has 5 fields', problems_of(f'{VALID} extra'))
        self.assertIn('has 5 fields', problems_of(VALID.replace('compose:traefik', 'compose:traefik, compose:edge')))

    def test_bad_target_is_refused(self) -> None:
        self.assertIn('does not match [a-z0-9._/-]+', problems_of(VALID.replace('library/traefik', 'Library/Traefik')))

    def test_target_must_follow_the_source(self) -> None:
        self.assertIn('should be library/traefik', problems_of(VALID.replace('library/traefik', 'traefik')))
        self.assertIn('should be zone/server', problems_of('server zone:server zone/manager compose:manager'))

    def test_bad_consumer_is_refused(self) -> None:
        for consumers in ('kustomize:traefik', 'compose:traefik,', 'helm-split:zone-apps', 'helm-string:zone-apps:server..image'):
            with self.subTest(consumers=consumers):
                self.assertIn('is not compose:<service>', problems_of(VALID.replace('compose:traefik', consumers)))

    def test_consumer_listed_twice_is_refused(self) -> None:
        other = f'edge {VALID_SOURCE.replace("a" * 64, "b" * 64)} library/traefik compose:traefik'
        self.assertIn('compose:traefik is already listed by traefik', problems_of(f'{VALID}\n{other}'))

    def test_exclusion_needs_a_service_and_a_reason(self) -> None:
        for line in ('#excluded', '#excluded comfyui', '#excludedcomfyui needs a GPU'):
            with self.subTest(line=line):
                self.assertIn('an exclusion is #excluded', problems_of(line))

    def test_exclusion_is_parsed(self) -> None:
        self.assertEqual(dict(parse('#excluded comfyui needs a GPU').exclusions), {'comfyui': 'needs a GPU'})

    def test_exclusion_listed_twice_is_refused(self) -> None:
        self.assertIn('excluded twice', problems_of('#excluded comfyui needs a GPU\n#excluded comfyui again'))

    def test_leading_whitespace_is_refused(self) -> None:
        self.assertIn('starts with whitespace', problems_of(f' {VALID}'))
        self.assertIn('starts with whitespace', problems_of('\t#excluded comfyui needs a GPU'))


class ReferenceTests(unittest.TestCase):
    def test_docker_hub_names_get_the_library_namespace(self) -> None:
        self.assertEqual(Reference.parse(VALID_SOURCE).target, 'library/traefik')
        self.assertEqual(Reference.parse('postgres:15-alpine').target, 'library/postgres')

    def test_registry_hosts_are_dropped_from_the_target(self) -> None:
        self.assertEqual(Reference.parse('gcr.io/cadvisor/cadvisor:v0.52.1').target, 'cadvisor/cadvisor')
        self.assertEqual(Reference.parse('localhost:5000/zone/server:1.4.3-lts').target, 'zone/server')
        self.assertEqual(Reference.parse('prom/prometheus:v3.14.0').target, 'prom/prometheus')

    def test_reference_parts(self) -> None:
        self.assertEqual(
            Reference.parse(f'ghcr.io/berriai/litellm:v1.99.1@sha256:{"c" * 64}'),
            Reference('ghcr.io', 'berriai/litellm', 'v1.99.1', f'sha256:{"c" * 64}'),
        )


class ComposeComparisonTests(unittest.TestCase):
    LISTING = parse('\n'.join([
        VALID,
        'zone-server zone:server zone/server compose:manager',
        '#excluded comfyui needs a GPU',
    ]))

    def compare(self, **services: ComposeService) -> Comparison:
        rendered = {
            'traefik': ComposeService(VALID_SOURCE, False, ()),
            'manager': ComposeService('zone-manager:prod', True, ()),
            **services,
        }
        return compare_compose(self.LISTING, {name: service for name, service in rendered.items() if service.image != 'absent'})

    def test_matching_services_pass_and_use_their_entries(self) -> None:
        comparison = self.compare()
        self.assertEqual(comparison.problems, [])
        self.assertEqual(comparison.used, {'traefik', 'zone-server'})

    def test_missing_image_is_reported(self) -> None:
        comparison = self.compare(valkey=ComposeService(f'valkey/valkey:8.1.10-alpine@sha256:{"d" * 64}', False, ()))
        self.assertIn('compose service valkey renders valkey/valkey', '\n'.join(comparison.problems))

    def test_drifted_digest_is_reported(self) -> None:
        comparison = self.compare(traefik=ComposeService(VALID_SOURCE.replace('a' * 64, 'e' * 64), False, ()))
        self.assertIn('but entry traefik has', '\n'.join(comparison.problems))

    def test_consumer_without_a_service_is_reported(self) -> None:
        comparison = self.compare(traefik=ComposeService('absent', False, ()))
        self.assertIn('lists compose:traefik, which does not render', '\n'.join(comparison.problems))

    def test_unmapped_build_is_reported(self) -> None:
        comparison = self.compare(worker=ComposeService('zone-worker:prod', True, ()))
        self.assertIn('builds zone-worker:prod, which maps to no Zone image', '\n'.join(comparison.problems))

    def test_rendered_exclusion_is_reported(self) -> None:
        comparison = self.compare(comfyui=ComposeService('zone-comfyui:1', True, ('bundled-comfyui',)))
        self.assertIn('compose service comfyui is excluded', '\n'.join(comparison.problems))

    def test_bundle_profiles_drop_only_fully_excluded_profiles(self) -> None:
        services = {
            'comfyui': ComposeService('zone-comfyui:1', True, ('bundled-comfyui', 'shared')),
            'grafana': ComposeService('grafana/grafana:1', False, ('monitoring', 'shared')),
        }
        self.assertEqual(bundle_profiles(self.LISTING, services), ('monitoring', 'shared'))

    def test_stale_exclusion_is_reported(self) -> None:
        self.assertEqual(stale_exclusions(self.LISTING, {}), ['exclusion comfyui names no compose service'])


class HelmComparisonTests(unittest.TestCase):
    LISTING = parse('\n'.join([
        'zone-server zone:server zone/server helm-split:zone-apps:server.image',
        f'postgres postgres:15-alpine@sha256:{"f" * 64} library/postgres helm-string:zone-apps:initContainers.postgres.image',
        f'curl curlimages/curl:8.22.0@sha256:{"1" * 64} curlimages/curl compose:curl',
    ]))
    CONSUMERS = tuple(LISTING.helm_consumers('zone-apps'))
    POSTGRES = f'postgres:15-alpine@sha256:{"f" * 64}'

    def compare(self, *pairs: tuple[str, str], default: bool = True) -> Comparison:
        return compare_helm(self.LISTING, HelmRender('zone-apps', 'test', default, pairs, self.CONSUMERS))

    def sentinel(self, index: int) -> str:
        return f'{SENTINEL_HOST}/consumer-{index}:{SENTINEL_TAG}'

    def test_controlled_images_matching_their_source_pass(self) -> None:
        comparison = self.compare(('zone/server:latest', self.sentinel(0)), (self.POSTGRES, self.sentinel(1)))
        self.assertEqual(comparison.problems, [])
        self.assertEqual(comparison.used, {'zone-server', 'postgres'})

    def test_registry_prefixed_sentinel_is_recognised(self) -> None:
        comparison = self.compare(('registry.example/zone/server:latest', f'registry.example/{self.sentinel(0)}'))
        self.assertEqual(comparison.problems, [])

    def test_drifted_default_image_is_reported(self) -> None:
        comparison = self.compare((self.POSTGRES.replace('f' * 64, '2' * 64), self.sentinel(1)))
        self.assertIn('through helm-string:zone-apps:initContainers.postgres.image', '\n'.join(comparison.problems))

    def test_example_may_rename_zone_images(self) -> None:
        self.assertEqual(self.compare(('gcr.io/my-project/zone-apps:1.0.0', self.sentinel(0)), default=False).problems, [])
        self.assertTrue(self.compare(('gcr.io/my-project/zone-apps:1.0.0', self.sentinel(0))).problems)

    def test_example_may_not_change_third_party_images(self) -> None:
        self.assertTrue(self.compare(('postgres:16-alpine', self.sentinel(1)), default=False).problems)

    def test_unlisted_fixed_image_is_reported(self) -> None:
        image = f'busybox:1.37@sha256:{"3" * 64}'
        self.assertIn(f'renders {image}, which no entry lists', '\n'.join(self.compare((image, image)).problems))

    def test_listed_fixed_image_is_bundled(self) -> None:
        image = f'curlimages/curl:8.22.0@sha256:{"1" * 64}'
        comparison = self.compare((image, image))
        self.assertEqual(comparison.problems, [])
        self.assertEqual(comparison.used, {'curl'})

    def test_unused_entries_are_reported(self) -> None:
        self.assertEqual(
            unused_entries(self.LISTING, {'zone-server', 'curl'}),
            ['entry postgres on line 2 is used by no rendered image'],
        )

    def test_consumer_that_sets_no_rendered_image_is_reported(self) -> None:
        render = HelmRender('zone-apps', 'test', True, (('zone/server:latest', self.sentinel(0)), (self.POSTGRES, self.POSTGRES)), self.CONSUMERS)
        self.assertEqual(
            idle_consumers(self.LISTING, [render]),
            ['entry postgres lists helm-string:zone-apps:initContainers.postgres.image, '
             'but setting it changes no image helm/zone-apps renders'],
        )

    def test_consumer_set_in_any_render_is_not_idle(self) -> None:
        renders = [
            HelmRender('zone-apps', 'test', True, (('zone/server:latest', self.sentinel(0)),), self.CONSUMERS),
            HelmRender('zone-apps', 'example', False, ((self.POSTGRES, self.sentinel(1)),), self.CONSUMERS),
        ]
        self.assertEqual(idle_consumers(self.LISTING, renders), [])

    def test_missing_consumer_path_is_reported(self) -> None:
        defaults = {'zone-apps': {'server': {'image': {'repository': 'zone/server', 'tag': 'latest'}}}}
        self.assertEqual(
            consumer_path_problems(self.LISTING, defaults),
            ['entry postgres lists helm-string:zone-apps:initContainers.postgres.image, '
             'but initContainers.postgres.image is not in helm/zone-apps/values.yaml'],
        )

    def test_consumer_path_of_the_wrong_shape_is_reported(self) -> None:
        defaults = {'zone-apps': {'server': {'image': 'zone/server:latest'}, 'initContainers': {'postgres': {'image': {'repository': 'postgres'}}}}}
        self.assertEqual(
            consumer_path_problems(self.LISTING, defaults),
            ['entry zone-server lists helm-split:zone-apps:server.image, but server.image has no repository and tag',
             'entry postgres lists helm-string:zone-apps:initContainers.postgres.image, but initContainers.postgres.image is not a string'],
        )

    def test_unknown_chart_is_reported(self) -> None:
        self.assertIn('helm/zone-apps/Chart.yaml does not exist', '\n'.join(consumer_path_problems(self.LISTING, {})))

    def test_sentinel_values_override_every_consumer(self) -> None:
        values = {'server': {'image': {'repository': 'zone/server', 'tag': 'latest', 'digest': f'sha256:{"4" * 64}'}}}
        self.assertEqual(sentinel_values(self.CONSUMERS, values), {
            'server': {'image': {'repository': f'{SENTINEL_HOST}/consumer-0', 'tag': SENTINEL_TAG, 'digest': ''}},
            'initContainers': {'postgres': {'image': self.sentinel(1)}},
        })

    def test_operator_values_layer_only_the_matching_value_set(self) -> None:
        inputs = (
            OperatorInput('zone-apps', 'values-ha.yaml', 'server.trustedProxies', '10.244.0.0/16'),
            OperatorInput('zone-apps', 'values-ha.yaml', 'highAvailability.externalValkey', True),
            OperatorInput('zone-infra', 'values-ha.yaml', 'postgres.backup.objectStore', 'zone-backups'),
        )
        self.assertEqual(
            operator_values(inputs, 'zone-apps', 'values-ha.yaml'),
            {'server': {'trustedProxies': '10.244.0.0/16'}, 'highAvailability': {'externalValkey': True}},
        )
        self.assertEqual(operator_values(inputs, 'zone-apps', DEFAULT_VALUES), {})

    def test_stale_operator_input_is_reported(self) -> None:
        defaults = {'zone-apps': {'server': {'trustedProxies': ''}}}
        problems = operator_input_problems((
            OperatorInput('zone-apps', 'values-ha.yaml', 'server.trustedProxies', '10.244.0.0/16'),
            OperatorInput('zone-apps', 'values-ha.yaml', 'server.renamed', True),
            OperatorInput('zone-apps', 'values-gone.yaml', 'server.trustedProxies', '10.244.0.0/16'),
        ), defaults)
        self.assertEqual(problems, [
            'operator input server.renamed is not in helm/zone-apps/values.yaml',
            'operator input server.trustedProxies names helm/zone-apps/values-gone.yaml, which does not exist',
        ])

    def test_operator_inputs_name_real_value_sets_and_keys(self) -> None:
        self.assertEqual(operator_input_problems(OPERATOR_INPUTS, chart_defaults()), [])

    def test_merge_follows_helm(self) -> None:
        base = {'server': {'image': {'repository': 'zone/server', 'tag': 'latest'}, 'replicas': 1}}
        override = {'server': {'image': {'tag': '1.0.0'}, 'replicas': None}}
        self.assertEqual(merge(base, override), {'server': {'image': {'repository': 'zone/server', 'tag': '1.0.0'}}})


class ComposeRenderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        require('docker', 'compose', 'version')

    def test_exclusions_name_compose_services(self) -> None:
        self.assertEqual(stale_exclusions(listing(), every_compose_service()), [])

    def test_bundle_profiles_leave_out_only_excluded_services(self) -> None:
        skipped = set(every_compose_service()) - set(bundle_compose_services())
        self.assertEqual(skipped, set(listing().exclusions))

    def test_every_rendered_image_is_listed(self) -> None:
        self.assertEqual(compare_compose(listing(), bundle_compose_services()).problems, [])


class HelmRenderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        require('helm', 'version', '--short')

    def test_charts_and_value_sets_are_discovered(self) -> None:
        names = {chart.name for chart in charts()}
        self.assertLessEqual({'zone-ai', 'zone-apps', 'zone-infra'}, names)
        examples = {path.name for path in value_sets(CHARTS / 'zone-apps') if path is not None}
        self.assertLessEqual({'minimal.yaml', 'production.yaml'}, examples)

    def test_consumer_paths_exist(self) -> None:
        self.assertEqual(consumer_path_problems(listing(), chart_defaults()), [])

    def test_high_availability_profile_renders(self) -> None:
        chart = CHARTS / 'zone-apps'
        profile = chart / 'values-ha.yaml'
        self.assertIn(profile, value_sets(chart))
        render = helm_render(chart, profile)
        self.assertEqual((render.chart, render.label), ('zone-apps', 'values-ha.yaml'))
        self.assertTrue(render.pairs, 'zone-apps (values-ha.yaml) renders no images')
        self.assertIn(render, every_helm_render())

    def test_every_rendered_image_is_listed(self) -> None:
        for render in every_helm_render():
            with self.subTest(chart=render.chart, values=render.label):
                self.assertTrue(render.pairs, f'{render.chart} ({render.label}) renders no images')
                self.assertEqual(compare_helm(listing(), render).problems, [])

    def test_every_helm_consumer_sets_a_rendered_image(self) -> None:
        self.assertEqual(idle_consumers(listing(), every_helm_render()), [])


class CoverageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        require('docker', 'compose', 'version')
        require('helm', 'version', '--short')

    def test_every_entry_is_used(self) -> None:
        used = set(compare_compose(listing(), bundle_compose_services()).used)
        for render in every_helm_render():
            used |= compare_helm(listing(), render).used
        self.assertEqual(unused_entries(listing(), used), [])


if __name__ == '__main__':
    unittest.main()
