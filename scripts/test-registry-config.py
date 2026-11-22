#!/usr/bin/env python3
"""Check the distribution registry configs and the signer's Caddyfiles with PyYAML, and with the pinned registry and Caddy images when Docker is available."""

import ipaddress
import json
import re
import shlex
import shutil
import subprocess
import time
import unittest
import urllib.error
import urllib.request
from collections.abc import Iterator
from contextlib import AbstractContextManager, contextmanager
from dataclasses import dataclass, field
from pathlib import Path
from tempfile import TemporaryDirectory

import yaml

ROOT = Path(__file__).resolve().parents[1]
DEPLOY = ROOT / 'runner' / 'zone_license' / 'deploy'
REGISTRY_CONFIG = DEPLOY / 'registry' / 'config.yml'
PUBLISH_CONFIG = DEPLOY / 'registry' / 'publish.yml'
CADDYFILE = DEPLOY / 'Caddyfile'
CURRENT_CADDYFILE = DEPLOY / 'Caddyfile.current'
CADDYFILE_DIFF = DEPLOY / 'Caddyfile.diff'

REGISTRY_IMAGE = 'registry:3.1.1@sha256:325b4b29b041e82803abeb703e201655e4e23ab83264ec1a7c9ddb0a5b14a6e0'
CADDY_IMAGE = 'caddy:2.11.9@sha256:4851e54a46b206517c574821bb5cc67f170d2156041e9fbc4c9193b45e84a75b'

