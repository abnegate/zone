from __future__ import annotations

import hashlib
import importlib.util
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

MODULE_PATH = Path(__file__).parents[1] / "download-models.py"
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
            {"id": "video", "bundle": "video"},
            {"id": "legacy"},
        ]
        self.assertEqual(
            [model["id"] for model in download_models.select_models(models, "image")],
            ["image", "legacy"],
        )
        self.assertEqual(
            [model["id"] for model in download_models.select_models(models, "video")],
            ["video"],
        )
        self.assertEqual(len(download_models.select_models(models, "all")), 3)

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


if __name__ == "__main__":
    unittest.main()
