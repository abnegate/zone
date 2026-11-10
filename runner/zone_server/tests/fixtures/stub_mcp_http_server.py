#!/usr/bin/env python3
"""A streamable-HTTP MCP server with one tool, for tests that attach a remote
server without depending on anything installed on the machine.

Binds 127.0.0.1 on a free port and prints PORT=<n> as its first stdout line.
POST /mcp answers JSON-RPC with application/json: initialize echoes the
protocol version the client proposed and sets Mcp-Session-Id, tools/list
returns one tool named delegate, and tools/call echoes the instruction back.
Notifications get 202. GET and DELETE /mcp get 405, which a client reads as
"no event stream" and "no session delete". With --token, every request needs
Authorization: Bearer <token> or gets 401.

--self-test drives the server in-process and exits non-zero on any failure.
"""
import argparse
import json
import sys
import threading
import urllib.error
import urllib.request
import uuid
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PATH = "/mcp"
HOST = "127.0.0.1"
SESSION_HEADER = "Mcp-Session-Id"
JSON_TYPE = "application/json"
SELF_TEST_TOKEN = "self-test"

TOOL = {
    "name": "delegate",
    "description": "Hand an instruction to another coding agent and return what it printed.",
    "inputSchema": {
        "type": "object",
        "properties": {"instruction": {"type": "string"}},
        "required": ["instruction"],
    },
}


def result(message):
    method = message.get("method")
    params = message.get("params") or {}
    if method == "initialize":
        return {
            "protocolVersion": params["protocolVersion"],
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "stub", "version": "0.0.1"},
        }
    if method == "tools/list":
        return {"tools": [TOOL]}
    if method == "tools/call":
        instruction = (params.get("arguments") or {}).get("instruction", "")
        return {"content": [{"type": "text", "text": f"delegated:{instruction}"}]}
    return {}


def handler(token):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, format, *args):
            pass

        def send(self, status, body=None, headers=None):
            payload = b"" if body is None else json.dumps(body).encode()
            self.send_response(status)
            for name, value in (headers or {}).items():
                self.send_header(name, value)
            if body is not None:
                self.send_header("Content-Type", JSON_TYPE)
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        def error(self, status, code, message, message_id=None):
            self.send(status, {"jsonrpc": "2.0", "id": message_id, "error": {"code": code, "message": message}})

        def admitted(self):
            if self.path != PATH:
                self.error(HTTPStatus.NOT_FOUND, -32601, "not found")
                return False
            if token is not None and self.headers.get("Authorization") != f"Bearer {token}":
                self.send(
                    HTTPStatus.UNAUTHORIZED,
                    {"jsonrpc": "2.0", "id": None, "error": {"code": -32001, "message": "unauthorized"}},
                    {"WWW-Authenticate": "Bearer"},
                )
                return False
            return True

        def refuse(self):
            if self.admitted():
                self.send(HTTPStatus.METHOD_NOT_ALLOWED, headers={"Allow": "POST"})

        do_GET = refuse
        do_DELETE = refuse

        def do_POST(self):
            length = int(self.headers.get("Content-Length") or 0)
            body = self.rfile.read(length)
            if not self.admitted():
                return
            try:
                message = json.loads(body)
            except ValueError:
                self.error(HTTPStatus.BAD_REQUEST, -32700, "parse error")
                return
            if not isinstance(message, dict):
                self.error(HTTPStatus.BAD_REQUEST, -32600, "invalid request")
                return
            message_id = message.get("id")
            if message_id is None:
                self.send(HTTPStatus.ACCEPTED)
                return
            try:
                answer = result(message)
            except (KeyError, TypeError, AttributeError):
                self.error(HTTPStatus.OK, -32602, "invalid params", message_id)
                return
            headers = {}
            if message.get("method") == "initialize":
                headers[SESSION_HEADER] = uuid.uuid4().hex
            self.send(HTTPStatus.OK, {"jsonrpc": "2.0", "id": message_id, "result": answer}, headers)

    return Handler


def serve(token):
    return ThreadingHTTPServer((HOST, 0), handler(token))


def request(url, method, body=None, token=None, session=None):
    headers = {"Accept": f"{JSON_TYPE}, text/event-stream"}
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = JSON_TYPE
    if token is not None:
        headers["Authorization"] = f"Bearer {token}"
    if session is not None:
        headers[SESSION_HEADER] = session
    try:
        with urllib.request.urlopen(urllib.request.Request(url, data, headers, method=method), timeout=5) as response:
            return response.status, response.headers, response.read()
    except urllib.error.HTTPError as failure:
        return failure.code, failure.headers, failure.read()


def check(condition, label, failures):
    if not condition:
        failures.append(label)


def drive(token):
    server = serve(token)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    url = f"http://{HOST}:{server.server_address[1]}{PATH}"
    failures = []
    try:
        version = "2025-11-25"
        status, headers, body = request(
            url,
            "POST",
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {"protocolVersion": version, "capabilities": {}, "clientInfo": {"name": "self-test", "version": "1"}},
            },
            token,
        )
        check(status == 200, f"initialize status {status}", failures)
        session = headers.get(SESSION_HEADER)
        check(bool(session), "initialize session header", failures)
        check(headers.get("Content-Type") == JSON_TYPE, "initialize content type", failures)
        check(json.loads(body)["result"]["protocolVersion"] == version, "initialize protocol version", failures)

        status, _, body = request(url, "POST", {"jsonrpc": "2.0", "method": "notifications/initialized"}, token, session)
        check(status == 202 and body == b"", f"initialized status {status} body {body!r}", failures)

        status, _, body = request(url, "POST", {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}, token, session)
        check(status == 200 and json.loads(body)["result"]["tools"] == [TOOL], f"tools/list status {status}", failures)

        status, _, body = request(
            url,
            "POST",
            {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "delegate", "arguments": {"instruction": "ping"}}},
            token,
            session,
        )
        check(
            status == 200 and json.loads(body)["result"]["content"] == [{"type": "text", "text": "delegated:ping"}],
            f"tools/call status {status} body {body!r}",
            failures,
        )

        for method in ("GET", "DELETE"):
            status, _, _ = request(url, method, token=token, session=session)
            check(status == 405, f"{method} status {status}", failures)

        if token is not None:
            status, headers, body = request(url, "POST", {"jsonrpc": "2.0", "id": 4, "method": "tools/list"})
            check(status == 401, f"missing token status {status}", failures)
            check(json.loads(body)["error"]["message"] == "unauthorized", "missing token body", failures)
            status, _, _ = request(url, "POST", {"jsonrpc": "2.0", "id": 5, "method": "tools/list"}, token + "x")
            check(status == 401, f"wrong token status {status}", failures)
            status, _, _ = request(url, "GET")
            check(status == 401, f"GET without token status {status}", failures)
    finally:
        server.shutdown()
        server.server_close()
    return failures


def self_test(token):
    failures = drive(token)
    if token is None:
        failures += [f"guarded: {failure}" for failure in drive(SELF_TEST_TOKEN)]
    for failure in failures:
        print(f"FAIL {failure}", file=sys.stderr)
    print("self-test failed" if failures else "self-test passed", file=sys.stderr)
    return 1 if failures else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--token", help="require Authorization: Bearer <token>")
    parser.add_argument("--self-test", action="store_true", help="drive the server in-process and exit")
    arguments = parser.parse_args()
    if arguments.self_test:
        return self_test(arguments.token)
    server = serve(arguments.token)
    print(f"PORT={server.server_address[1]}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
