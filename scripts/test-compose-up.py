#!/usr/bin/env python3
"""Verify that make up and make dev retire services outside the new profile set, without Docker."""

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ALL_PROFILES = 'dev,vpn,monitoring,bundled-ollama,bundled-comfyui,comfyui-model-setup'
CLEARED = ('PROFILES', 'COMPOSE_PROFILES', 'COMPOSE_FILE', 'ZONE_VPN', 'COMPOSE_PATH_SEPARATOR')

FAKE_COMPOSE = '''#!/usr/bin/env python3
import json, os, subprocess, sys
with open(os.environ['COMPOSE_CALLS'], 'a') as output:
    output.write(json.dumps(sys.argv[1:]) + '\\n')
if sys.argv[1:2] == ['persist']:
    if os.environ.get('COMPOSE_PERSIST_FAILS'):
        sys.exit(1)
    sys.exit(subprocess.run(['sh', os.environ['COMPOSE_SCRIPT'], *sys.argv[1:]]).returncode)
'''

FAKE_DOCKER = '''#!/usr/bin/env python3
import json, os, sys
arguments = sys.argv[1:]
with open(os.environ['DOCKER_CALLS'], 'a') as output:
    output.write(json.dumps(arguments) + '\\n')
if 'config' in arguments and '--services' in arguments:
    profiles = {arguments[index + 1] for index, value in enumerate(arguments) if value == '--profile'}
    services = ['manager', 'postgres']
    if 'monitoring' in profiles:
        services += ['prometheus', 'grafana']
    if 'vpn' in profiles:
        services += ['gluetun', 'searxng']
    if 'bundled-comfyui' in profiles:
        services += ['comfyui']
    print('\\n'.join(services))
'''


def clean_environment(**extra: str) -> dict[str, str]:
    environment = {
        key: value for key, value in os.environ.items() if key not in CLEARED and not key.startswith('MAKE')
    }
    return environment | extra


def install(folder: Path, name: str, source: str) -> Path:
    command = folder / name
    command.write_text(source)
    command.chmod(0o700)
    return command


def read_calls(log: Path) -> list[list[str]]:
    if not log.exists():
        return []
    return [json.loads(line) for line in log.read_text().splitlines()]


def saved_profiles(path: Path) -> str:
    return next(line.split('=', 1)[1] for line in path.read_text().splitlines() if line.startswith('COMPOSE_PROFILES='))


class MakeTargets(unittest.TestCase):
    def run_make(self, *arguments: str, succeeds: bool = True, saved: str = 'vpn',
                 persisted: str | None = None, **extra: str) -> list[list[str]]:
        with tempfile.TemporaryDirectory(prefix='zone-compose-up-') as directory:
            folder = Path(directory)
            log = folder / 'calls.jsonl'
            command = install(folder, 'compose', FAKE_COMPOSE)
            environment_file = write_environment_file(folder, saved)
            environment = clean_environment(COMPOSE_CALLS=str(log), COMPOSE_SCRIPT=str(ROOT / 'scripts/compose.sh'),
                                            ZONE_ENV_FILE=str(environment_file), **extra)
            result = subprocess.run(
                ['make', '-f', os.environ.get('ZONE_MAKEFILE', 'Makefile'), *arguments, f'COMPOSE={command}'],
                cwd=ROOT, env=environment, text=True, capture_output=True, timeout=10,
            )
            self.assertEqual(result.returncode == 0, succeeds, result.stdout + result.stderr)
            if persisted is not None:
                self.assertEqual(saved_profiles(environment_file), persisted)
            return read_calls(log)

    def test_plain_up_retires_every_optional_profile_before_starting_core(self) -> None:
        self.assertEqual(self.run_make('up'), [
            ['persist', ''],
            ['retire', ''],
            ['--replace-profiles=', 'up', '-d'],
        ])

    def test_up_with_profiles_retires_services_outside_them(self) -> None:
        self.assertEqual(self.run_make('up', 'PROFILES=monitoring', persisted='monitoring'), [
            ['persist', 'monitoring'],
            ['retire', 'monitoring'],
            ['--replace-profiles=monitoring', 'up', '-d'],
        ])

    def test_up_with_compose_profiles_environment_retires_services_outside_them(self) -> None:
        self.assertEqual(self.run_make('up', COMPOSE_PROFILES='vpn'), [
            ['persist', 'vpn'],
            ['retire', 'vpn'],
            ['--replace-profiles=vpn', 'up', '-d'],
        ])

    def test_dev_adds_dev_to_the_saved_profiles_and_retires_the_rest(self) -> None:
        self.assertEqual(self.run_make('dev', persisted='dev,vpn'), [
            ['persist', '--ensure', 'dev'],
            ['retire', 'dev,vpn'],
            ['--replace-profiles=dev,vpn', 'up', '--build'],
        ])

    def test_dev_with_profiles_replaces_the_saved_profiles_and_retires_the_rest(self) -> None:
        self.assertEqual(self.run_make('dev', 'PROFILES=monitoring', persisted='dev,monitoring'), [
            ['persist', '--ensure', 'dev', 'monitoring'],
            ['retire', 'dev,monitoring'],
            ['--replace-profiles=dev,monitoring', 'up', '--build'],
        ])

    def test_dev_with_no_saved_profiles_runs_dev_alone(self) -> None:
        self.assertEqual(self.run_make('dev', saved='', persisted='dev'), [
            ['persist', '--ensure', 'dev'],
            ['retire', 'dev'],
            ['--replace-profiles=dev', 'up', '--build'],
        ])

    def test_up_stops_when_the_profiles_cannot_be_saved(self) -> None:
        self.assertEqual(self.run_make('up', 'PROFILES=monitoring', succeeds=False, COMPOSE_PERSIST_FAILS='1'), [
            ['persist', 'monitoring'],
        ])

    def test_dev_stops_when_the_profiles_cannot_be_saved(self) -> None:
        self.assertEqual(self.run_make('dev', succeeds=False, COMPOSE_PERSIST_FAILS='1'), [
            ['persist', '--ensure', 'dev'],
        ])

    def test_up_comfyui_persists_its_profile_before_starting_comfyui(self) -> None:
        calls = self.run_make('up-comfyui')
        persist = ['persist', '--ensure', 'bundled-comfyui']
        self.assertIn(persist, calls)
        starts = [index for index, call in enumerate(calls) if call[-3:] == ['up', '-d', 'comfyui']]
        self.assertEqual(len(starts), 1, calls)
        self.assertLess(calls.index(persist), starts[0])
        self.assertFalse(any(call[:1] == ['retire'] for call in calls), calls)


