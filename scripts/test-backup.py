#!/usr/bin/env python3
"""Verify make backup copies the postgres cluster stopped and archives in one pass, and make restore replaces volumes only with the stack stopped, without Docker."""

import json
import os
import re
import shutil
import subprocess
import tempfile
import time
import unittest
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MAKEFILE = Path(os.environ.get('ZONE_MAKEFILE', ROOT / 'Makefile')).resolve()
SHELLS = list(dict.fromkeys(filter(None, ['/bin/sh', shutil.which('dash')])))
POSTGRES = 'c0ffee'
OTHER_MOUNTS = ['zone_ollama_data:/data/ollama:ro', 'zone_valkey_data:/data/valkey:ro',
                'zone_manager_repos:/data/manager_repos:ro', 'zone_manager_artifacts:/data/manager_artifacts:ro',
                'zone_manager_agent_state:/data/manager_agent_state:ro', 'zone_prometheus_data:/data/prometheus:ro',
                'zone_grafana_data:/data/grafana:ro', 'zone_traefik_letsencrypt:/data/traefik:ro']
BACKUP = 'BACKUP=backups/zone_backup_20260930_000000.tar.gz'
DATE = '20260930_000000'
RUN_NAME = re.compile(rf'^zone_backup_{DATE}-[0-9]+$')
ARCHIVE_NAME = re.compile(rf'^zone_backup_{DATE}-[0-9]+\.tar\.gz$')

FAKE_DOCKER = '''#!/usr/bin/env python3
import json, os, signal, subprocess, sys, time
arguments = sys.argv[1:]
with open(os.environ['FAKE_DOCKER_LOG'], 'a') as output:
    output.write(json.dumps(arguments) + '\\n')
joined = ' '.join(arguments)
for match, code in json.loads(os.environ['FAKE_DOCKER_FAIL']).items():
    if match in joined:
        sys.exit(code)
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
    handshake = os.environ.get('FAKE_DOCKER_HANDSHAKE')
    if handshake:
        open(handshake + '.ready', 'w').close()
        while not os.path.exists(handshake + '.closed'):
            time.sleep(0.01)
    os.kill(os.getppid(), getattr(signal, os.environ['FAKE_DOCKER_SIGNAL']))
    sys.exit(0)
if arguments[0] == 'run' and 'tar czf' in arguments[-1]:
    mounts = [arguments[index + 1] for index, argument in enumerate(arguments) if argument == '-v']
    backup = next(mount.rsplit(':', 1)[0] for mount in mounts if mount.endswith(':/backup'))
    sys.exit(subprocess.run(['sh', '-c', arguments[-1].replace('/backup/', backup + '/')]).returncode)
sys.exit(0)
'''

FAKE_DATE = f'''#!/bin/sh
echo {DATE}
'''

FAKE_TAR = '''#!/usr/bin/env python3
import os, pathlib, sys, time
if sys.argv[1] != 'czf':
    sys.exit(64)
partial = pathlib.Path(sys.argv[2])
partial.write_text('archive')
if os.environ.get('FAKE_TAR_COLLIDE'):
    (partial.parent / partial.name.removeprefix('.')).write_text('other')
block = os.environ.get('FAKE_TAR_BLOCK')
if block:
    pathlib.Path(block + '.ready').touch()
    while not os.path.exists(block + '.release'):
        time.sleep(0.01)
'''


@dataclass
class Run:
    returncode: int
    output: str
    calls: list[list[str]]
    directory_mode: int | None = None
    files: dict[str, tuple[str, int]] = field(default_factory=dict)

    def indexes(self, predicate: Callable[[list[str]], bool]) -> list[int]:
        return [index for index, call in enumerate(self.calls) if predicate(call)]

    def first(self, predicate: Callable[[list[str]], bool]) -> int:
        found = self.indexes(predicate)
        if not found:
            raise AssertionError(f'no matching docker call in {json.dumps(self.calls, indent=1)}')
        return found[0]

    def last(self, predicate: Callable[[list[str]], bool]) -> int:
        return self.indexes(predicate)[-1]

    def stage(self) -> str:
        return self.calls[self.first(is_stage_create)][2]


def is_image_check(call: list[str]) -> bool:
    return call[:2] == ['image', 'inspect'] and call[-1] == 'alpine'


def is_pull(call: list[str]) -> bool:
    return call == ['pull', 'alpine']


