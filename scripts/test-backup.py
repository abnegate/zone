#!/usr/bin/env python3
"""Verify make backup copies the postgres cluster stopped and archives in one pass, and make restore replaces volumes only with the stack stopped, without Docker.

ZONE_TEST_DOCKER=1 also runs backup and restore against throwaway Docker volumes.
"""

import io
import json
import os
import re
import shutil
import subprocess
import tarfile
import tempfile
import time
import unittest
import uuid
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
PREVIOUS = '.zone-restore-previous'

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
                self.assertIn(f"--exclude='./*/{PREVIOUS}'", script)
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

    def test_restore_sets_each_archived_volume_aside_before_extracting(self) -> None:
        for shell in SHELLS:
            with self.subTest(shell=shell):
                run = self.make('restore', Fake(running=''), shell, BACKUP)
                self.assertEqual(run.returncode, 0, run.output)
                self.assertEqual(len(run.indexes(is_extract)), 1)
                call = run.calls[run.first(is_extract)]
                self.assertEqual(call[-1], 'zone_backup_20260930_000000.tar.gz')
                self.assertIn('zone_postgres_data:/data/postgres', mounts(call))
                self.assertEqual(len(mounts(call)), 10)
                script = call[call.index('-c') + 1]
                listing = script.index('tar tzf')
                aside = script.index('set_aside "$directory" || exit 1')
                extraction = script.index('tar xzf')
                self.assertLess(listing, aside)
                self.assertLess(aside, extraction)
                self.assertNotIn('-delete', script)


Tree = dict[str, str | None]


def write_tree(root: Path, tree: Tree) -> None:
    for path, content in tree.items():
        if content is None:
            (root / path).mkdir(parents=True, exist_ok=True)
            continue
        (root / path).parent.mkdir(parents=True, exist_ok=True)
        (root / path).write_text(content)


def read_tree(root: Path) -> Tree:
    return {
        str(path.relative_to(root)): None if path.is_dir() else path.read_text()
        for path in sorted(root.rglob('*'))
    }


def restore_script() -> str:
    run = subprocess.run(['make', '-n', '-f', str(MAKEFILE), 'restore', BACKUP],
                         text=True, capture_output=True, check=True)
    command = next(line for line in run.stdout.split('docker run')[1:] if 'tar xzf' in line)
    return command[command.index("sh -c '") + 7:command.rindex("' sh ")].replace('\\\n', '')


FAILING_TAR = '''#!/bin/sh
if [ "$1" = xzf ]; then
    "$REAL_TAR" "$@"
    echo partial > "$FAILING_TAR_STRAY"
    exit 1
fi
exec "$REAL_TAR" "$@"
'''


