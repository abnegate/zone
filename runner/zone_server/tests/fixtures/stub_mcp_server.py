#!/usr/bin/env python3
"""A stdio MCP server with one tool, for tests that attach a server without
depending on anything installed on the machine.

Speaks newline-delimited JSON-RPC: answers initialize with the protocol
version the client proposed, lists one tool named delegate, and echoes a
call's instruction back as text.
"""
import json
import sys

TOOL = {
    "name": "delegate",
    "description": "Hand an instruction to another coding agent and return what it printed.",
    "inputSchema": {
        "type": "object",
        "properties": {"instruction": {"type": "string"}},
        "required": ["instruction"],
    },
}


def reply(message_id, result):
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": message_id, "result": result}) + "\n")
    sys.stdout.flush()


for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    message = json.loads(line)
    method = message.get("method")
    message_id = message.get("id")
    if message_id is None:
        continue
    if method == "initialize":
        reply(
            message_id,
            {
                "protocolVersion": message["params"]["protocolVersion"],
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "stub", "version": "0.0.1"},
            },
        )
    elif method == "tools/list":
        reply(message_id, {"tools": [TOOL]})
    elif method == "tools/call":
        instruction = message["params"].get("arguments", {}).get("instruction", "")
        reply(message_id, {"content": [{"type": "text", "text": f"delegated:{instruction}"}]})
    else:
        reply(message_id, {})