def is_stop(call: list[str]) -> bool:
    return call[0] == 'stop'


def is_start(call: list[str]) -> bool:
    return call[0] == 'start'


def is_exit_code_check(call: list[str]) -> bool:
    return call[0] == 'inspect' and any('.State.ExitCode' in argument for argument in call)


def is_stage_create(call: list[str]) -> bool:
    return call[:2] == ['volume', 'create'] and call[2].startswith('zone_backup_stage_')


def is_stage_remove(call: list[str]) -> bool:
    return call[:2] == ['volume', 'rm'] and call[2].startswith('zone_backup_stage_')


def is_stage_copy(call: list[str]) -> bool:
    return call[0] == 'run' and 'cp' in call and call[call.index('cp') + 1] == '-a'


def is_archive(call: list[str]) -> bool:
    return call[0] == 'run' and any('tar czf' in argument for argument in call)


def is_extract(call: list[str]) -> bool:
    return call[0] == 'run' and any('tar xzf' in argument for argument in call)


def mounts(call: list[str]) -> list[str]:
    return [call[index + 1] for index, argument in enumerate(call) if argument == '-v']


def names(calls: list[list[str]]) -> set[str]:
    created = {call[2] for call in calls if is_stage_create(call)}
    workers = {call[index + 1] for call in calls for index, argument in enumerate(call) if argument == '--name'}
    return created | workers


def removals(calls: list[list[str]]) -> set[str]:
    removed = {call[-1] for call in calls if call[:2] == ['rm', '-f']}
    return removed | {call[2] for call in calls if is_stage_remove(call)}


def partial_of(calls: list[list[str]]) -> str:
    script = next(call[-1] for call in calls if is_archive(call))
    return re.search(r'tar czf /backup/(\S+)', script).group(1)


def read_files(folder: Path) -> dict[str, tuple[str, int]]:
    if not folder.exists():
        return {}
    return {path.name: (path.read_text(), path.stat().st_mode & 0o777) for path in folder.iterdir()}


@dataclass
class Fake:
    running: str = POSTGRES
    cluster: bool = True
    exit_code: str = '0'
    fail: dict[str, int] = field(default_factory=dict)
    signal_on: str = ''
    signal: str = 'SIGTERM'
    closed_output: bool = False
    tar_block: str = ''
    tar_collide: bool = False


class Sandbox:
    def __init__(self, folder: Path) -> None:
        self.folder = folder
        self.bin = folder / 'bin'
        self.bin.mkdir()
        for name, source in [('docker', FAKE_DOCKER), ('date', FAKE_DATE), ('tar', FAKE_TAR)]:
            command = self.bin / name
            command.write_text(source)
            command.chmod(0o700)
        self.work = folder / 'work'
        self.work.mkdir()
        self.backups = self.work / 'backups'

    def environment(self, fake: Fake, log: Path) -> dict[str, str]:
        log.touch()
        environment = os.environ | {
            'PATH': f'{self.bin}:{os.environ["PATH"]}',
            'FAKE_DOCKER_LOG': str(log),
            'FAKE_DOCKER_PS': fake.running,
            'FAKE_DOCKER_CLUSTER': '1' if fake.cluster else '0',
            'FAKE_DOCKER_EXIT_CODE': fake.exit_code,
            'FAKE_DOCKER_FAIL': json.dumps(fake.fail),
            'FAKE_DOCKER_SIGNAL_ON': fake.signal_on,
            'FAKE_DOCKER_SIGNAL': fake.signal,
            'FAKE_TAR_BLOCK': fake.tar_block,
            'FAKE_TAR_COLLIDE': '1' if fake.tar_collide else '',
        }
        environment.pop('ALLOW_EMPTY_POSTGRES', None)
        return environment

    def command(self, target: str, shell: str, *variables: str) -> list[str]:
        return ['make', '-f', str(MAKEFILE), '-C', str(self.work), target, f'SHELL={shell}', *variables]


def read_calls(log: Path) -> list[list[str]]:
    return [json.loads(line) for line in log.read_text().splitlines()]


