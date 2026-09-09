#!/usr/bin/env python3
"""Verify migration command failures and reset database identity without Docker."""

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


class Migrations(unittest.TestCase):
    def run_make(self, target: str) -> tuple[subprocess.CompletedProcess[str], list[list[str]]]:
        root = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory(prefix='zone-migration-command-') as directory:
            folder = Path(directory)
            log = folder / 'calls.jsonl'
            command = folder / 'docker'
            command.write_text('''#!/usr/bin/env python3
import json, os, sys
with open(os.environ['MIGRATION_CALLS'], 'a') as output:
    output.write(json.dumps(sys.argv[1:]) + '\\n')
if 'config' in sys.argv:
    print(json.dumps({'services': {'manager': {'environment': {'POSTGRES_DB': 'manager "quoted" database'}}}}))
    sys.exit(0)
if 'run' in sys.argv or ('exec' in sys.argv and 'psql' in sys.argv):
    sys.exit(19)
sys.exit(0)
''')
            command.chmod(0o700)
            environment = os.environ | {
                'PATH': f'{folder}:{os.environ["PATH"]}',
                'MIGRATION_CALLS': str(log),
                'POSTGRES_DB': 'unrelated',
            }
            result = subprocess.run(
                ['make', '-f', os.environ.get('ZONE_MAKEFILE', 'Makefile'), target, f'COMPOSE={command}'],
                cwd=root, env=environment, input='yes\n', text=True, capture_output=True, timeout=10,
            )
            calls = [json.loads(line) for line in log.read_text().splitlines()]
            return result, calls

    def test_migration_failure_propagates(self) -> None:
        result, calls = self.run_make('db-migrate')
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn('Migrations complete!', result.stdout)
        self.assertTrue(any('--migrate-only' in ' '.join(call) and '--build' in call for call in calls))

    def test_reset_uses_manager_database_and_propagates_migration_failure(self) -> None:
        result, calls = self.run_make('db-reset')
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn('Database reset complete!', result.stdout)
        resets = [call for call in calls if 'exec' in call]
        self.assertEqual(len(resets), 2)
        for call in resets:
            self.assertEqual(call[-1], 'manager "quoted" database')
        self.assertTrue(any('--migrate-only' in ' '.join(call) and '--build' in call for call in calls))


if __name__ == '__main__':
    unittest.main()
