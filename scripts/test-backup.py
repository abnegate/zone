#!/usr/bin/env python3
"""Verify make backup archives the postgres cluster stopped, and make restore refuses a running stack, without Docker."""

import json
import os
import shutil
import subprocess
import tempfile
import unittest
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MAKEFILE = Path(os.environ.get('ZONE_MAKEFILE', ROOT / 'Makefile')).resolve()
SHELLS = list(dict.fromkeys(filter(None, ['/bin/sh', shutil.which('dash')])))
POSTGRES = 'c0ffee'
OTHER_VOLUMES = ['./ollama', './valkey', './manager_repos', './manager_artifacts',
                 './manager_agent_state', './prometheus', './grafana', './traefik']

FAKE_DOCKER = '''#!/usr/bin/env python3
import json, os, signal, sys
arguments = sys.argv[1:]
with open(os.environ['FAKE_DOCKER_LOG'], 'a') as output:
    output.write(json.dumps(arguments) + '\\n')
joined = ' '.join(arguments)
if arguments[0] == 'ps':
    print(os.environ['FAKE_DOCKER_PS'])
    sys.exit(0)
if arguments[0] == 'inspect':
    print(os.environ['FAKE_DOCKER_EXIT_CODE'])
    sys.exit(0)
if arguments[0] == 'run' and 'PG_VERSION' in joined:
    sys.exit(0 if os.environ['FAKE_DOCKER_CLUSTER'] == '1' else 1)
signalled = os.environ.get('FAKE_DOCKER_SIGNAL_ON')
if signalled and signalled in joined:
    os.kill(os.getppid(), signal.SIGTERM)
for match, code in json.loads(os.environ['FAKE_DOCKER_FAIL']).items():
    if match in joined:
        sys.exit(code)
sys.exit(0)
'''


@dataclass
class Run:
    returncode: int
    output: str
    calls: list[list[str]]
    directory_mode: int | None = None

    def indexes(self, predicate: Callable[[list[str]], bool]) -> list[int]:
        return [index for index, call in enumerate(self.calls) if predicate(call)]

    def first(self, predicate: Callable[[list[str]], bool]) -> int:
        found = self.indexes(predicate)
        if not found:
            raise AssertionError(f'no matching docker call in {json.dumps(self.calls, indent=1)}')
        return found[0]


def is_stop(call: list[str]) -> bool:
    return call[0] == 'stop'


def is_start(call: list[str]) -> bool:
    return call[0] == 'start'


def is_exit_code_check(call: list[str]) -> bool:
    return call[0] == 'inspect' and any('.State.ExitCode' in argument for argument in call)


def is_cluster_archive(call: list[str]) -> bool:
    return call[0] == 'run' and 'PG_VERSION' not in ' '.join(call) and any(
        'tar' in argument and './postgres' in argument for argument in call)


def is_other_archive(call: list[str]) -> bool:
    return call[0] == 'run' and any('tar' in argument and './valkey' in argument for argument in call)


def is_extract(call: list[str]) -> bool:
    return call[0] == 'run' and 'tar' in call and call[call.index('tar') + 1].startswith('x')


@dataclass
class Fake:
    running: str = POSTGRES
    cluster: bool = True
    exit_code: str = '0'
    fail: dict[str, int] = field(default_factory=dict)
    signal_on: str = ''


class MakeTarget(unittest.TestCase):
    def make(self, target: str, fake: Fake, shell: str, *variables: str) -> Run:
        with tempfile.TemporaryDirectory(prefix='zone-backup-command-') as directory:
            folder = Path(directory)
            bin_directory = folder / 'bin'
            bin_directory.mkdir()
            command = bin_directory / 'docker'
            command.write_text(FAKE_DOCKER)
            command.chmod(0o700)
            work = folder / 'work'
            work.mkdir()
            log = folder / 'calls.jsonl'
            log.touch()
            environment = os.environ | {
                'PATH': f'{bin_directory}:{os.environ["PATH"]}',
                'FAKE_DOCKER_LOG': str(log),
                'FAKE_DOCKER_PS': fake.running,
                'FAKE_DOCKER_CLUSTER': '1' if fake.cluster else '0',
                'FAKE_DOCKER_EXIT_CODE': fake.exit_code,
                'FAKE_DOCKER_FAIL': json.dumps(fake.fail),
                'FAKE_DOCKER_SIGNAL_ON': fake.signal_on,
            }
            environment.pop('ALLOW_EMPTY_POSTGRES', None)
            result = subprocess.run(
                ['make', '-f', str(MAKEFILE), '-C', str(work), target, f'SHELL={shell}', *variables],
                env=environment, text=True, capture_output=True, timeout=30,
            )
            backups = work / 'backups'
            mode = backups.stat().st_mode & 0o777 if backups.exists() else None
            calls = [json.loads(line) for line in log.read_text().splitlines()]
            return Run(result.returncode, result.stdout + result.stderr, calls, mode)