class MakeTarget(unittest.TestCase):
    def make(self, target: str, fake: Fake, shell: str, *variables: str) -> Run:
        with tempfile.TemporaryDirectory(prefix='zone-backup-command-') as directory:
            sandbox = Sandbox(Path(directory))
            log = sandbox.folder / 'calls.jsonl'
            environment = sandbox.environment(fake, log)
            command = sandbox.command(target, shell, *variables)
            if fake.closed_output:
                handshake = sandbox.folder / 'handshake'
                environment['FAKE_DOCKER_HANDSHAKE'] = str(handshake)
                returncode, output = self.run_with_output_closed_on_signal(command, environment, handshake)
            else:
                result = subprocess.run(command, env=environment, text=True, capture_output=True, timeout=30)
                returncode, output = result.returncode, result.stdout + result.stderr
            mode = sandbox.backups.stat().st_mode & 0o777 if sandbox.backups.exists() else None
            return Run(returncode, output, read_calls(log), mode, read_files(sandbox.backups))

    def run_with_output_closed_on_signal(self, command: list[str], environment: dict[str, str],
                                         handshake: Path) -> tuple[int, str]:
        reader, writer = os.pipe()
        with tempfile.TemporaryFile(mode='w+') as errors:
            process = subprocess.Popen(command, env=environment, text=True, stdout=writer, stderr=errors)
            os.close(writer)
            ready = handshake.with_name(handshake.name + '.ready')
            try:
                deadline = time.monotonic() + 30
                while not ready.exists() and process.poll() is None:
                    if time.monotonic() > deadline:
                        raise AssertionError('the signalled docker call never ran')
                    time.sleep(0.01)
            finally:
                os.close(reader)
                handshake.with_name(handshake.name + '.closed').touch()
            returncode = process.wait(timeout=30)
            errors.seek(0)
            return returncode, errors.read()


