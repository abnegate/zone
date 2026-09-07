from __future__ import annotations

import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

COMFYUI_DIR = Path(__file__).parents[1]
MODULE_PATH = COMFYUI_DIR / "download-models.py"
SCRIPT = COMFYUI_DIR.parent / "scripts" / "setup-comfyui-macos.sh"

SPEC = importlib.util.spec_from_file_location("download_models", MODULE_PATH)
assert SPEC and SPEC.loader
download_models = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(download_models)

MANIFEST_BUNDLES = list(
    dict.fromkeys(
        download_models.model_bundle(model)
        for model in download_models.load_manifest(MODULE_PATH.with_name("model-manifest.json"))
    )
)

# The installer refuses to run off Apple Silicon, so the platform probe and the
# interpreter are stubbed to reach the argument handling on any CI runner.
UNAME_STUB = """#!/bin/sh
case "$1" in
    -s) echo Darwin ;;
    -m) echo arm64 ;;
    *) exit 1 ;;
esac
"""

PYTHON_STUB = """#!/bin/sh
if [ "$1" = "-" ]; then
    if [ "$#" -eq 1 ]; then
        cat >/dev/null
        exit 0
    fi
    exec '@PYTHON@' "$@"
fi
printf '%s\\n' "$@" > '@RECORD@'
"""


class SetupMacosTest(unittest.TestCase):
    def script(self, *arguments: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(["sh", str(SCRIPT), *arguments], capture_output=True, text=True)

    def stub(self, path: Path, body: str) -> None:
        path.write_text(body, encoding="utf-8")
        path.chmod(0o755)

    def selected_bundle(self, *arguments: str) -> str:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            record = root / "argv"
            self.stub(root / "uname", UNAME_STUB)
            self.stub(
                root / "python",
                PYTHON_STUB.replace("@PYTHON@", sys.executable).replace("@RECORD@", str(record)),
            )
            result = subprocess.run(
                ["sh", str(SCRIPT), *arguments],
                capture_output=True,
                text=True,
                env={
                    **os.environ,
                    "PATH": f"{root}{os.pathsep}{os.environ['PATH']}",
                    "PYTHON_BIN": str(root / "python"),
                    "COMFYUI_INSTALL_DIR": str(root / "runtime"),
                    "COMFYUI_MODELS_DIR": str(root / "models"),
                },
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            recorded = record.read_text(encoding="utf-8").splitlines()
        self.assertIn("--verify-only", recorded)
        self.assertIn("--bundle", recorded, "the downloader was called without a bundle")
        return recorded[recorded.index("--bundle") + 1]

    def test_every_manifest_bundle_is_accepted(self) -> None:
        for bundle in MANIFEST_BUNDLES:
            with self.subTest(bundle=bundle):
                result = self.script("--bundle", bundle, "--help")
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_every_manifest_bundle_reaches_the_downloader(self) -> None:
        for bundle in MANIFEST_BUNDLES:
            with self.subTest(bundle=bundle):
                self.assertEqual(
                    self.selected_bundle("--verify-model", "--bundle", bundle), bundle
                )

    def test_usage_lists_every_manifest_bundle(self) -> None:
        result = self.script("--help")
        self.assertEqual(result.returncode, 0, result.stderr)
        line = next(
            line
            for line in result.stdout.splitlines()
            if line.strip().startswith("--bundle NAME")
        )
        for bundle in MANIFEST_BUNDLES:
            self.assertIn(bundle, line.split())

    def test_unknown_bundle_is_rejected(self) -> None:
        result = self.script("--bundle", "bogus")
        self.assertEqual(result.returncode, 2)
        self.assertIn("bogus", result.stderr)
        for bundle in MANIFEST_BUNDLES:
            self.assertIn(bundle, result.stderr.split())

    def test_bundle_requires_a_value(self) -> None:
        self.assertEqual(self.script("--bundle").returncode, 2)

    def test_legacy_flags_keep_their_bundles(self) -> None:
        message = "a bundle the legacy flags select is no longer in the manifest"
        self.assertIn("image", MANIFEST_BUNDLES, message)
        self.assertIn("video", MANIFEST_BUNDLES, message)
        self.assertEqual(self.selected_bundle("--verify-model"), "image")
        self.assertEqual(self.selected_bundle("--verify-video-model"), "video")
        for flag in ("--download-model", "--download-video-model"):
            with self.subTest(flag=flag):
                self.assertEqual(self.script(flag, "--help").returncode, 0)

    def test_last_bundle_selection_wins(self) -> None:
        self.assertEqual(
            self.selected_bundle("--verify-video-model", "--bundle", "image"), "image"
        )
        self.assertEqual(
            self.selected_bundle("--bundle", "image", "--verify-video-model"), "video"
        )


if __name__ == "__main__":
    unittest.main()