class RestoreScript(unittest.TestCase):
    def restore(self, archive: Tree, volumes: Tree, succeeds: bool = True,
                failing_extraction: bool = False) -> tuple[Path, str]:
        folder = Path(tempfile.mkdtemp(prefix='zone-restore-script-'))
        self.addCleanup(shutil.rmtree, folder)
        source = folder / 'source'
        source.mkdir()
        write_tree(source, archive)
        backup = folder / 'backup'
        backup.mkdir()
        subprocess.run(['tar', 'czf', str(backup / 'archive.tar.gz'), '-C', str(source), '.'], check=True)
        data = folder / 'data'
        data.mkdir()
        write_tree(data, volumes)
        script = restore_script().replace('/backup/', f'{backup}/').replace('/data', str(data)).replace(
            '/tmp/entries', str(folder / 'entries'))
        environment = dict(os.environ)
        if failing_extraction:
            bin_directory = folder / 'bin'
            bin_directory.mkdir()
            (bin_directory / 'tar').write_text(FAILING_TAR)
            (bin_directory / 'tar').chmod(0o700)
            environment |= {'PATH': f'{bin_directory}:{os.environ["PATH"]}', 'REAL_TAR': shutil.which('tar'),
                            'FAILING_TAR_STRAY': str(data / 'valkey/partial')}
        result = subprocess.run(['sh', '-c', script, 'sh', 'archive.tar.gz'], text=True, capture_output=True,
                                env=environment)
        self.assertEqual(result.returncode == 0, succeeds, result.stdout + result.stderr)
        return data, result.stdout + result.stderr

    def test_archived_volumes_lose_files_written_after_the_backup(self) -> None:
        data, _ = self.restore(
            {'postgres/PG_VERSION': '16', 'postgres/base/1': 'row', 'valkey/dump.rdb': 'old', 'grafana/': None},
            {'postgres/PG_VERSION': '16', 'postgres/base/1_vm': 'stale', 'valkey/dump.rdb': 'new',
             'valkey/temp.rdb': 'stale', 'valkey/.hidden': 'stale', 'grafana/grafana.db': 'new',
             'unlisted/keep': 'kept'})
        self.assertEqual(read_tree(data), {
            'grafana': None,
            'postgres': None, 'postgres/PG_VERSION': '16', 'postgres/base': None, 'postgres/base/1': 'row',
            'unlisted': None, 'unlisted/keep': 'kept',
            'valkey': None, 'valkey/dump.rdb': 'old',
        })

    def test_failed_extraction_puts_every_volume_back(self) -> None:
        volumes: Tree = {'postgres/PG_VERSION': '16', 'postgres/base/1': 'new', 'valkey/dump.rdb': 'new',
                         'valkey/.hidden': 'new', 'valkey/..double': 'new', 'grafana/grafana.db': 'new',
                         'unlisted/keep': 'kept'}
        data, output = self.restore(
            {'postgres/PG_VERSION': '16', 'postgres/base/1': 'old', 'postgres/base/2': 'old',
             'valkey/dump.rdb': 'old', 'grafana/': None},
            volumes, succeeds=False, failing_extraction=True)
        expected = Path(tempfile.mkdtemp(prefix='zone-restore-expected-'))
        self.addCleanup(shutil.rmtree, expected)
        write_tree(expected, volumes)
        self.assertEqual(read_tree(data), read_tree(expected))
        self.assertIn('every volume holds what it held before', output)

    def test_archive_without_a_cluster_keeps_the_current_one(self) -> None:
        data, output = self.restore(
            {'postgres/data/': None, 'postgres/stray': 'old', 'valkey/dump.rdb': 'old'},
            {'postgres/PG_VERSION': '16', 'postgres/base/1': 'row', 'valkey/dump.rdb': 'new'})
        self.assertEqual(read_tree(data), {
            'postgres': None, 'postgres/PG_VERSION': '16', 'postgres/base': None, 'postgres/base/1': 'row',
            'valkey': None, 'valkey/dump.rdb': 'old',
        })
        self.assertIn('carries no postgres cluster', output)

    def test_failed_extraction_keeps_the_current_cluster_when_the_archive_has_none(self) -> None:
        volumes: Tree = {'postgres/PG_VERSION': '16', 'valkey/dump.rdb': 'new'}
        data, _ = self.restore({'postgres/data/': None, 'valkey/dump.rdb': 'old'}, volumes,
                               succeeds=False, failing_extraction=True)
        self.assertEqual(read_tree(data), {'postgres': None, 'postgres/PG_VERSION': '16',
                                           'valkey': None, 'valkey/dump.rdb': 'new'})

    def test_leftover_aside_directory_stops_the_restore_before_anything_moves(self) -> None:
        volumes: Tree = {'postgres/PG_VERSION': '16', 'valkey/dump.rdb': 'new',
                         f'grafana/{PREVIOUS}/grafana.db': 'older', 'grafana/grafana.db': 'new'}
        data, output = self.restore({'postgres/PG_VERSION': '16', 'valkey/dump.rdb': 'old', 'grafana/': None},
                                    volumes, succeeds=False)
        expected = Path(tempfile.mkdtemp(prefix='zone-restore-expected-'))
        self.addCleanup(shutil.rmtree, expected)
        write_tree(expected, volumes)
        self.assertEqual(read_tree(data), read_tree(expected))
        self.assertIn(f'An interrupted restore left grafana/{PREVIOUS}', output)

    def test_archive_carrying_an_aside_directory_is_refused(self) -> None:
        volumes: Tree = {'postgres/PG_VERSION': '16', 'valkey/dump.rdb': 'new'}
        data, output = self.restore({'postgres/PG_VERSION': '16', f'valkey/{PREVIOUS}/dump.rdb': 'old'},
                                    volumes, succeeds=False)
        self.assertEqual(read_tree(data), {'postgres': None, 'postgres/PG_VERSION': '16',
                                           'valkey': None, 'valkey/dump.rdb': 'new'})
        self.assertIn(PREVIOUS, output)


