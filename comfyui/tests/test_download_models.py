from __future__ import annotations

import hashlib
import importlib.util
import sys
import tempfile
import threading
import unittest
import unittest.mock
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

MODULE_PATH = Path(__file__).parents[1] / "download-models.py"
MANIFEST_PATH = Path(__file__).parents[1] / "model-manifest.json"
SPEC = importlib.util.spec_from_file_location("download_models", MODULE_PATH)
assert SPEC and SPEC.loader
download_models = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(download_models)


class PayloadHandler(BaseHTTPRequestHandler):
    payload = b""
    honor_range = True

    def do_GET(self) -> None:
        offset = 0
        range_header = self.headers.get("Range")
        if self.honor_range and range_header:
            offset = int(range_header.removeprefix("bytes=").removesuffix("-"))
            self.send_response(206)
            self.send_header(
                "Content-Range",
                f"bytes {offset}-{len(self.payload) - 1}/{len(self.payload)}",
            )
        else:
            self.send_response(200)
        body = self.payload[offset:]
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args: object) -> None:
        pass


class DownloadModelsTest(unittest.TestCase):
    def model(self, url: str, payload: bytes) -> dict[str, object]:
        return {
            "id": "fixture",
            "url": url,
            "size_bytes": len(payload),
            "sha256": hashlib.sha256(payload).hexdigest(),
        }

    def serve(self, payload: bytes, honor_range: bool = True):
        handler = type(
            "Handler",
            (PayloadHandler,),
            {"payload": payload, "honor_range": honor_range},
        )
        server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        return f"http://127.0.0.1:{server.server_port}/model"

    def test_verify_checks_size_and_digest(self) -> None:
        payload = b"verified model fixture"
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "model.bin"
            target.write_bytes(payload)
            model = self.model("http://invalid", payload)
            self.assertEqual(download_models.verify(target, model), (True, "verified"))

            target.write_bytes(b"x" * len(payload))
            valid, detail = download_models.verify(target, model)
            self.assertFalse(valid)
            self.assertIn("SHA-256 mismatch", detail)

    def test_target_cannot_escape_models_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(ValueError):
                download_models.checked_target(Path(directory), "../outside")

    def test_select_models_filters_bundle(self) -> None:
        models = [
            {"id": "image", "bundle": "image"},
            {"id": "image-edit", "bundle": "image-edit"},
            {"id": "video", "bundle": "video"},
            {"id": "audio", "bundle": "audio"},
            {"id": "legacy"},
        ]
        self.assertEqual(
            [model["id"] for model in download_models.select_models(models, "image")],
            ["image", "legacy"],
        )
        self.assertEqual(
            [model["id"] for model in download_models.select_models(models, "image-edit")],
            ["image-edit"],
        )
        self.assertEqual(
            [model["id"] for model in download_models.select_models(models, "video")],
            ["video"],
        )
        self.assertEqual(
            [model["id"] for model in download_models.select_models(models, "audio")],
            ["audio"],
        )
        self.assertEqual(len(download_models.select_models(models, "all")), 5)

    def test_parse_args_accepts_every_valid_bundle(self) -> None:
        self.assertEqual(
            download_models.VALID_BUNDLES,
            {"audio", "image", "image-dev", "image-edit", "video"},
        )
        for bundle in [*sorted(download_models.VALID_BUNDLES), "all"]:
            with self.subTest(bundle=bundle):
                argv = [
                    "download-models.py",
                    "--models-dir",
                    ".",
                    "--bundle",
                    bundle,
                ]
                with unittest.mock.patch.object(sys, "argv", argv):
                    self.assertEqual(download_models.parse_args().bundle, bundle)

    def test_shipped_manifest_can_be_verified(self) -> None:
        models = download_models.load_manifest(MODULE_PATH.with_name("model-manifest.json"))
        identifiers = [model["id"] for model in models]
        self.assertEqual(len(identifiers), len(set(identifiers)), "duplicate model id")
        for model in models:
            with self.subTest(model=model["id"]):
                # verify() indexes these, so a missing one is a crash at setup.
                self.assertIn(download_models.model_bundle(model), download_models.VALID_BUNDLES)
                self.assertIsInstance(model["size_bytes"], int)
                self.assertGreater(model["size_bytes"], 0)
                self.assertRegex(str(model["sha256"]), r"^[0-9a-f]{64}$")
                self.assertIn(model["source_revision"], model["url"])
                self.assertTrue(str(model["url"]).startswith("https://huggingface.co/"))
                self.assertTrue(
                    str(model["relative_path"]).endswith(str(model["filename"])),
                    "relative_path must land on the declared filename",
                )

    def test_download_resumes_partial_file(self) -> None:
        payload = b"0123456789" * 1000
        url = self.serve(payload)
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "model.bin"
            target.with_name("model.bin.part").write_bytes(payload[:317])
            model = self.model(url, payload)

            download_models.download(model, target)

            self.assertEqual(target.read_bytes(), payload)

    def test_download_restarts_if_server_ignores_range(self) -> None:
        payload = b"abcdefghij" * 1000
        url = self.serve(payload, honor_range=False)
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "model.bin"
            target.with_name("model.bin.part").write_bytes(payload[:129])
            model = self.model(url, payload)

            download_models.download(model, target)

            self.assertEqual(target.read_bytes(), payload)


class ModelManifestTest(unittest.TestCase):
    REQUIRED_KEYS = (
        "id",
        "bundle",
        "filename",
        "relative_path",
        "url",
        "size_bytes",
        "sha256",
        "license",
    )

    def setUp(self) -> None:
        self.models = download_models.load_manifest(MANIFEST_PATH)
        self.assertTrue(self.models, f"no models declared in {MANIFEST_PATH}")

    def test_every_entry_declares_a_supported_bundle(self) -> None:
        for model in self.models:
            with self.subTest(model=model.get("id")):
                self.assertIn(
                    download_models.model_bundle(model),
                    {"audio", "image", "image-dev", "image-edit", "video"},
                )

    def test_every_bundle_selects_at_least_one_entry(self) -> None:
        for bundle in sorted(download_models.VALID_BUNDLES):
            with self.subTest(bundle=bundle):
                self.assertTrue(
                    download_models.select_models(self.models, bundle),
                    f"bundle {bundle} selects no models",
                )

    def test_every_entry_declares_the_required_keys(self) -> None:
        for model in self.models:
            with self.subTest(model=model.get("id")):
                missing = [key for key in self.REQUIRED_KEYS if not model.get(key)]
                self.assertEqual(missing, [], f"missing keys: {missing}")


if __name__ == "__main__":
    unittest.main()