class Backup(MakeTarget):
    def assertRestartedAndStageRemoved(self, run: Run, after: Callable[[list[str]], bool]) -> None:
        self.assertNotEqual(run.returncode, 0)
        self.assertEqual(run.calls[run.first(is_start)], ['start', POSTGRES])
        self.assertLess(run.first(after), run.first(is_start))
        self.assertEqual(len(run.indexes(is_start)), 1)
        self.assertEqual(run.calls[run.first(is_stage_remove)], ['volume', 'rm', run.stage()])
        self.assertLess(run.first(after), run.first(is_stage_remove))
        self.assertNotIn('Backup created', run.output)

    def test_running_postgres_is_stopped_only_while_its_cluster_is_copied(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(), shell)
                self.assertEqual(run.returncode, 0, run.output)
                listing = run.calls[run.first(lambda call: call[0] == 'ps')]
                self.assertIn('label=com.docker.compose.service=postgres', listing)
                self.assertIn('volume=zone_postgres_data', listing)
                image = run.first(is_image_check)
                create = run.first(is_stage_create)
                stop = run.first(is_stop)
                self.assertEqual(run.calls[stop], ['stop', '-t', '120', POSTGRES])
                check = run.first(is_exit_code_check)
                copy = run.first(is_stage_copy)
                start = run.first(is_start)
                self.assertEqual(run.calls[start], ['start', POSTGRES])
                self.assertEqual(run.calls[start - 1], ['stop', '-t', '120', POSTGRES])
                archive = run.first(is_archive)
                remove = run.first(is_stage_remove)
                self.assertEqual([image, create, stop, check, copy, start, archive, remove], sorted(
                    [image, create, stop, check, copy, start, archive, remove]))
                self.assertEqual(len(run.indexes(is_start)), 1)
                self.assertEqual(len(run.indexes(is_archive)), 1)
                self.assertEqual(run.indexes(is_pull), [])
                stage = run.stage()
                self.assertRegex(stage, rf'^zone_backup_stage_{DATE}-[0-9]+$')
                self.assertEqual(len(names(run.calls)), 2, run.calls)
                for name in names(run.calls) - {stage}:
                    self.assertRegex(name, RUN_NAME)
                self.assertEqual(mounts(run.calls[copy]), ['zone_postgres_data:/source:ro', f'{stage}:/stage'])
                self.assertEqual(run.calls[copy][-4:], ['cp', '-a', '/source/.', '/stage/'])
                self.assertEqual(run.calls[remove], ['volume', 'rm', stage])
                archived = mounts(run.calls[archive])
                self.assertIn(f'{stage}:/data/postgres:ro', archived)
                self.assertNotIn('zone_postgres_data:/data/postgres:ro', archived)
                for mount in OTHER_MOUNTS:
                    self.assertIn(mount, archived)
                script = run.calls[archive][-1]
                self.assertIn('tar czf /backup/.zone_backup_', script)
                self.assertIn('-C /data .', script)
                self.assertNotIn('-rf', script)
                self.assertNotIn('gzip', script)
                self.assertEqual(run.directory_mode, 0o700)
                [(archive_name, (content, mode))] = run.files.items()
                self.assertRegex(archive_name, ARCHIVE_NAME)
                self.assertEqual((content, mode), ('archive', 0o600))
                self.assertEqual(partial_of(run.calls), f'.{archive_name}')
                self.assertIn(f'Backup created: backups/{archive_name}', run.output)

    def test_missing_image_is_pulled_before_postgres_stops(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(fail={'image inspect': 1}), shell)
                self.assertEqual(run.returncode, 0, run.output)
                self.assertLess(run.first(is_image_check), run.first(is_pull))
                self.assertLess(run.first(is_pull), run.first(is_stop))

                refused = self.make('backup', Fake(fail={'image inspect': 1, 'pull': 1}), shell)
                self.assertNotEqual(refused.returncode, 0)
                self.assertEqual(refused.indexes(is_stop), [])
                self.assertEqual(refused.indexes(is_start), [])
                self.assertEqual(refused.indexes(is_stage_create), [])

    def test_failed_stage_volume_leaves_postgres_running(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(fail={'volume create': 1}), shell)
                self.assertNotEqual(run.returncode, 0)
                self.assertEqual(run.indexes(is_stop), [])
                self.assertEqual(run.indexes(is_start), [])
                self.assertEqual(run.indexes(is_archive), [])

    def test_failed_stage_copy_still_starts_postgres_and_removes_the_stage(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(fail={'cp -a': 2}), shell)
                self.assertRestartedAndStageRemoved(run, is_stage_copy)
                self.assertEqual(run.indexes(is_archive), [])

    def test_signal_during_stage_copy_still_starts_postgres_and_removes_the_stage(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(signal_on='cp -a'), shell)
                self.assertRestartedAndStageRemoved(run, is_stage_copy)
                self.assertEqual(run.calls[run.first(is_start) - 1], ['stop', '-t', '120', POSTGRES])
                self.assertEqual(run.indexes(is_archive), [])

    def test_signal_during_stage_copy_with_its_output_pipe_gone_still_starts_postgres(self) -> None:
        for shell in SHELLS:
            for name in ['SIGINT', 'SIGTERM', 'SIGHUP', 'SIGQUIT']:
                with self.subTest(shell=shell, signal=name):
                    run = self.make('backup', Fake(signal_on='cp -a', signal=name, closed_output=True), shell)
                    self.assertRestartedAndStageRemoved(run, is_stage_copy)
                    self.assertEqual(run.indexes(is_archive), [])

    def test_quit_during_stage_copy_still_starts_postgres_and_removes_the_stage(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(signal_on='cp -a', signal='SIGQUIT'), shell)
                self.assertRestartedAndStageRemoved(run, is_stage_copy)
                self.assertEqual(run.indexes(is_archive), [])

    def test_failed_archive_removes_the_stage(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(fail={'tar czf': 2}), shell)
                self.assertRestartedAndStageRemoved(run, is_stage_copy)
                self.assertLess(run.first(is_start), run.first(is_archive))
                self.assertLess(run.first(is_archive), run.first(is_stage_remove))

    def test_signal_during_archive_removes_the_stage(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(signal_on='tar czf'), shell)
                self.assertRestartedAndStageRemoved(run, is_stage_copy)
                self.assertLess(run.first(is_archive), run.first(is_stage_remove))

    def test_unclean_shutdown_is_not_copied(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(exit_code='137'), shell)
                self.assertRestartedAndStageRemoved(run, is_exit_code_check)
                self.assertEqual(run.indexes(is_stage_copy), [])
                self.assertEqual(run.indexes(is_archive), [])
                self.assertIn('did not shut down cleanly', run.output)

    def test_failed_restart_points_at_the_container(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(fail={'start': 1}), shell)
                self.assertNotEqual(run.returncode, 0)
                self.assertIn(f"run 'docker start {POSTGRES}'", run.output)
                self.assertNotIn('make up', run.output)
                self.assertEqual(len(run.indexes(is_start)), 1)
                self.assertLess(run.first(is_start), run.first(is_archive))
                self.assertIn('Backup created', run.output)

    def test_existing_archive_is_not_overwritten(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(tar_collide=True), shell)
                self.assertRestartedAndStageRemoved(run, is_stage_copy)
                self.assertLess(run.first(is_archive), run.first(is_stage_remove))
                [(archive_name, (content, _))] = run.files.items()
                self.assertEqual(f'.{archive_name}', partial_of(run.calls))
                self.assertEqual(content, 'other')
                self.assertIn(f'backups/{archive_name} already exists; not overwriting it.', run.output)

    def test_stopped_postgres_is_archived_in_place(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('backup', Fake(running=''), shell)
                self.assertEqual(run.returncode, 0, run.output)
                self.assertEqual(run.indexes(is_stop), [])
                self.assertEqual(run.indexes(is_start), [])
                self.assertEqual(run.indexes(is_stage_create), [])
                self.assertEqual(run.indexes(is_stage_remove), [])
                self.assertIn('zone_postgres_data:/data/postgres:ro', mounts(run.calls[run.first(is_archive)]))

    def test_empty_cluster_volume_is_archived_without_stopping_anything(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                refused = self.make('backup', Fake(cluster=False), shell)
                self.assertNotEqual(refused.returncode, 0)
                self.assertEqual(refused.indexes(is_stop), [])
                self.assertEqual(refused.indexes(is_archive), [])
                self.assertIn('./scripts/compose.sh up -d', refused.output)

                run = self.make('backup', Fake(cluster=False), shell, 'ALLOW_EMPTY_POSTGRES=1')
                self.assertEqual(run.returncode, 0, run.output)
                self.assertEqual(run.indexes(is_stop), [])
                self.assertEqual(run.indexes(is_start), [])
                self.assertEqual(run.indexes(is_stage_create), [])
                self.assertIn('zone_postgres_data:/data/postgres:ro', mounts(run.calls[run.first(is_archive)]))


class ConcurrentBackups(unittest.TestCase):
    def test_backups_started_in_the_same_second_keep_to_their_own_resources(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell), tempfile.TemporaryDirectory(prefix='zone-backup-concurrent-') as directory:
                sandbox = Sandbox(Path(directory))
                handshake = sandbox.folder / 'handshake'
                first_log = sandbox.folder / 'first.jsonl'
                second_log = sandbox.folder / 'second.jsonl'
                first = subprocess.Popen(
                    sandbox.command('backup', shell), env=sandbox.environment(Fake(tar_block=str(handshake)), first_log),
                    text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
                try:
                    deadline = time.monotonic() + 30
                    while not handshake.with_name('handshake.ready').exists():
                        self.assertIsNone(first.poll(), 'the first backup ended before it wrote its archive')
                        self.assertLess(time.monotonic(), deadline, 'the first backup never wrote its archive')
                        time.sleep(0.01)
                    second = subprocess.run(
                        sandbox.command('backup', shell), env=sandbox.environment(Fake(), second_log),
                        text=True, capture_output=True, timeout=30)
                    during = read_files(sandbox.backups)
                finally:
                    handshake.with_name('handshake.release').touch()
                    output, _ = first.communicate(timeout=30)
                self.assertEqual(second.returncode, 0, second.stdout + second.stderr)
                self.assertEqual(first.returncode, 0, output)
                first_calls = read_calls(first_log)
                second_calls = read_calls(second_log)
                first_partial = partial_of(first_calls)
                second_partial = partial_of(second_calls)
                self.assertNotEqual(first_partial, second_partial)
                self.assertIn(first_partial, during, "the second backup removed the first one's partial archive")
                self.assertIn(second_partial.removeprefix('.'), during)
                self.assertTrue(names(first_calls).isdisjoint(names(second_calls)), (first_calls, second_calls))
                self.assertLessEqual(removals(first_calls), names(first_calls))
                self.assertLessEqual(removals(second_calls), names(second_calls))
                self.assertEqual(len(removals(second_calls)), 2, second_calls)
                archives = read_files(sandbox.backups)
                expected = [first_partial.removeprefix('.'), second_partial.removeprefix('.')]
                self.assertEqual(sorted(archives), sorted(expected))
                for name, (content, _) in archives.items():
                    self.assertRegex(name, ARCHIVE_NAME)
                    self.assertEqual(content, 'archive')


class Restore(MakeTarget):
    def test_restore_refuses_while_a_zone_volume_is_mounted(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('restore', Fake(), shell, BACKUP)
                self.assertNotEqual(run.returncode, 0)
                self.assertEqual(run.indexes(is_extract), [])
                self.assertIn('Stop the stack first: make stop', run.output)
                listing = run.calls[run.first(lambda call: call[0] == 'ps')]
                for volume in ['zone_postgres_data', 'zone_manager_agent_state', 'zone_traefik_letsencrypt']:
                    self.assertIn(f'volume={volume}', listing)

    def test_restore_clears_each_archived_volume_before_extracting(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('restore', Fake(running=''), shell, BACKUP)
                self.assertEqual(run.returncode, 0, run.output)
                self.assertEqual(len(run.indexes(is_extract)), 1)
                call = run.calls[run.first(is_extract)]
                self.assertEqual(call[-1], 'zone_backup_20260930_000000.tar.gz')
                self.assertIn('zone_postgres_data:/data/postgres', mounts(call))
                self.assertEqual(len(mounts(call)), 10)
                script = call[call.index('-ec') + 1]
                listing = script.index('tar tzf')
                clearing = script.index('find "$directory" -mindepth 1 -delete')
                extraction = script.index('tar xzf')
                self.assertLess(listing, clearing)
                self.assertLess(clearing, extraction)


class RestoreScript(unittest.TestCase):
    def restore(self, archive_tree: dict[str, str | None], volumes: dict[str, str]) -> tuple[Path, str]:
        run = subprocess.run(['make', '-n', '-f', str(MAKEFILE), 'restore', BACKUP],
                             text=True, capture_output=True, check=True)
        command = next(line for line in run.stdout.split('docker run')[1:] if 'tar xzf' in line)
        script = command[command.index("-ec '") + 5:command.rindex("' sh ")].replace('\\\n', '')
        folder = Path(tempfile.mkdtemp(prefix='zone-restore-script-'))
        self.addCleanup(shutil.rmtree, folder)
        source = folder / 'source'
        for path, content in archive_tree.items():
            if content is None:
                (source / path).mkdir(parents=True, exist_ok=True)
                continue
            (source / path).parent.mkdir(parents=True, exist_ok=True)
            (source / path).write_text(content)
        backup = folder / 'backup'
        backup.mkdir()
        subprocess.run(['tar', 'czf', str(backup / 'archive.tar.gz'), '-C', str(source), '.'], check=True)
        data = folder / 'data'
        for path, content in volumes.items():
            (data / path).parent.mkdir(parents=True, exist_ok=True)
            (data / path).write_text(content)
        script = script.replace('/backup/', f'{backup}/').replace('/data', str(data)).replace(
            '/tmp/entries', str(folder / 'entries'))
        result = subprocess.run(['sh', '-ec', script, 'sh', 'archive.tar.gz'], text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        return data, result.stdout + result.stderr

    def test_archived_volumes_lose_files_written_after_the_backup(self) -> None:
        data, _ = self.restore(
            {'postgres/PG_VERSION': '16', 'postgres/base/1': 'row', 'valkey/dump.rdb': 'old', 'grafana/': None},
            {'postgres/PG_VERSION': '16', 'postgres/base/1_vm': 'stale', 'valkey/dump.rdb': 'new',
             'valkey/temp.rdb': 'stale', 'grafana/grafana.db': 'new', 'unlisted/keep': 'kept'})
        self.assertFalse((data / 'postgres/base/1_vm').exists())
        self.assertEqual((data / 'postgres/base/1').read_text(), 'row')
        self.assertEqual((data / 'valkey/dump.rdb').read_text(), 'old')
        self.assertFalse((data / 'valkey/temp.rdb').exists())
        self.assertEqual(list((data / 'grafana').iterdir()), [])
        self.assertEqual((data / 'unlisted/keep').read_text(), 'kept')

    def test_archive_without_a_cluster_keeps_the_current_one(self) -> None:
        data, output = self.restore(
            {'postgres/data/': None, 'valkey/dump.rdb': 'old'},
            {'postgres/PG_VERSION': '16', 'valkey/dump.rdb': 'new'})
        self.assertEqual((data / 'postgres/PG_VERSION').read_text(), '16')
        self.assertEqual((data / 'valkey/dump.rdb').read_text(), 'old')
        self.assertIn('carries no postgres cluster', output)


if __name__ == '__main__':
    unittest.main()
