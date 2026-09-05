#!/usr/bin/env python3
"""Hit /api/context/search with the retrieval eval set.

Env:
  ZONE_EVAL_BASE_URL   default http://127.0.0.1:8000
  ZONE_EVAL_TOKEN      JWT with workspace read
  ZONE_EVAL_WORKSPACE  workspace UUID
"""

from __future__ import annotations

import json
import os
import sys
import urllib.parse
import urllib.request
from pathlib import Path

EVAL_PATH = Path(__file__).with_name("retrieval.json")


def main() -> int:
    base = os.environ.get("ZONE_EVAL_BASE_URL", "http://127.0.0.1:8000").rstrip("/")
    token = os.environ.get("ZONE_EVAL_TOKEN")
    workspace = os.environ.get("ZONE_EVAL_WORKSPACE")
    if not token or not workspace:
        print("ZONE_EVAL_TOKEN and ZONE_EVAL_WORKSPACE are required", file=sys.stderr)
        return 2

    cases = json.loads(EVAL_PATH.read_text())["cases"]
    failed = 0
    for case in cases:
        params = urllib.parse.urlencode(
            {
                "q": case["query"],
                "workspace_id": workspace,
                "mode": "hybrid",
                "limit": "10",
            }
        )
        req = urllib.request.Request(
            f"{base}/api/context/search?{params}",
            headers={"Authorization": f"Bearer {token}"},
        )
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                body = json.loads(resp.read().decode())
        except Exception as exc:
            print(f"FAIL {case['id']}: request error {exc}")
            failed += 1
            continue

        uris = [item.get("uri") or "" for item in body.get("results", [])]
        expected = case.get("expect_uri_contains") or []
        hit = any(any(needle in uri for uri in uris) for needle in expected)
        if hit:
            print(f"PASS {case['id']}")
        else:
            print(f"FAIL {case['id']}: wanted {expected} in {uris[:5]}")
            failed += 1

    print(f"{len(cases) - failed}/{len(cases)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