DOCKER_DIRECTORIES = {'postgres': 'postgres_data', 'valkey': 'valkey_data', 'grafana': 'grafana_data'}


@unittest.skipUnless(os.environ.get('ZONE_TEST_DOCKER') == '1',
                     'set ZONE_TEST_DOCKER=1 to run backup and restore against throwaway Docker volumes')
class DockerVolumes(unittest.TestCase):
    def setUp(self) -> None:
        self.prefix = f'zonetest{uuid.uuid4().hex[:12]}'
        self.assertNotEqual(self.prefix, 'zone')
        self.addCleanup(self.remove_volumes)
        self.work = Path(tempfile.mkdtemp(prefix='zone-backup-docker-'))
        self.addCleanup(shutil.rmtree, self.work, True)
        (self.work / 'backups').mkdir(mode=0o700)
        for target in ['backup', f'restore BACKUP=backups/{self.prefix}.tar.gz']:
            planned = subprocess.run(['make', '-n', '-f', str(MAKEFILE), '-C', str(self.work), *target.split(),
                                      f'VOLUME_PREFIX={self.prefix}'], text=True, capture_output=True, check=True)
            self.assertNotRegex(planned.stdout, r'(?<![A-Za-z0-9_])zone_[a-z_]+_(data|repos|artifacts|state|letsencrypt)')

    def remove_volumes(self) -> None:
        listed = subprocess.run(['docker', 'volume', 'ls', '-q', '--filter', f'name={self.prefix}_'],
                                text=True, capture_output=True, check=True).stdout.split()
        owned = [volume for volume in listed if volume.startswith(f'{self.prefix}_')]
        if owned:
            subprocess.run(['docker', 'volume', 'rm', *owned], capture_output=True, check=True)

    def volume(self, directory: str) -> str:
        return f'{self.prefix}_{DOCKER_DIRECTORIES[directory]}'

    def mounts(self) -> list[str]:
        return [argument for directory in DOCKER_DIRECTORIES
                for argument in ['-v', f'{self.volume(directory)}:/data/{directory}']]

    def seed(self, tree: Tree) -> None:
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode='w') as archive:
            add_tree(archive, tree)
        subprocess.run(['docker', 'run', '--rm', '-i', *self.mounts(), 'alpine', 'sh', '-c',
                        'find /data -mindepth 2 -maxdepth 2 -exec rm -rf {} + && tar xf - -C /data'],
                       input=buffer.getvalue(), capture_output=True, check=True)

    def snapshot(self) -> Tree:
        result = subprocess.run(['docker', 'run', '--rm', *self.mounts(), 'alpine', 'tar', 'cf', '-', '-C', '/data',
                                 *DOCKER_DIRECTORIES], capture_output=True, check=True)
        tree: Tree = {}
        with tarfile.open(fileobj=io.BytesIO(result.stdout)) as archive:
            for member in archive.getmembers():
                content = archive.extractfile(member) if member.isfile() else None
                tree[member.name.rstrip('/')] = content.read().decode() if content else None
        return tree

    def make(self, *arguments: str) -> subprocess.CompletedProcess[str]:
        environment = {key: value for key, value in os.environ.items() if not key.startswith('MAKE')}
        environment.pop('ALLOW_EMPTY_POSTGRES', None)
        return subprocess.run(['make', '-f', str(MAKEFILE), '-C', str(self.work), *arguments,
                               f'VOLUME_PREFIX={self.prefix}'], text=True, capture_output=True, timeout=300,
                              env=environment)

    def archives(self) -> list[Path]:
        return sorted((self.work / 'backups').glob('zone_backup_*.tar.gz'))

    def test_round_trip_restores_the_archive_and_leaves_no_aside_directory(self) -> None:
        self.seed({'postgres/PG_VERSION': '16', 'postgres/base/1': 'old', 'valkey/dump.rdb': 'old',
                   'grafana/grafana.db': 'old', f'grafana/{PREVIOUS}/grafana.db': 'older'})
        backup = self.make('backup')
        self.assertEqual(backup.returncode, 0, backup.stdout + backup.stderr)
        [archive] = self.archives()
        with tarfile.open(archive) as opened:
            members = opened.getnames()
        self.assertIn('./grafana/grafana.db', members)
        self.assertFalse([member for member in members if PREVIOUS in member], members)

        self.seed({'postgres/PG_VERSION': '16', 'postgres/base/1': 'new', 'postgres/base/1_vm': 'new',
                   'valkey/dump.rdb': 'new', 'valkey/.hidden': 'new', 'grafana/grafana.db': 'new'})
        restore = self.make('restore', f'BACKUP=backups/{archive.name}')
        self.assertEqual(restore.returncode, 0, restore.stdout + restore.stderr)
        self.assertEqual(self.snapshot(), {
            'postgres': None, 'postgres/PG_VERSION': '16', 'postgres/base': None, 'postgres/base/1': 'old',
            'valkey': None, 'valkey/dump.rdb': 'old', 'grafana': None, 'grafana/grafana.db': 'old',
        })

    def test_truncated_archive_leaves_every_volume_as_it_was(self) -> None:
        self.seed({'postgres/PG_VERSION': '16', 'postgres/base/1': 'old', 'valkey/dump.rdb': os.urandom(1 << 16).hex()})
        backup = self.make('backup')
        self.assertEqual(backup.returncode, 0, backup.stdout + backup.stderr)
        [archive] = self.archives()
        archive.write_bytes(archive.read_bytes()[:archive.stat().st_size // 2])
        current: Tree = {'postgres/PG_VERSION': '16', 'postgres/base/1': 'new', 'valkey/dump.rdb': 'new',
                         'grafana/grafana.db': 'new'}
        self.seed(current)
        before = self.snapshot()
        restore = self.make('restore', f'BACKUP=backups/{archive.name}')
        self.assertNotEqual(restore.returncode, 0, restore.stdout + restore.stderr)
        self.assertEqual(self.snapshot(), before)

    def test_extraction_that_runs_out_of_space_puts_every_volume_back(self) -> None:
        subprocess.run(['docker', 'volume', 'create', '--driver', 'local', '--opt', 'type=tmpfs',
                        '--opt', 'device=tmpfs', '--opt', 'o=size=1m', self.volume('valkey')],
                       capture_output=True, check=True)
        current: Tree = {'postgres/PG_VERSION': '16', 'postgres/base/1': 'new', 'postgres/base/1_vm': 'new',
                         'valkey/dump.rdb': 'new', 'valkey/.hidden': 'new', 'grafana/grafana.db': 'new'}
        self.seed(current)
        before = self.snapshot()
        archive = self.work / 'backups' / f'zone_backup_{self.prefix}.tar.gz'
        with tarfile.open(archive, 'w:gz') as opened:
            add_tree(opened, {'postgres/PG_VERSION': '16', 'postgres/base/1': 'old', 'grafana/grafana.db': 'old',
                              'valkey/dump.rdb': os.urandom(3 << 20).hex()})
        restore = self.make('restore', f'BACKUP=backups/{archive.name}')
        self.assertNotEqual(restore.returncode, 0, restore.stdout + restore.stderr)
        self.assertEqual(self.snapshot(), before)
        self.assertIn('every volume holds what it held before', restore.stderr)


def add_tree(archive: tarfile.TarFile, tree: Tree) -> None:
    directories = {'.'} | {str(Path(path).parent) for path in tree} | {path.rstrip('/') for path, content in tree.items()
                                                                       if content is None}
    expanded = set()
    for directory in directories:
        while directory not in ('', '.'):
            expanded.add(directory)
            directory = str(Path(directory).parent)
    for directory in ['.', *sorted(expanded)]:
        member = tarfile.TarInfo('.' if directory == '.' else f'./{directory}')
        member.type = tarfile.DIRTYPE
        member.mode = 0o755
        archive.addfile(member)
    for path, content in tree.items():
        if content is None:
            continue
        data = content.encode()
        member = tarfile.TarInfo(f'./{path}')
        member.size = len(data)
        member.mode = 0o644
        archive.addfile(member, io.BytesIO(data))


if __name__ == '__main__':
    unittest.main()