SIGNER_SITE = 'licenses.bitecode.ltd'
REGISTRY_SITE = 'registry.bitecode.ltd'
SIGNER = 'zone-license:8080'
REGISTRY = 'registry:5000'
SIGNER_PATHS = ('/webhooks/polar', '/licenses/renew', '/health')
METHODS = ('GET', 'HEAD', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS')
PULL_METHODS = ('GET', 'HEAD')
TOKEN_METHODS = ('GET', 'HEAD', 'POST')
BUNDLE_METHODS = ('GET', 'HEAD')
TOKEN_PATH = '/token'
BUNDLE_PATH = '/bundles/1.4.3-lts/amd64'
REGISTRY_PATHS = ('/v2/', '/v2/zone/server/manifests/1.4-lts', '/v2/zone/server/blobs/uploads/', '/v2/_catalog')
UNKNOWN_PATHS = ('/', '/metrics', '/tokens', '/bundles', '/v2')

TOKEN_AUTH = {
    'realm': 'https://licenses.bitecode.ltd/token',
    'service': REGISTRY_SITE,
    'issuer': 'zone-license',
    'jwks': '/etc/zone-registry/jwks.json',
    'signingalgorithms': ['EdDSA'],
}
CHALLENGE = 'Bearer realm="https://licenses.bitecode.ltd/token",service="registry.bitecode.ltd"'
CREDENTIAL_KEYS = ('accesskey', 'secretkey', 'sessiontoken')
JWKS = {
    'keys': [
        {
            'kty': 'OKP',
            'crv': 'Ed25519',
            'alg': 'EdDSA',
            'use': 'sig',
            'kid': 'registry-test-1-1',
            'x': '11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo',
        }
    ]
}
STORAGE_ENVIRONMENT = ('REGISTRY_STORAGE_S3_BUCKET=zone-distribution-check', 'REGISTRY_STORAGE_S3_REGION=ap-southeast-2')
HUNK = re.compile(r'^@@ -(\d+)(?:,\d+)? \+\d+(?:,\d+)? @@')
READABLE_FOLDER = 0o755
READABLE_FILE = 0o644
STARTUP_SECONDS = 30


def load(path: Path) -> dict:
    return yaml.safe_load(path.read_text())


def storage(path: Path) -> dict:
    return load(path)['storage']


@dataclass(frozen=True)
class Block:
    tokens: tuple[str, ...]
    children: tuple['Block', ...] = ()


def parse_caddyfile(text: str) -> list[Block]:
    stack: list[list[Block]] = [[]]
    heads: list[list[str]] = []
    for line in text.splitlines():
        tokens = shlex.split(line, comments=True)
        if not tokens:
            continue
        if tokens == ['}']:
            children = stack.pop()
            stack[-1].append(Block(tuple(heads.pop()), tuple(children)))
        elif tokens[-1] == '{':
            heads.append(tokens[:-1])
            stack.append([])
        else:
            stack[-1].append(Block(tuple(tokens)))
    if heads:
        raise AssertionError(f'unclosed Caddyfile block {heads[-1]}')
    return stack[0]


def path_matches(pattern: str, path: str) -> bool:
    if pattern.endswith('*'):
        return path.startswith(pattern[:-1])
    return path == pattern


@dataclass(frozen=True)
class Matcher:
    paths: tuple[str, ...] = ()
    methods: tuple[str, ...] = ()
    excluded: tuple[str, ...] = ()

    @classmethod
    def from_block(cls, block: Block) -> 'Matcher':
        lines = [block.tokens[1:]] if len(block.tokens) > 1 else [child.tokens for child in block.children]
        paths: list[str] = []
        methods: list[str] = []
        excluded: list[str] = []
        for tokens in lines:
            match tokens:
                case ('path', *values):
                    paths.extend(values)
                case ('method', *values):
                    methods.extend(values)
                case ('not', 'method', *values):
                    excluded.extend(values)
                case _:
                    raise AssertionError(f'{block.tokens[0]} uses a matcher this test does not model: {tokens}')
        return cls(tuple(paths), tuple(methods), tuple(excluded))

    def matches(self, method: str, path: str) -> bool:
        return (
            (not self.paths or any(path_matches(pattern, path) for pattern in self.paths))
            and (not self.methods or method in self.methods)
            and method not in self.excluded
        )


@dataclass(frozen=True)
class Outcome:
    upstream: str | None = None
    status: int | None = None
    headers: dict[str, str] = field(default_factory=dict)


@dataclass(frozen=True)
class Handle:
    matcher: Matcher | None
    outcome: Outcome


@dataclass(frozen=True)
class Site:
    matchers: dict[str, Matcher]
    handles: tuple[Handle, ...]

    @classmethod
    def from_block(cls, block: Block) -> 'Site':
        matchers = {child.tokens[0]: Matcher.from_block(child) for child in block.children if child.tokens[0].startswith('@')}
        handles = []
        for child in block.children:
            match child.tokens:
                case ('handle',):
                    handles.append(Handle(None, outcome(child)))
                case ('handle', name):
                    handles.append(Handle(matchers[name], outcome(child)))
                case (name, *_) if name.startswith('@'):
                    continue
                case _:
                    raise AssertionError(f'a site uses a directive this test does not model: {child.tokens}')
        return cls(matchers, tuple(handles))

    def route(self, method: str, path: str) -> Outcome:
        matched = [handle for handle in self.handles if handle.matcher is not None and handle.matcher.matches(method, path)]
        if len(matched) > 1:
            raise AssertionError(f'{method} {path} matches more than one handle, so it depends on how Caddy orders them')
        if matched:
            return matched[0].outcome
        fallback = [handle for handle in self.handles if handle.matcher is None]
        return fallback[0].outcome if fallback else Outcome()


def outcome(block: Block) -> Outcome:
    upstream = None
    status = None
    headers: dict[str, str] = {}
    for child in block.children:
        match child.tokens:
            case ('reverse_proxy', target):
                upstream = target
            case ('respond', code):
                status = int(code)
            case ('header', name, value):
                headers[name] = value
            case _:
                raise AssertionError(f'a handle uses a directive this test does not model: {child.tokens}')
    return Outcome(upstream, status, headers)


def sites(path: Path) -> dict[str, Site]:
    return {block.tokens[0]: Site.from_block(block) for block in parse_caddyfile(path.read_text())}


def json_matches(sets: list[dict] | None, host: str, method: str, path: str) -> bool:
    if sets is None:
        return True
    return any(json_set_matches(matcher, host, method, path) for matcher in sets)


def json_set_matches(matcher: dict, host: str, method: str, path: str) -> bool:
    for kind, value in matcher.items():
        match kind:
            case 'host':
                found = host in value
            case 'path':
                found = any(path_matches(pattern, path) for pattern in value)
            case 'method':
                found = method in value
            case 'not':
                found = not json_matches(value, host, method, path)
            case _:
                raise AssertionError(f'the adapted config uses a matcher this test does not model: {kind}')
        if not found:
            return False
    return True


def evaluate(routes: list[dict], host: str, method: str, path: str, headers: dict[str, str]) -> Outcome | None:
    taken: set[str] = set()
    for route in routes:
        group = route.get('group')
        if group in taken or not json_matches(route.get('match'), host, method, path):
            continue
        if group is not None:
            taken.add(group)
        for handler in route.get('handle', []):
            match handler['handler']:
                case 'subroute':
                    found = evaluate(handler['routes'], host, method, path, headers)
                    if found is not None:
                        return found
                case 'reverse_proxy':
                    return Outcome(handler['upstreams'][0]['dial'], None, dict(headers))
                case 'static_response':
                    return Outcome(None, int(handler.get('status_code', 200)), dict(headers))
                case 'headers':
                    headers.update({name: ', '.join(values) for name, values in handler['response']['set'].items()})
                case kind:
                    raise AssertionError(f'the adapted config uses a handler this test does not model: {kind}')
        if route.get('terminal'):
            return None
    return None


def expected_reference_routes() -> list[tuple[str, str, str, Outcome]]:
    signer = Outcome(SIGNER)
    missing = Outcome(status=404)
    refused = Outcome(status=405, headers={'Allow': 'GET, HEAD'})
    routes = []
    for method in METHODS:
        routes.extend((SIGNER_SITE, method, path, signer) for path in SIGNER_PATHS)
        routes.append((SIGNER_SITE, method, TOKEN_PATH, signer if method in TOKEN_METHODS else missing))
        routes.append((SIGNER_SITE, method, BUNDLE_PATH, signer if method in BUNDLE_METHODS else missing))
        routes.extend((SIGNER_SITE, method, path, missing) for path in UNKNOWN_PATHS)
        routes.extend(
            (REGISTRY_SITE, method, path, Outcome(REGISTRY) if method in PULL_METHODS else refused)
            for path in REGISTRY_PATHS
        )
        routes.extend((REGISTRY_SITE, method, path, missing) for path in UNKNOWN_PATHS + SIGNER_PATHS + (TOKEN_PATH,))
    return routes


def expected_current_routes() -> list[tuple[str, str, str, Outcome]]:
    routes = []
    for method in METHODS:
        routes.extend((SIGNER_SITE, method, path, Outcome(SIGNER)) for path in SIGNER_PATHS)
        routes.extend(
            (SIGNER_SITE, method, path, Outcome(status=404))
            for path in UNKNOWN_PATHS + (TOKEN_PATH, BUNDLE_PATH) + REGISTRY_PATHS
        )
    return routes


def patched(original: str, hunks: list[str]) -> str:
    source = original.splitlines(keepends=True)
    result: list[str] = []
    position = 0
    for line in hunks:
        header = HUNK.match(line)
        if header is not None:
            start = int(header.group(1)) - 1
            if start < position:
                raise AssertionError(f'hunk {line.strip()} overlaps the one before it')
            result.extend(source[position:start])
            position = start
        elif line.startswith((' ', '-')):
            if position >= len(source) or source[position] != line[1:]:
                raise AssertionError(f'the diff expects {line[1:]!r} at line {position + 1}')
            if line.startswith(' '):
                result.append(line[1:])
            position += 1
        elif line.startswith('+'):
            result.append(line[1:])
        else:
            raise AssertionError(f'not a unified diff line: {line!r}')
    result.extend(source[position:])
    return ''.join(result)


def docker_available() -> bool:
    if shutil.which('docker') is None:
        return False
    try:
        result = subprocess.run(['docker', 'info'], capture_output=True, timeout=30)
    except subprocess.TimeoutExpired:
        return False
    return result.returncode == 0


def docker(*arguments: str, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(['docker', *arguments], capture_output=True, text=True, timeout=180, check=check)


@contextmanager
def staged(files: dict[str, str]) -> Iterator[Path]:
    with TemporaryDirectory() as folder:
        staging = Path(folder)
        staging.chmod(READABLE_FOLDER)
        for name, content in files.items():
            path = staging / name
            path.write_text(content)
            path.chmod(READABLE_FILE)
        yield staging


@contextmanager
def container(*arguments: str) -> Iterator[str]:
    identifier = docker('run', '-d', '--rm', *arguments).stdout.strip()
    try:
        yield identifier
    finally:
        docker('rm', '-f', identifier, check=False)


def wait_for_log(identifier: str, text: str) -> str:
    deadline = time.monotonic() + STARTUP_SECONDS
    while time.monotonic() < deadline:
        logs = docker('logs', identifier, check=False)
        output = logs.stdout + logs.stderr
        if text in output:
            return output
        time.sleep(0.5)
    raise AssertionError(f'the registry never logged {text!r}:\n{output}')


class StorageTests(unittest.TestCase):
    def test_storage_blocks_match_apart_from_maintenance(self) -> None:
        served = storage(REGISTRY_CONFIG)
        published = storage(PUBLISH_CONFIG)
        served.pop('maintenance')
        published.pop('maintenance')
        self.assertEqual(served, published, 'the publishing registry must write exactly where the served registry reads')

    def test_s3_uses_the_instance_role_and_the_registry_prefix(self) -> None:
        for path in (REGISTRY_CONFIG, PUBLISH_CONFIG):
            s3 = storage(path)['s3']
            with self.subTest(config=path.name):
                for key in CREDENTIAL_KEYS:
                    self.assertNotIn(key, s3, 'credentials come from the instance role or GitHub OIDC, never the file')
                self.assertNotIn('bucket', s3, 'the bucket comes from REGISTRY_STORAGE_S3_BUCKET')
                self.assertNotIn('region', s3, 'the region comes from REGISTRY_STORAGE_S3_REGION')
                self.assertNotIn('regionendpoint', s3)
                self.assertEqual(s3['rootdirectory'], '/registry')
                self.assertIs(s3['encrypt'], True)
                self.assertIs(s3.get('secure', True), True)

    def test_blobs_redirect_to_s3_and_nothing_is_deleted(self) -> None:
        for path in (REGISTRY_CONFIG, PUBLISH_CONFIG):
            served = storage(path)
            with self.subTest(config=path.name):
                self.assertEqual(served['redirect'], {'disable': False})
                self.assertEqual(served['delete'], {'enabled': False})
                self.assertEqual(served['cache'], {'blobdescriptor': 'inmemory'})
                self.assertEqual(served['maintenance']['uploadpurging'], {'enabled': False})

    def test_only_one_storage_driver_is_configured(self) -> None:
        options = {'delete', 'redirect', 'cache', 'maintenance', 'tag'}
        for path in (REGISTRY_CONFIG, PUBLISH_CONFIG):
            with self.subTest(config=path.name):
                self.assertEqual(set(storage(path)) - options, {'s3'})


class ServedRegistryTests(unittest.TestCase):
    def test_is_read_only(self) -> None:
        self.assertIs(storage(REGISTRY_CONFIG)['maintenance']['readonly']['enabled'], True)

    def test_trusts_only_eddsa_tokens_from_the_signer(self) -> None:
        self.assertEqual(load(REGISTRY_CONFIG)['auth'], {'token': TOKEN_AUTH})

    def test_listens_for_caddy_on_the_signer_network(self) -> None:
        http = load(REGISTRY_CONFIG)['http']
        self.assertEqual(http['addr'], ':5000')
        self.assertEqual(http['host'], f'https://{REGISTRY_SITE}')
        self.assertNotIn('debug', http, 'a debug server would expose pprof and metrics on the signer network')
        self.assertNotIn('secret', http, 'keep secrets out of the repository; pass REGISTRY_HTTP_SECRET if one is ever needed')

    def test_logs_json_without_the_combined_access_log(self) -> None:
        log = load(REGISTRY_CONFIG)['log']
        self.assertEqual(log['formatter'], 'json')
        self.assertIs(log['accesslog']['disabled'], True)


class PublishRegistryTests(unittest.TestCase):
    def test_has_no_auth(self) -> None:
        self.assertNotIn('auth', load(PUBLISH_CONFIG))

    def test_is_writable(self) -> None:
        self.assertIs(storage(PUBLISH_CONFIG)['maintenance']['readonly']['enabled'], False)

    def test_listens_on_loopback_port_5000(self) -> None:
        http = load(PUBLISH_CONFIG)['http']
        host, _, port = http['addr'].rpartition(':')
        self.assertTrue(ipaddress.ip_address(host).is_loopback, http['addr'])
        self.assertEqual(port, '5000')
        self.assertNotIn('debug', http)

    def test_logs_json(self) -> None:
        self.assertEqual(load(PUBLISH_CONFIG)['log']['formatter'], 'json')


class CaddyfileTests(unittest.TestCase):
    def assert_routes(self, parsed: dict[str, Site], expected: list[tuple[str, str, str, Outcome]]) -> None:
        for site, method, path, want in expected:
            with self.subTest(site=site, method=method, path=path):
                self.assertEqual(parsed[site].route(method, path), want)

    def test_reference_routes(self) -> None:
        parsed = sites(CADDYFILE)
        self.assertEqual(set(parsed), {SIGNER_SITE, REGISTRY_SITE})
        self.assert_routes(parsed, expected_reference_routes())

    def test_token_passes_get_post_and_head_to_the_signer(self) -> None:
        site = sites(CADDYFILE)[SIGNER_SITE]
        for method in TOKEN_METHODS:
            with self.subTest(method=method):
                self.assertEqual(site.route(method, TOKEN_PATH).upstream, SIGNER, 'containerd POSTs to the realm and falls back to GET only on the signer 405')

    def test_registry_site_refuses_everything_but_pulls(self) -> None:
        site = sites(CADDYFILE)[REGISTRY_SITE]
        for method in METHODS:
            for path in REGISTRY_PATHS:
                with self.subTest(method=method, path=path):
                    routed = site.route(method, path)
                    if method in PULL_METHODS:
                        self.assertEqual(routed.upstream, REGISTRY)
                    else:
                        self.assertIsNone(routed.upstream)
                        self.assertEqual(routed.status, 405)

    def test_current_routes(self) -> None:
        parsed = sites(CURRENT_CADDYFILE)
        self.assertEqual(set(parsed), {SIGNER_SITE})
        self.assert_routes(parsed, expected_current_routes())

    def test_existing_routes_are_unchanged(self) -> None:
        current = sites(CURRENT_CADDYFILE)[SIGNER_SITE]
        reference = sites(CADDYFILE)[SIGNER_SITE]
        self.assertEqual(reference.matchers['@signer'], current.matchers['@signer'])
        for method in METHODS:
            for path in SIGNER_PATHS:
                with self.subTest(method=method, path=path):
                    self.assertEqual(reference.route(method, path), current.route(method, path))

    def test_diff_turns_the_current_file_into_the_reference(self) -> None:
        diff = CADDYFILE_DIFF.read_text().splitlines(keepends=True)
        self.assertEqual(diff[:2], [f'--- {CURRENT_CADDYFILE.name}\n', f'+++ {CADDYFILE.name}\n'])
        self.assertEqual(
            patched(CURRENT_CADDYFILE.read_text(), diff[2:]),
            CADDYFILE.read_text(),
            'regenerate it with: diff -u --label Caddyfile.current --label Caddyfile Caddyfile.current Caddyfile > Caddyfile.diff',
        )


@unittest.skipUnless(docker_available(), 'docker is not available')
class CaddyImageTests(unittest.TestCase):
    def caddy(self, path: Path, *arguments: str) -> subprocess.CompletedProcess:
        return docker(
            'run', '--rm', '--network', 'none',
            '-v', f'{path}:/etc/caddy/Caddyfile:ro',
            CADDY_IMAGE, 'caddy', *arguments, '--config', '/etc/caddy/Caddyfile', '--adapter', 'caddyfile',
            check=False,
        )

    def adapted(self, path: Path) -> list[dict]:
        result = self.caddy(path, 'adapt')
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)['apps']['http']['servers']['srv0']['routes']

    def assert_adapted_routes(self, path: Path, expected: list[tuple[str, str, str, Outcome]]) -> None:
        routes = self.adapted(path)
        for site, method, request, want in expected:
            with self.subTest(caddyfile=path.name, site=site, method=method, path=request):
                self.assertEqual(evaluate(routes, site, method, request, {}), want)

    def test_both_caddyfiles_validate(self) -> None:
        for path in (CURRENT_CADDYFILE, CADDYFILE):
            with self.subTest(caddyfile=path.name):
                result = self.caddy(path, 'validate')
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_reference_routes_as_caddy_orders_them(self) -> None:
        self.assert_adapted_routes(CADDYFILE, expected_reference_routes())

    def test_current_routes_as_caddy_orders_them(self) -> None:
        self.assert_adapted_routes(CURRENT_CADDYFILE, expected_current_routes())


@unittest.skipUnless(docker_available(), 'docker is not available')
class RegistryImageTests(unittest.TestCase):
    def registry(self, config: Path, staging: Path, *arguments: str) -> AbstractContextManager[str]:
        environment = [option for variable in STORAGE_ENVIRONMENT for option in ('-e', variable)]
        return container(
            '--user', '65534:65534', '--read-only', '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges',
            '--memory', '128m', *environment,
            '-v', f'{config}:/etc/distribution/config.yml:ro',
            '-v', f'{staging / "jwks.json"}:/etc/zone-registry/jwks.json:ro',
            *arguments, REGISTRY_IMAGE,
        )

    def test_served_config_challenges_for_a_signer_token(self) -> None:
        with staged({'jwks.json': json.dumps(JWKS)}) as staging, self.registry(
            REGISTRY_CONFIG, staging, '-p', '127.0.0.1::5000'
        ) as identifier:
            wait_for_log(identifier, 'listening on')
            address = docker('port', identifier, '5000/tcp').stdout.splitlines()[0].strip()
            with self.assertRaises(urllib.error.HTTPError) as raised:
                urllib.request.urlopen(f'http://{address}/v2/', timeout=10)
            refusal = raised.exception
            refusal.close()
            self.assertEqual(refusal.code, 401)
            self.assertEqual(refusal.headers['WWW-Authenticate'], CHALLENGE)

    def test_publish_config_listens_on_loopback(self) -> None:
        with staged({'jwks.json': json.dumps(JWKS)}) as staging, self.registry(PUBLISH_CONFIG, staging) as identifier:
            wait_for_log(identifier, 'listening on 127.0.0.1:5000')


if __name__ == '__main__':
    unittest.main()