def write_environment_file(folder: Path, saved: str = '') -> Path:
    path = folder / 'environment'
    example = (ROOT / '.env.example').read_text()
    assert '\nCOMPOSE_PROFILES=\n' in example, '.env.example must leave COMPOSE_PROFILES empty'
    path.write_text(example.replace('\nCOMPOSE_PROFILES=\n', f'\nCOMPOSE_PROFILES={saved}\n'))
    return path


def run_script(folder: Path, *arguments: str, **extra: str) -> list[list[str]]:
    log = folder / 'calls.jsonl'
    log.unlink(missing_ok=True)
    install(folder, 'docker', FAKE_DOCKER)
    environment = clean_environment(DOCKER_CALLS=str(log), PATH=f'{folder}:{os.environ["PATH"]}', **extra)
    result = subprocess.run(
        ['sh', 'scripts/compose.sh', *arguments],
        cwd=ROOT, env=environment, text=True, capture_output=True, timeout=10,
    )
    if result.returncode != 0:
        raise AssertionError(result.stdout + result.stderr)
    return [call for call in read_calls(log) if call != ['compose', 'version']]


def env_files(call: list[str]) -> list[str]:
    return [call[index + 1] for index, value in enumerate(call) if value == '--env-file']


class Retire(unittest.TestCase):
    def retire(self, profiles: str) -> list[list[str]]:
        with tempfile.TemporaryDirectory(prefix='zone-compose-retire-') as directory:
            folder = Path(directory)
            chosen = str(write_environment_file(folder))
            calls = run_script(folder, 'retire', '--env-file', chosen, profiles, ZONE_ENV_FILE=str(folder / 'missing'))
            for call in calls:
                self.assertEqual(env_files(call), [chosen], call)
            return [call for call in calls if 'config' not in call]

    def assert_stops(self, profiles: str, services: list[str]) -> None:
        retirements = self.retire(profiles)
        self.assertEqual(len(retirements), 1, retirements)
        call = retirements[0]
        self.assertNotIn('rm', call, 'removing a container orphans the anonymous volumes its image declares')
        self.assertNotIn('down', call)
        self.assertEqual(call[call.index('stop') + 1:], services)
        for profile in ALL_PROFILES.split(','):
            self.assertIn(['--profile', profile], [call[index:index + 2] for index in range(len(call))])

    def test_nothing_is_stopped_when_every_profile_stays_active(self) -> None:
        self.assertEqual(self.retire(ALL_PROFILES), [])

    def test_core_stops_every_optional_service(self) -> None:
        self.assert_stops('', ['comfyui', 'gluetun', 'grafana', 'prometheus', 'searxng'])

    def test_monitoring_keeps_its_services(self) -> None:
        self.assert_stops('monitoring', ['comfyui', 'gluetun', 'searxng'])


class EnvironmentFile(unittest.TestCase):
    def test_up_reads_the_same_env_file_as_retire(self) -> None:
        with tempfile.TemporaryDirectory(prefix='zone-compose-env-') as directory:
            folder = Path(directory)
            chosen = str(write_environment_file(folder))
            retire = run_script(folder, 'retire', '', ZONE_ENV_FILE=chosen)
            up = run_script(folder, '--replace-profiles=', 'up', '-d', ZONE_ENV_FILE=chosen)
            self.assertTrue(retire, 'retire must ask Compose which services are inactive')
            self.assertEqual(len(up), 1, up)
            for call in retire + up:
                self.assertEqual(env_files(call), [chosen], call)

    def test_up_reads_the_env_file_it_is_given(self) -> None:
        with tempfile.TemporaryDirectory(prefix='zone-compose-env-') as directory:
            folder = Path(directory)
            chosen = str(write_environment_file(folder))
            calls = run_script(
                folder, '--env-file', chosen, '--replace-profiles=', 'up', '-d', ZONE_ENV_FILE=str(folder / 'missing'),
            )
            self.assertEqual(len(calls), 1, calls)
            self.assertEqual(env_files(calls[0]), [chosen])
            self.assertEqual(calls[0][-2:], ['up', '-d'])

    def test_a_missing_env_file_is_left_to_compose(self) -> None:
        with tempfile.TemporaryDirectory(prefix='zone-compose-env-') as directory:
            folder = Path(directory)
            calls = run_script(folder, '--replace-profiles=', 'ps', ZONE_ENV_FILE=str(folder / 'missing'))
            self.assertEqual(len(calls), 1, calls)
            self.assertEqual(env_files(calls[0]), [])


if __name__ == '__main__':
    unittest.main()
