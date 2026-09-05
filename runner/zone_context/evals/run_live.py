#!/usr/bin/env python3
"""Hit /api/context/search with graded chunk judgments.

Env:
  ZONE_EVAL_BASE_URL   default http://127.0.0.1:8000
  ZONE_EVAL_TOKEN      JWT with workspace read
  ZONE_EVAL_WORKSPACE  workspace UUID
  ZONE_EVAL_SET        eval JSON path (default retrieval.json next to this script)
"""

from __future__ import annotations

import json
import math
import os
import ssl
import sys
import urllib.parse
import urllib.request
from pathlib import Path

EVAL_PATH = Path(os.environ.get("ZONE_EVAL_SET") or Path(__file__).with_name("retrieval.json"))
K = 10
RELEVANT = 3


def grade_hit(uri: str, text: str, judgments: list[dict]) -> int:
    best = 0
    for judgment in judgments:
        if judgment["uri_contains"] not in uri:
            continue
        needles = judgment.get("must_contain") or []
        if needles and not any(needle in text or needle in uri for needle in needles):
            continue
        best = max(best, int(judgment["grade"]))
    return best


def grade_file(uri: str, judgments: list[dict]) -> int:
    best = 0
    for judgment in judgments:
        if judgment["uri_contains"] in uri:
            best = max(best, int(judgment["grade"]))
    return best


def unique_file_grades(
    hits: list[dict], judgments: list[dict], *, file_only: bool = False
) -> list[int]:
    grades: list[int] = []
    seen: set[str] = set()
    for item in hits:
        uri = item.get("uri") or ""
        file_key = uri.split("@", 1)[0]
        if file_key in seen:
            continue
        seen.add(file_key)
        if file_only:
            grades.append(grade_file(uri, judgments))
            continue
        passage = item.get("text") or item.get("snippet") or ""
        grades.append(grade_hit(uri, passage, judgments))
    return grades


def dcg(grades: list[int]) -> float:
    return sum((2**grade - 1) / math.log2(index + 2) for index, grade in enumerate(grades))


def ndcg_at(grades: list[int], ideal: list[int], k: int) -> float:
    actual = grades[:k]
    gold = sorted(ideal, reverse=True)[:k]
    gold.extend([0] * max(0, len(actual) - len(gold)))
    denom = dcg(gold)
    if denom == 0:
        return 0.0
    return min(1.0, dcg(actual) / denom)


def average_precision(grades: list[int], relevant: int) -> float:
    seen = 0.0
    acc = 0.0
    total = sum(1 for grade in grades if grade >= relevant)
    if total == 0:
        return 0.0
    for index, grade in enumerate(grades):
        if grade >= relevant:
            seen += 1
            acc += seen / (index + 1)
    return acc / total


def first_relevant_rank(grades: list[int], relevant: int) -> int | None:
    for index, grade in enumerate(grades, start=1):
        if grade >= relevant:
            return index
    return None


def main() -> int:
    base = os.environ.get("ZONE_EVAL_BASE_URL", "http://127.0.0.1:8000").rstrip("/")
    token = os.environ.get("ZONE_EVAL_TOKEN")
    workspace = os.environ.get("ZONE_EVAL_WORKSPACE")
    if not token or not workspace:
        print("ZONE_EVAL_TOKEN and ZONE_EVAL_WORKSPACE are required", file=sys.stderr)
        return 2

    cases = json.loads(EVAL_PATH.read_text())["cases"]
    print(f"eval_set={EVAL_PATH.name} cases={len(cases)}", file=sys.stderr)
    failed = 0
    ndcgs: list[float] = []
    file_ndcgs: list[float] = []
    maps: list[float] = []
    reciprocal_ranks: list[float] = []
    file_reciprocal: list[float] = []
    for case in cases:
        params = urllib.parse.urlencode(
            {
                "q": case["query"],
                "workspace_id": workspace,
                "mode": "hybrid",
                "limit": str(K),
            }
        )
        req = urllib.request.Request(
            f"{base}/api/context/search?{params}",
            headers={"Authorization": f"Bearer {token}"},
        )
        context = ssl._create_unverified_context() if os.environ.get("ZONE_EVAL_INSECURE") else None
        try:
            with urllib.request.urlopen(req, timeout=90, context=context) as resp:
                body = json.loads(resp.read().decode())
        except Exception as exc:
            print(f"FAIL {case['id']}: request error {exc}")
            failed += 1
            ndcgs.append(0.0)
            maps.append(0.0)
            reciprocal_ranks.append(0.0)
            continue

        hits = body.get("results") or []
        judgments = case.get("judgments") or []
        grades = unique_file_grades(hits, judgments)
        file_grades = unique_file_grades(hits, judgments, file_only=True)
        ideal = [int(j["grade"]) for j in judgments]
        ndcg = ndcg_at(grades, ideal, K)
        file_ndcg = ndcg_at(file_grades, ideal, K)
        ap = average_precision(grades, RELEVANT)
        rank = first_relevant_rank(grades, RELEVANT)
        file_rank = first_relevant_rank(file_grades, RELEVANT)
        ndcgs.append(ndcg)
        file_ndcgs.append(file_ndcg)
        maps.append(ap)
        file_reciprocal.append(0.0 if file_rank is None else 1.0 / file_rank)
        if rank is None:
            top = [
                (item.get("uri") or "")[-72:]
                for item in hits[:3]
            ]
            print(
                f"FAIL {case['id']}: no grade>={RELEVANT} chunk in top {K}  "
                f"ndcg={ndcg:.3f} file_ndcg={file_ndcg:.3f}  {grades[:5]}  "
                f"files={file_grades[:5]}  top={top}"
            )
            failed += 1
            reciprocal_ranks.append(0.0)
            continue
        print(
            f"PASS {case['id']} rank={rank}  nDCG@{K}={ndcg:.3f}  "
            f"file_nDCG={file_ndcg:.3f}  AP={ap:.3f}  grades={grades[:5]}"
        )
        reciprocal_ranks.append(1.0 / rank)

    total = len(cases)
    passed = total - failed
    mean_ndcg = sum(ndcgs) / total if total else 0.0
    mean_file_ndcg = sum(file_ndcgs) / total if total else 0.0
    mean_map = sum(maps) / total if total else 0.0
    mrr = sum(reciprocal_ranks) / total if total else 0.0
    file_mrr = sum(file_reciprocal) / total if total else 0.0
    print(
        f"{passed}/{total} grade>={RELEVANT}@{K}  "
        f"nDCG@{K}={mean_ndcg:.3f}  MAP={mean_map:.3f}  MRR={mrr:.3f}"
    )
    print(
        f"file-level nDCG@{K}={mean_file_ndcg:.3f}  "
        f"file-MRR={file_mrr:.3f}  "
        f"(uri match only; first chunk may miss must_contain)"
    )
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
