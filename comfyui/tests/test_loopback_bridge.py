"""The loopback bridge checks the token in the header Zone sends it in."""

from __future__ import annotations

import http.client
import importlib.util
import os
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from unittest import mock

BRIDGE = Path(__file__).parents[2] / 'scripts' / 'comfyui-loopback-bridge.py'
TOKEN = 'a' * 32


def load_bridge():
    specification = importlib.util.spec_from_file_location('loopback_bridge', BRIDGE)
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


bridge = load_bridge()


class Upstream(BaseHTTPRequestHandler):
    received: list[dict[str, str]] = []

    def do_GET(self) -> None:
        Upstream.received.append({name.lower(): value for name, value in self.headers.items()})
        self.send_response(200)
        self.send_header('Content-Length', '2')
        self.end_headers()
        self.wfile.write(b'ok')

    def log_message(self, *arguments: object) -> None:
        pass


class Bridge(bridge.BridgeHandler):
    def log_message(self, *arguments: object) -> None:
        pass


def serve(server: ThreadingHTTPServer) -> ThreadingHTTPServer:
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


class LoopbackBridgeTests(unittest.TestCase):
    def setUp(self) -> None:
        Upstream.received = []
        self.upstream = serve(ThreadingHTTPServer(('127.0.0.1', 0), Upstream))
        self.addCleanup(self.upstream.server_close)
        self.addCleanup(self.upstream.shutdown)

    def bridged(self, token_header: str) -> ThreadingHTTPServer:
        server = ThreadingHTTPServer(('127.0.0.1', 0), Bridge)
        server.token = TOKEN
        server.token_header = token_header
        server.upstream_host, server.upstream_port = self.upstream.server_address
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        return serve(server)

    def status(self, server: ThreadingHTTPServer, headers: dict[str, str]) -> int:
        host, port = server.server_address
        connection = http.client.HTTPConnection(host, port, timeout=10)
        try:
            connection.request('GET', '/system_stats', headers=headers)
            response = connection.getresponse()
            response.read()
            return response.status
        finally:
            connection.close()

    def test_the_token_is_checked_in_the_configured_header_and_not_forwarded(self) -> None:
        server = self.bridged('X-Proxy-Token')
        self.assertEqual(self.status(server, {'X-Proxy-Token': TOKEN}), 200)
        self.assertEqual(len(Upstream.received), 1)
        self.assertNotIn('x-proxy-token', Upstream.received[0])

    def test_the_default_header_is_refused_once_another_is_configured(self) -> None:
        server = self.bridged('X-Proxy-Token')
        self.assertEqual(self.status(server, {'X-Zone-ComfyUI-Token': TOKEN}), 403)
        self.assertEqual(Upstream.received, [])

    def test_the_header_comes_from_the_environment_with_zones_as_the_default(self) -> None:
        with mock.patch.dict(os.environ, {'COMFYUI_TOKEN_HEADER': ' X-Proxy-Token '}):
            self.assertEqual(bridge.configured_token_header(), 'X-Proxy-Token')
        with mock.patch.dict(os.environ, {'COMFYUI_TOKEN_HEADER': ''}):
            self.assertEqual(bridge.configured_token_header(), 'X-Zone-ComfyUI-Token')


if __name__ == '__main__':
    unittest.main()
