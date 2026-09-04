#!/usr/bin/env python3
"""Authenticated bridge from Docker Desktop to loopback-only ComfyUI."""

from __future__ import annotations

import argparse
import hmac
import http.client
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MAX_REQUEST_BYTES = 1_048_576
HOP_BY_HOP = {
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
}


class BridgeHandler(BaseHTTPRequestHandler):
    server_version = "ZoneComfyUIBridge/1"

    def do_GET(self) -> None:
        self._proxy()

    def do_POST(self) -> None:
        self._proxy()

    def _proxy(self) -> None:
        expected = self.server.token  # type: ignore[attr-defined]
        supplied = self.headers.get("X-Zone-ComfyUI-Token", "")
        if not hmac.compare_digest(supplied, expected):
            self.send_error(403)
            return

        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            self.send_error(400)
            return
        if length < 0 or length > MAX_REQUEST_BYTES:
            self.send_error(413)
            return
        body = self.rfile.read(length) if length else None

        connection = http.client.HTTPConnection(
            self.server.upstream_host,  # type: ignore[attr-defined]
            self.server.upstream_port,  # type: ignore[attr-defined]
            timeout=30,
        )
        headers = {
            key: value
            for key, value in self.headers.items()
            if key.lower() not in HOP_BY_HOP
            and key.lower() not in {"host", "x-zone-comfyui-token", "content-length"}
        }
        try:
            connection.request(self.command, self.path, body=body, headers=headers)
            response = connection.getresponse()
            self.send_response(response.status)
            for key, value in response.getheaders():
                if key.lower() not in HOP_BY_HOP and key.lower() != "content-length":
                    self.send_header(key, value)
            self.end_headers()
            while chunk := response.read(64 * 1024):
                self.wfile.write(chunk)
        except (OSError, http.client.HTTPException):
            self.send_error(502)
        finally:
            connection.close()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--listen-host", default="0.0.0.0")
    parser.add_argument("--listen-port", type=int, default=8189)
    parser.add_argument("--upstream-host", default="127.0.0.1")
    parser.add_argument("--upstream-port", type=int, default=8188)
    args = parser.parse_args()

    token = os.environ.get("COMFYUI_BRIDGE_TOKEN", "")
    if len(token) < 32:
        raise SystemExit("COMFYUI_BRIDGE_TOKEN must contain at least 32 characters")

    server = ThreadingHTTPServer((args.listen_host, args.listen_port), BridgeHandler)
    server.token = token
    server.upstream_host = args.upstream_host
    server.upstream_port = args.upstream_port
    server.serve_forever()


if __name__ == "__main__":
    main()
