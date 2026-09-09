#!/usr/bin/env python3
"""Exercise dependency license policy with the actual pinned cargo-deny binary."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path


class Dependencies(unittest.TestCase):
    def check_license(
        self, name: str, version: str, license: str | None
    ) -> subprocess.CompletedProcess[str]:
        config = Path(__file__).resolve().parents[1] / 'runner' / 'deny.toml'
        with tempfile.TemporaryDirectory(prefix='zone-dependency-policy-') as directory:
            root = Path(directory)
            manifest = root / 'Cargo.toml'
            content = (
                f'[package]\nname = "{name}"\nversion = "{version}"\n'
                'edition = "2021"\n[workspace]\n'
            )
            if license is not None:
                content = content.replace('[workspace]', f'license = "{license}"\n[workspace]')
            manifest.write_text(content)
            (root / 'src').mkdir()
            (root / 'src' / 'lib.rs').write_text('')
            return subprocess.run(
                [
                    'cargo', 'deny', '--color', 'never', '--offline',
                    '--manifest-path', str(manifest),
                    '--config', str(config), 'check', 'licenses',
                ],
                cwd=root,
                env=os.environ | {'CARGO_TARGET_DIR': str(root / 'target')},
                text=True, capture_output=True, timeout=60,
            )

    def test_reviewed_package_and_version_are_accepted(self) -> None:
        result = self.check_license('quoted_printable', '0.5.2', '0BSD')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('licenses ok', result.stdout)

    def test_unreviewed_version_is_rejected(self) -> None:
        result = self.check_license('quoted_printable', '0.5.3', '0BSD')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('error[rejected]', result.stderr)

    def test_exception_does_not_allow_another_package(self) -> None:
        result = self.check_license('unreviewed', '0.5.2', '0BSD')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('error[rejected]', result.stderr)

    def test_exception_does_not_allow_another_license(self) -> None:
        result = self.check_license('quoted_printable', '0.5.2', 'GPL-3.0-only')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('error[rejected]', result.stderr)

    def test_unlicensed_package_is_rejected(self) -> None:
        result = self.check_license('quoted_printable', '0.5.2', None)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('error[unlicensed]', result.stderr)


if __name__ == '__main__':
    unittest.main()