class Backup(MakeTarget):
    def test_running_postgres_is_stopped_only_while_its_cluster_is_archived(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(), shell)
                self.assertEqual(run.returncode, 0, run.output)
                stop = run.first(is_stop)
                self.assertEqual(run.calls[stop], ['stop', '-t', '120', POSTGRES])
                check = run.first(is_exit_code_check)
                archive = run.first(is_cluster_archive)
                start = run.first(is_start)
                self.assertEqual(run.calls[start], ['start', POSTGRES])
                others = run.first(is_other_archive)
                self.assertLess(stop, check)
                self.assertLess(check, archive)
                self.assertLess(archive, start)
                self.assertLess(start, others)
                self.assertEqual(len(run.indexes(is_start)), 1)
                self.assertNotIn('zone_ollama_data:/data/ollama:ro', run.calls[archive])
                self.assertIn('zone_postgres_data:/data/postgres:ro', run.calls[archive])
                self.assertNotIn('zone_postgres_data:/data/postgres:ro', run.calls[others])
                archived = ' '.join(run.calls[others])
                for volume in OTHER_VOLUMES:
                    self.assertIn(volume, archived)
                self.assertIn('-rf', archived)
                self.assertIn('Backup created: backups/zone_backup_', run.output)
                self.assertEqual(run.directory_mode, 0o700)

    def test_failed_cluster_archive_still_starts_postgres(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(fail={'./postgres': 2}), shell)
                self.assertNotEqual(run.returncode, 0)
                self.assertLess(run.first(is_cluster_archive), run.first(is_start))
                self.assertEqual(run.indexes(is_other_archive), [])
                self.assertNotIn('Backup created', run.output)

    def test_signal_during_cluster_archive_still_starts_postgres(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(signal_on='./postgres'), shell)
                self.assertNotEqual(run.returncode, 0)
                self.assertLess(run.first(is_cluster_archive), run.first(is_start))
                self.assertEqual(run.indexes(is_other_archive), [])

    def test_unclean_shutdown_is_not_archived(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(exit_code='137'), shell)
                self.assertNotEqual(run.returncode, 0)
                self.assertLess(run.first(is_exit_code_check), run.first(is_start))
                self.assertEqual(run.indexes(is_cluster_archive), [])
                self.assertIn('did not shut down cleanly', run.output)

    def test_stopped_postgres_is_neither_stopped_nor_started(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(running=''), shell)
                self.assertEqual(run.returncode, 0, run.output)
                self.assertEqual(run.indexes(is_stop), [])
                self.assertEqual(run.indexes(is_start), [])
                self.assertLess(run.first(is_cluster_archive), run.first(is_other_archive))

    def test_empty_cluster_volume_is_archived_without_stopping_anything(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                refused = self.make('backup', Fake(cluster=False), shell)
                self.assertNotEqual(refused.returncode, 0)
                self.assertEqual(refused.indexes(is_stop), [])
                self.assertEqual(refused.indexes(is_cluster_archive), [])

                run = self.make('backup', Fake(cluster=False), shell, 'ALLOW_EMPTY_POSTGRES=1')
                self.assertEqual(run.returncode, 0, run.output)
                self.assertEqual(run.indexes(is_stop), [])
                self.assertEqual(run.indexes(is_start), [])
                self.assertLess(run.first(is_cluster_archive), run.first(is_other_archive))


class Restore(MakeTarget):
    def test_restore_refuses_while_a_zone_volume_is_mounted(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('restore', Fake(), shell, 'BACKUP=backups/zone_backup_20260930_000000.tar.gz')
                self.assertNotEqual(run.returncode, 0)
                self.assertEqual(run.indexes(is_extract), [])
                self.assertIn('Stop the stack first: make stop', run.output)
                listing = run.calls[run.first(lambda call: call[0] == 'ps')]
                for volume in ['zone_postgres_data', 'zone_manager_agent_state', 'zone_traefik_letsencrypt']:
                    self.assertIn(f'volume={volume}', listing)

    def test_restore_extracts_with_the_stack_down(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('restore', Fake(running=''), shell, 'BACKUP=backups/zone_backup_20260930_000000.tar.gz')
                self.assertEqual(run.returncode, 0, run.output)
                self.assertEqual(len(run.indexes(is_extract)), 1)


if __name__ == '__main__':
    unittest.main()
