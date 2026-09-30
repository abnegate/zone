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
ENSURED = 'dev,vpn'
CLEARED = ('PROFILES', 'COMPOSE_PROFILES', 'COMPOSE_FILE', 'ZONE_VPN', 'COMPOSE_PATH_SEPARATOR')

FAKE_COMPOSE = '''#!/usr/bin/env python3
import json, os, sys
with open(os.environ['COMPOSE_CALLS'], 'a') as output:
    output.write(json.dumps(sys.argv[1:]) + '\\n')
if sys.argv[1:2] == ['persist']:
    if os.environ.get('COMPOSE_PERSIST_FAILS'):
        sys.exit(1)
    if '--ensure' in sys.argv:
        print(os.environ['COMPOSE_ENSURED'])
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


class MakeTargets(unittest.TestCase):
    def run_make(self, *arguments: str, succeeds: bool = True, **extra: str) -> list[list[str]]:
        with tempfile.TemporaryDirectory(prefix='zone-compose-up-') as directory:
            folder = Path(directory)
            log = folder / 'calls.jsonl'
            command = install(folder, 'compose', FAKE_COMPOSE)
            environment = clean_environment(COMPOSE_CALLS=str(log), COMPOSE_ENSURED=ENSURED, **extra)
            result = subprocess.run(
                ['make', '-f', os.environ.get('ZONE_MAKEFILE', 'Makefile'), *arguments, f'COMPOSE={command}'],
                cwd=ROOT, env=environment, text=True, capture_output=True, timeout=10,
            )
            self.assertEqual(result.returncode == 0, succeeds, result.stdout + result.stderr)
            return read_calls(log)

    def test_plain_up_retires_every_optional_profile_before_starting_core(self) -> None:
        self.assertEqual(self.run_make('up'), [
            ['persist', ''],
            ['retire', ''],
            ['--replace-profiles=', 'up', '-d'],
        ])

    def test_up_with_profiles_retires_services_outside_them(self) -> None:
        self.assertEqual(self.run_make('up', 'PROFILES=monitoring'), [
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

    def test_dev_retires_services_outside_the_ensured_profiles(self) -> None:
        self.assertEqual(self.run_make('dev'), [
            ['persist', '--ensure', 'dev'],
            ['retire', ENSURED],
            [f'--replace-profiles={ENSURED}', 'up', '--build'],
        ])

    def test_dev_with_profiles_retires_services_outside_the_ensured_profiles(self) -> None:
        self.assertEqual(self.run_make('dev', 'PROFILES=monitoring'), [
            ['persist', '--ensure', 'dev', 'monitoring'],
            ['retire', ENSURED],
            [f'--replace-profiles={ENSURED}', 'up', '--build'],
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


class Retire(unittest.TestCase):
    def retire(self, profiles: str) -> list[list[str]]:
        with tempfile.TemporaryDirectory(prefix='zone-compose-retire-') as directory:
            folder = Path(directory)
            log = folder / 'calls.jsonl'
            install(folder, 'docker', FAKE_DOCKER)
            environment_file = folder / 'environment'
            environment_file.write_text((ROOT / '.env.example').read_text())
            environment = clean_environment(
                DOCKER_CALLS=str(log),
                PATH=f'{folder}:{os.environ["PATH"]}',
                ZONE_ENV_FILE=str(folder / 'missing'),
            )
            result = subprocess.run(
                ['sh', 'scripts/compose.sh', 'retire', '--env-file', str(environment_file), profiles],
                cwd=ROOT, env=environment, text=True, capture_output=True, timeout=10,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            calls = read_calls(log)
            for call in calls:
                if 'config' in call:
                    self.assertIn(str(environment_file), call)
            return [call for call in calls if 'rm' in call]

    def assert_removes(self, profiles: str, services: list[str]) -> None:
        removals = self.retire(profiles)
        self.assertEqual(len(removals), 1, removals)
        call = removals[0]
        self.assertNotIn('-v', call)
        self.assertNotIn('--volumes', call)
        self.assertEqual(call[call.index('rm') + 1:], ['--stop', '--force', *services])
        for profile in ALL_PROFILES.split(','):
            self.assertIn(['--profile', profile], [call[index:index + 2] for index in range(len(call))])

    def test_nothing_is_removed_when_every_profile_stays_active(self) -> None:
        self.assertEqual(self.retire(ALL_PROFILES), [])

    def test_core_retires_every_optional_service(self) -> None:
        self.assert_removes('', ['comfyui', 'gluetun', 'grafana', 'prometheus', 'searxng'])

    def test_monitoring_keeps_its_services(self) -> None:
        self.assert_removes('monitoring', ['comfyui', 'gluetun', 'searxng'])


if __name__ == '__main__':
    unittest.main()
