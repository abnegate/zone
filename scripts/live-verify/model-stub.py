#!/usr/bin/env python3
"""A scripted model behind the real model paths.

`zone-server` reaches its model through two doors: `LITELLM_HOST` for chat
completions (the OpenAI shape, streamed for every round of the agent loop) and
`OLLAMA_HOST` for the installed-model catalog, model profiles and embeddings.
This serves both, so the server, the workers and the console run exactly as
they do in production while what the "model" says is decided by the test.

Nothing here claims anything about a model. What it proves is everything on
the far side of the model: that a tool call the model makes is executed,
recorded, rendered and answered the way the code says it is.

Control endpoints, for the live suite:

    POST /_stub/script    {"trigger": "...", "rounds": [...], "aside": "..."}
    POST /_stub/reset     forget every script and every logged request
    GET  /_stub/requests  every chat completion request seen, oldest first

A script answers the rounds of every turn whose LAST user message contains its
trigger (case-insensitive), in order; the most recently registered matching
script wins. A round is one of

    {"text": "..."}                                   a reply
    {"reasoning": "...", "text": "..."}              thinking, then a reply
    {"calls": [{"name": "...", "arguments": {...}}]} one or more tool calls
    {"text": "...", "calls": [...]}                  both
    {"status": 503, "body": "..."}                   an upstream fault
    {"delay": 2.5, ...}                              wait before answering

Requests with `stream: false` are asides (a title, a summary, a classifier)
and never consume a round: they get the script's `aside` text, or a default.

With MODEL_STUB_UPSTREAM set (an OpenAI-compatible base URL such as a
llama-server's `http://127.0.0.1:8080/v1`), a round no script answers is
forwarded there verbatim and the answer streamed back, so the same rig can be
driven by a real model. MODEL_STUB_EMBED_UPSTREAM does the same for
embeddings; without it embeddings are deterministic pseudo-vectors.
"""

import argparse
import hashlib
import json
import os
import random
import re
import sys
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MODEL = os.environ.get("MODEL_STUB_MODEL", "stand-in:latest")
EMBED = os.environ.get("MODEL_STUB_EMBED", "nomic-embed-text:latest")
EMBED_DIMENSION = int(os.environ.get("MODEL_STUB_EMBED_DIMENSION", "768"))
UPSTREAM = os.environ.get("MODEL_STUB_UPSTREAM")
UPSTREAM_MODEL = os.environ.get("MODEL_STUB_UPSTREAM_MODEL")
EMBED_UPSTREAM = os.environ.get("MODEL_STUB_EMBED_UPSTREAM")

LOCK = threading.Lock()
SCRIPTS = []
REQUESTS = []
LOG_PATH = os.environ.get("MODEL_STUB_LOG")


def log(entry):
    with LOCK:
        entry["n"] = len(REQUESTS)
        REQUESTS.append(entry)
    if LOG_PATH:
        with open(LOG_PATH, "a", encoding="utf-8") as handle:
            handle.write(json.dumps(entry) + "\n")


