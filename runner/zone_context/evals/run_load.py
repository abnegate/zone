#!/usr/bin/env python3
"""Measure hybrid search latency and the live corpus size.

Env:
  ZONE_EVAL_BASE_URL   default http://127.0.0.1:8000
  ZONE_EVAL_TOKEN      JWT with workspace read
  ZONE_EVAL_WORKSPACE  workspace UUID
  ZONE_LOAD_REQUESTS   default 64
  ZONE_LOAD_CONCURRENCY default 8
"""

from __future__ import annotations

import json
import os
import ssl
import statistics
import subprocess
import sys
import time
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

EVAL_DIR = Path(__file__).resolve().parent
DEFAULT_QUERIES = [
    "How does the server verify a GitHub webhook signature?",
    "What does should_skip_blob do when a GitHub file SHA is unchanged?",
    "Where is NotionAdapter implemented?",
    "How does validate_public_url reject private IP addresses?",
    "Where is the in-process Jina ONNX cross-encoder constructed?",
    "How is source_sync_state version stored after an incremental index?",
    "What fields does CreateKnowledgeRequest require?",
    "How does generate_token produce a password-reset secret?",
]


def corpus_stats() -> dict[str, int]:
    sql = (
        "SELECT "
        "(SELECT COUNT(*) FROM embeddings) AS embeddings, "
        "(SELECT COUNT(*) FROM content_chunks) AS chunks, "
        "(SELECT COUNT(*) FROM content_items) AS items;"
    )
    try:
        user = subprocess.check_output(
            ["docker", "exec", "postgres", "printenv", "POSTGRES_USER"],
            text=True,
        ).strip()
        db = subprocess.check_output(
            ["docker", "exec", "manager", "printenv", "POSTGRES_DB"],
            text=True,
        ).strip()
        raw = subprocess.check_output(
            [
                "docker",
                "exec",
                "postgres",
                "psql",
                "-U",
                user or "zone",
                "-d",
                db or "manager",
                "-t",
                "-A",
                "-F",
                ",",
                "-c",
                sql,
            ],
            text=True,
        ).strip()
        embeddings, chunks, items = (int(part) for part in raw.split(","))
        return {"embeddings": embeddings, "chunks": chunks, "items": items}
    except Exception as exc:
        print(f"corpus_stats skipped: {exc}", file=sys.stderr)
        return {}


def search(base: str, token: str, workspace: str, query: str, insecure: bool) -> float:
    params = urllib.parse.urlencode(
        {
            "q": query,
            "workspace_id": workspace,
            "mode": "hybrid",
            "limit": "10",
        }
    )
    req = urllib.request.Request(
        f"{base}/api/context/search?{params}",
        headers={"Authorization": f"Bearer {token}"},
    )
    context = ssl._create_unverified_context() if insecure else None
    started = time.perf_counter()
    with urllib.request.urlopen(req, timeout=90, context=context) as resp:
        body = json.loads(resp.read().decode())
        if not isinstance(body.get("results"), list):
            raise RuntimeError("search returned no results list")
    return time.perf_counter() - started


def percentile(values: list[float], pct: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    index = min(len(ordered) - 1, max(0, round((pct / 100.0) * (len(ordered) - 1))))
    return ordered[index]


def main() -> int:
    base = os.environ.get("ZONE_EVAL_BASE_URL", "http://127.0.0.1:8000").rstrip("/")
    token = os.environ.get("ZONE_EVAL_TOKEN")
    workspace = os.environ.get("ZONE_EVAL_WORKSPACE")
    if not token or not workspace:
        print("ZONE_EVAL_TOKEN and ZONE_EVAL_WORKSPACE are required", file=sys.stderr)
        return 2

    requests_n = int(os.environ.get("ZONE_LOAD_REQUESTS", "64"))
    concurrency = int(os.environ.get("ZONE_LOAD_CONCURRENCY", "8"))
    insecure = bool(os.environ.get("ZONE_EVAL_INSECURE"))
    queries = DEFAULT_QUERIES
    heldout = EVAL_DIR / "retrieval_heldout.json"
    train = EVAL_DIR / "retrieval.json"
    extra = []
    for path in (train, heldout):
        if path.exists():
            extra.extend(case["query"] for case in json.loads(path.read_text())["cases"])
    if extra:
        queries = extra

    stats = corpus_stats()
    if stats:
        print(
            f"corpus embeddings={stats['embeddings']} "
            f"chunks={stats['chunks']} items={stats['items']}"
        )

    # Warm the ANN / GIN / CE path so the timed window is serving, not startup.
    for query in queries[: min(8, len(queries))]:
        search(base, token, workspace, query, insecure)

    planned = [queries[i % len(queries)] for i in range(requests_n)]
    latencies: list[float] = []
    errors = 0
    started = time.perf_counter()
    with ThreadPoolExecutor(max_workers=concurrency) as pool:
        futures = [
            pool.submit(search, base, token, workspace, query, insecure) for query in planned
        ]
        for future in as_completed(futures):
            try:
                latencies.append(future.result())
            except Exception as exc:
                errors += 1
                print(f"error: {exc}", file=sys.stderr)
    elapsed = time.perf_counter() - started
    ok = len(latencies)
    qps = ok / elapsed if elapsed else 0.0
    print(
        f"load requests={requests_n} ok={ok} errors={errors} "
        f"concurrency={concurrency} wall_s={elapsed:.2f} qps={qps:.2f}"
    )
    if latencies:
        ms = [value * 1000.0 for value in latencies]
        print(
            f"latency_ms p50={percentile(ms, 50):.1f} "
            f"p95={percentile(ms, 95):.1f} "
            f"p99={percentile(ms, 99):.1f} "
            f"mean={statistics.mean(ms):.1f} "
            f"max={max(ms):.1f}"
        )
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