def text_of(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(
            part.get("text", "") for part in content if isinstance(part, dict)
        )
    return ""


def last_user(messages):
    for message in reversed(messages):
        if message.get("role") == "user":
            return text_of(message.get("content"))
    return ""


def user_block(messages):
    """The person's text for the current turn.

    The server sends what was typed as one user message and may follow it with
    more user messages of its own (a web-search context block, for one), so the
    turn's text is the whole run of user messages before the assistant's first
    round, read back from wherever the conversation currently ends.
    """
    index = len(messages) - 1
    while index >= 0 and messages[index].get("role") != "user":
        index -= 1
    block = []
    while index >= 0 and messages[index].get("role") == "user":
        block.append(text_of(messages[index].get("content")))
        index -= 1
    block.reverse()
    return "\n".join(block)


def system_text(messages):
    return "\n\n".join(
        text_of(m.get("content")) for m in messages if m.get("role") == "system"
    )


def tool_results(messages):
    """The tool results since the last user message, oldest first."""
    results = []
    for message in reversed(messages):
        if message.get("role") == "user":
            break
        if message.get("role") == "tool":
            results.append(
                {
                    "tool_call_id": message.get("tool_call_id"),
                    "content": text_of(message.get("content")),
                }
            )
    results.reverse()
    return results


def match_script(last):
    lowered = last.lower()
    with LOCK:
        for script in reversed(SCRIPTS):
            if script["trigger"].lower() in lowered:
                return script
    return None


def chunk(delta, finish=None, usage=None):
    body = {
        "id": "stand-in",
        "object": "chat.completion.chunk",
        "created": int(time.time()),
        "model": MODEL,
        "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
    }
    if usage is not None:
        body["usage"] = usage
    return ("data: " + json.dumps(body) + "\n\n").encode()


def estimate(text):
    return max(1, len(text) // 4)


def dimension_for(model):
    """The width the server expects of a model, mirroring zone_context's table."""
    name = (model or "").split(":")[0].lower()
    if "qwen3-embedding" in name:
        return 1024
    return {
        "nomic-embed-text": 768,
        "nomic-embed-text-v1.5": 768,
        "mxbai-embed-large": 1024,
        "snowflake-arctic-embed": 1024,
        "bge-small-en": 384,
        "all-minilm": 384,
    }.get(name, EMBED_DIMENSION)


def fitted(vector, width):
    """Pad with zeros or truncate so a real model's width matches the expected one."""
    if len(vector) >= width:
        return vector[:width]
    return vector + [0.0] * (width - len(vector))


def pseudo_embedding(prompt, width):
    seed = int.from_bytes(hashlib.sha256(prompt.encode("utf-8")).digest()[:8], "big")
    generator = random.Random(seed)
    vector = [generator.gauss(0.0, 1.0) for _ in range(width)]
    norm = sum(v * v for v in vector) ** 0.5 or 1.0
    return [v / norm for v in vector]


def forward(url, body, headers=None):
    data = json.dumps(body).encode()
    request = urllib.request.Request(url, data=data, method="POST")
    request.add_header("Content-Type", "application/json")
    for key, value in (headers or {}).items():
        request.add_header(key, value)
    return urllib.request.urlopen(request, timeout=3600)


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):  # quiet
        if os.environ.get("MODEL_STUB_VERBOSE"):
            sys.stderr.write("%s - %s\n" % (self.address_string(), fmt % args))

    # -- plumbing ---------------------------------------------------------

    def body(self):
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b""
        try:
            return json.loads(raw) if raw else {}
        except ValueError:
            return {}

    def send_json(self, status, payload):
        data = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def send_text(self, status, text):
        data = text.encode()
        self.send_response(status)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    # -- routes -----------------------------------------------------------

    def do_GET(self):
        path = self.path.split("?")[0]
        if path in ("/", "/health"):
            return self.send_text(200, "Ollama is running")
        if path == "/api/tags":
            return self.send_json(200, {"models": [tag(MODEL), tag(EMBED)]})
        if path == "/api/version":
            return self.send_json(200, {"version": "0.0.0-stand-in"})
        if path == "/api/ps":
            return self.send_json(200, {"models": []})
        if path in ("/v1/models", "/models"):
            return self.send_json(
                200,
                {
                    "object": "list",
                    "data": [
                        {"id": MODEL, "object": "model", "owned_by": "stand-in"},
                        {"id": EMBED, "object": "model", "owned_by": "stand-in"},
                    ],
                },
            )
        if path == "/_stub/requests":
            with LOCK:
                return self.send_json(200, {"requests": list(REQUESTS)})
        if path == "/_stub/scripts":
            with LOCK:
                return self.send_json(200, {"scripts": [summary(s) for s in SCRIPTS]})
        return self.send_json(404, {"error": "no such route: %s" % path})

    def do_POST(self):
        path = self.path.split("?")[0]
        body = self.body()
        if path in ("/v1/chat/completions", "/chat/completions"):
            return self.completion(body)
        if path == "/api/show":
            return self.show(body)
        if path in ("/api/embeddings", "/api/embed"):
            return self.embeddings(body)
        if path == "/api/chat":
            return self.send_json(
                200,
                {
                    "model": MODEL,
                    "message": {"role": "assistant", "content": "Aside."},
                    "done": True,
                },
            )
        if path == "/_stub/script":
            trigger = (body.get("trigger") or "").strip()
            if not trigger:
                return self.send_json(400, {"error": "a script needs a trigger"})
            script = {
                "trigger": trigger,
                "rounds": list(body.get("rounds") or []),
                "aside": body.get("aside"),
                "served": 0,
            }
            with LOCK:
                SCRIPTS[:] = [s for s in SCRIPTS if s["trigger"] != trigger]
                SCRIPTS.append(script)
            return self.send_json(200, {"registered": summary(script)})
        if path == "/_stub/reset":
            with LOCK:
                SCRIPTS.clear()
                REQUESTS.clear()
            return self.send_json(200, {"reset": True})
        return self.send_json(404, {"error": "no such route: %s" % path})

    # -- ollama -------------------------------------------------------------

    def show(self, body):
        name = body.get("model") or body.get("name") or ""
        if same(name, MODEL):
            return self.send_json(
                200,
                {
                    "capabilities": ["completion", "tools"],
                    "details": tag(MODEL)["details"],
                    "model_info": {"general.parameter_count": 7_600_000_000},
                    "template": "{{ .System }}{{ .Prompt }}",
                    "modelfile": "FROM stand-in",
                },
            )
        if same(name, EMBED):
            return self.send_json(
                200,
                {"capabilities": ["embedding"], "details": tag(EMBED)["details"]},
            )
        return self.send_json(404, {"error": "model '%s' not found" % name})

    def embeddings(self, body):
        prompt = body.get("prompt")
        if prompt is None:
            prompt = body.get("input")
        if isinstance(prompt, list):
            prompt = "\n".join(str(p) for p in prompt)
        prompt = prompt or ""
        width = dimension_for(body.get("model"))
        if EMBED_UPSTREAM:
            try:
                with forward(
                    EMBED_UPSTREAM.rstrip("/") + "/embeddings", {"input": prompt[:6000]}
                ) as response:
                    payload = json.load(response)
                vector = fitted(payload["data"][0]["embedding"], width)
            except Exception as error:  # noqa: BLE001
                return self.send_json(502, {"error": "embedding upstream: %s" % error})
        else:
            vector = pseudo_embedding(prompt, width)
        if os.environ.get("MODEL_STUB_VERBOSE"):
            sys.stderr.write("embedding %s -> %d\n" % (body.get("model"), width))
        return self.send_json(200, {"embedding": vector, "embeddings": [vector]})

    # -- chat completions ---------------------------------------------------

    def completion(self, body):
        messages = body.get("messages") or []
        stream = bool(body.get("stream"))
        last = user_block(messages)
        script = match_script(last)
        entry = {
            "at": time.time(),
            "kind": "round" if stream else "aside",
            "model": body.get("model"),
            "stream": stream,
            "trigger": script["trigger"] if script else None,
            "round": None,
            "last_user": last,
            "system": system_text(messages),
            "tools": [
                (t.get("function") or {}).get("name")
                for t in (body.get("tools") or [])
            ],
            "messages": len(messages),
            "tool_results": tool_results(messages),
            "exhausted": False,
            "forwarded": False,
            "answer": None,
        }

        if not stream:
            text = (script or {}).get("aside") or default_aside(messages)
            log(entry)
            return self.send_json(
                200,
                {
                    "id": "aside",
                    "object": "chat.completion",
                    "created": int(time.time()),
                    "model": MODEL,
                    "choices": [
                        {
                            "index": 0,
                            "finish_reason": "stop",
                            "message": {"role": "assistant", "content": text},
                        }
                    ],
                    "usage": {
                        "prompt_tokens": estimate(json.dumps(messages)),
                        "completion_tokens": estimate(text),
                        "total_tokens": estimate(json.dumps(messages)) + estimate(text),
                    },
                },
            )

        round_ = None
        if script is not None:
            with LOCK:
                index = script["served"]
                script["served"] += 1
            entry["round"] = index
            if index < len(script["rounds"]):
                round_ = script["rounds"][index]
            else:
                entry["exhausted"] = True
                round_ = {"text": "Done."}
        elif UPSTREAM:
            entry["forwarded"] = True
            log(entry)
            return self.forward_stream(body, entry)
        else:
            round_ = {"text": "I have no script for that message."}

        log(entry)
        self.stream_round(round_, messages)

    def forward_stream(self, body, entry):
        """Relay a real model's stream, and keep what it answered on the log entry."""
        if UPSTREAM_MODEL:
            body = dict(body, model=UPSTREAM_MODEL)
        started = time.time()
        try:
            response = forward(UPSTREAM.rstrip("/") + "/chat/completions", body)
        except urllib.error.HTTPError as error:  # type: ignore[attr-defined]
            return self.send_text(error.code, error.read().decode("utf-8", "replace"))
        except Exception as error:  # noqa: BLE001
            return self.send_text(502, "upstream: %s" % error)
        self.send_response(response.status)
        self.send_header("Content-Type", response.headers.get("Content-Type", "text/event-stream"))
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()
        raw = []
        with response:
            while True:
                piece = response.read(4096)
                if not piece:
                    break
                raw.append(piece)
                self.wfile.write(("%x\r\n" % len(piece)).encode() + piece + b"\r\n")
                self.wfile.flush()
        self.wfile.write(b"0\r\n\r\n")
        self.wfile.flush()
        content, calls = [], {}
        for line in b"".join(raw).decode("utf-8", "replace").splitlines():
            if not line.startswith("data: ") or line == "data: [DONE]":
                continue
            try:
                delta = json.loads(line[6:])["choices"][0]["delta"]
            except (ValueError, KeyError, IndexError):
                continue
            if delta.get("content"):
                content.append(delta["content"])
            for call in delta.get("tool_calls") or []:
                slot = calls.setdefault(call.get("index", 0), {"name": "", "arguments": ""})
                function = call.get("function") or {}
                slot["name"] = function.get("name") or slot["name"]
                slot["arguments"] += function.get("arguments") or ""
        with LOCK:
            entry["answer"] = {
                "content": "".join(content),
                "calls": [calls[k] for k in sorted(calls)],
                "seconds": round(time.time() - started, 1),
            }
        if LOG_PATH:
            with open(LOG_PATH, "a", encoding="utf-8") as handle:
                handle.write(json.dumps({"answer_for": entry["n"], **entry["answer"]}) + "\n")

    def stream_round(self, round_, messages):
        delay = round_.get("delay")
        if delay:
            time.sleep(float(delay))
        if "status" in round_:
            return self.send_text(int(round_["status"]), round_.get("body") or "upstream fell over")

        pieces = []
        reasoning = round_.get("reasoning")
        if reasoning:
            pieces.append(chunk({"reasoning_content": reasoning}))
        text = round_.get("text") or ""
        words = text.split(" ")
        for start in range(0, len(words), 6):
            part = " ".join(words[start : start + 6])
            if start + 6 < len(words):
                part += " "
            if part:
                pieces.append(chunk({"content": part}))
        calls = []
        for position, call in enumerate(round_.get("calls") or []):
            arguments = fill(call.get("arguments", {}), messages)
            if not isinstance(arguments, str):
                arguments = json.dumps(arguments)
            calls.append(
                {
                    "index": position,
                    "id": call.get("id") or "call-%d-%d" % (int(time.time() * 1000) % 100000, position),
                    "type": "function",
                    "function": {"name": call["name"], "arguments": arguments},
                }
            )
        if calls:
            pieces.append(chunk({"tool_calls": calls}))
        prompt_tokens = estimate(json.dumps(messages))
        completion_tokens = estimate(text) + sum(estimate(c["function"]["arguments"]) for c in calls)
        pieces.append(
            chunk(
                {},
                finish="tool_calls" if calls and not text else "stop",
                usage={
                    "prompt_tokens": prompt_tokens,
                    "completion_tokens": completion_tokens,
                    "total_tokens": prompt_tokens + completion_tokens,
                },
            )
        )
        pieces.append(b"data: [DONE]\n\n")

        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()
        for piece in pieces:
            self.wfile.write(("%x\r\n" % len(piece)).encode() + piece + b"\r\n")
            self.wfile.flush()
            time.sleep(0.03)
        self.wfile.write(b"0\r\n\r\n")
        self.wfile.flush()


def fill(value, messages):
    """Resolve `$re:<pattern>` strings against the tool results of this turn.

    A script cannot know a job id or a document id before the tool that minted
    it has run, so an argument may name a pattern instead; the first capture
    group of its first match over the newest tool result backwards fills it.
    A pattern with no match is left as the literal, which the server then
    refuses, and the refusal is what the test sees.
    """
    if isinstance(value, dict):
        return {k: fill(v, messages) for k, v in value.items()}
    if isinstance(value, list):
        return [fill(v, messages) for v in value]
    if isinstance(value, str) and value.startswith("$re:"):
        pattern = re.compile(value[4:], re.S)
        for result in reversed(tool_results(messages)):
            found = pattern.search(result["content"])
            if found:
                return found.group(1) if found.groups() else found.group(0)
        for message in reversed(messages):
            if message.get("role") == "system":
                found = pattern.search(text_of(message.get("content")))
                if found:
                    return found.group(1) if found.groups() else found.group(0)
    return value


def same(a, b):
    a = a.lower()
    b = b.lower()
    return a == b or a == b.rsplit(":latest", 1)[0] or b == a.rsplit(":latest", 1)[0]


def tag(name):
    embedding = "embed" in name
    return {
        "name": name,
        "model": name,
        "size": 274_000_000 if embedding else 4_700_000_000,
        "digest": hashlib.sha256(name.encode()).hexdigest(),
        "modified_at": "2026-09-19T00:00:00Z",
        "details": {
            "format": "gguf",
            "family": "nomic-bert" if embedding else "qwen2",
            "parameter_size": "137M" if embedding else "7.6B",
            "quantization_level": "F16" if embedding else "Q4_K_M",
        },
    }


def summary(script):
    return {
        "trigger": script["trigger"],
        "rounds": len(script["rounds"]),
        "served": script["served"],
        "aside": script.get("aside"),
    }


def default_aside(messages):
    everything = " ".join(text_of(m.get("content")) for m in messages)
    lowered = everything.lower()
    if lowered.startswith("return exactly image, audio, or chat"):
        # The media intent classifier: decide from the user's words alone.
        asked = everything.split("User:", 1)[-1].lower()
        if re.search(r"\b(draw|paint|sketch|picture|image|photo|render|illustrat)", asked):
            return "IMAGE"
        if re.search(r"\b(soundscape|sound|song|music|audio|melody|jingle)", asked):
            return "AUDIO"
        return "CHAT"
    if "title" in lowered:
        return "Scripted conversation"
    return "Aside."


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=int(os.environ.get("MODEL_STUB_PORT", "11435")))
    parser.add_argument("--host", default="127.0.0.1")
    args = parser.parse_args()
    server = ThreadingHTTPServer((args.host, args.port), Handler)
    server.daemon_threads = True
    sys.stderr.write("model stand-in on %s:%d (model %s, embeddings %s%s)\n" % (
        args.host, args.port, MODEL, EMBED, ", forwarding to %s" % UPSTREAM if UPSTREAM else ""))
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
