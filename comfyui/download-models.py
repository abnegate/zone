#!/usr/bin/env python3
"""Download and verify the models declared in model-manifest.json."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import time
import urllib.error
import urllib.request
from collections.abc import Callable
from pathlib import Path
from typing import Any

CHUNK_SIZE = 8 * 1024 * 1024
USER_AGENT = "zone-comfyui-model-setup/1"
ProgressFn = Callable[[dict[str, Any]], None]


VALID_BUNDLES = {
    "audio",
    "image",
    "image-dev",
    "image-edit",
    "image-people",
    "image-people-control",
    "upscale",
    "video",
    "vision",
}


def load_manifest(path: Path) -> list[dict[str, Any]]:
    with path.open(encoding="utf-8") as handle:
        manifest = json.load(handle)
    if manifest.get("schema_version") != 1 or not isinstance(manifest.get("models"), list):
        raise ValueError(f"unsupported model manifest: {path}")
    return manifest["models"]


def model_bundle(model: dict[str, Any]) -> str:
    bundle = str(model.get("bundle") or "image")
    if bundle not in VALID_BUNDLES:
        raise ValueError(f"unsupported model bundle: {bundle}")
    return bundle


def model_bundles(model: dict[str, Any]) -> set[str]:
    primary = model_bundle(model)
    extra = model.get("bundles")
    if extra is None:
        return {primary}
    if not isinstance(extra, list):
        raise ValueError(f"unsupported bundles list: {extra!r}")
    bundles = {primary}
    for item in extra:
        name = str(item)
        if name not in VALID_BUNDLES:
            raise ValueError(f"unsupported model bundle: {name}")
        bundles.add(name)
    return bundles


def select_models(
    models: list[dict[str, Any]],
    bundle: str | list[str],
    only: str | None = None,
) -> list[dict[str, Any]]:
    if only:
        chosen = [model for model in models if model.get("id") == only]
        if not chosen:
            known = ", ".join(sorted(str(model.get("id")) for model in models))
            raise ValueError(f"unknown model id: {only} (known: {known})")
        return chosen
    requested = [bundle] if isinstance(bundle, str) else list(bundle)
    if not requested:
        raise ValueError("at least one bundle is required")
    if "all" in requested:
        return models
    unknown = [name for name in requested if name not in VALID_BUNDLES]
    if unknown:
        raise ValueError(f"unsupported bundle filter: {unknown[0]}")
    wanted = set(requested)
    selected: list[dict[str, Any]] = []
    seen: set[str] = set()
    for model in models:
        if wanted.isdisjoint(model_bundles(model)):
            continue
        identifier = str(model.get("id"))
        if identifier in seen:
            continue
        seen.add(identifier)
        selected.append(model)
    return selected


def checked_target(models_dir: Path, relative_path: str) -> Path:
    root = models_dir.expanduser().resolve()
    target = (root / relative_path).resolve()
    if root not in target.parents:
        raise ValueError(f"model path escapes models directory: {relative_path}")
    return target


def digest(path: Path) -> str:
    checksum = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(CHUNK_SIZE):
            checksum.update(chunk)
    return checksum.hexdigest()


def verify(path: Path, model: dict[str, Any]) -> tuple[bool, str]:
    if not path.is_file():
        return False, "missing"
    actual_size = path.stat().st_size
    expected_size = int(model["size_bytes"])
    if actual_size != expected_size:
        return False, f"size mismatch ({actual_size} != {expected_size})"
    actual_sha = digest(path)
    expected_sha = str(model["sha256"]).lower()
    if actual_sha != expected_sha:
        return False, f"SHA-256 mismatch ({actual_sha} != {expected_sha})"
    return True, "verified"


def progress_event(
    model: dict[str, Any],
    downloaded: int,
    total: int,
    *,
    index: int,
    count: int,
    started_at: float,
    origin_bytes: int,
    now: float | None = None,
) -> dict[str, Any]:
    moment = time.monotonic() if now is None else now
    elapsed = max(moment - started_at, 1e-6)
    transferred = max(downloaded - origin_bytes, 0)
    rate = transferred / elapsed
    remaining = max(total - downloaded, 0)
    percent = 0.0 if total <= 0 else min(100.0, downloaded * 100.0 / total)
    eta = remaining / rate if rate > 0 else None
    return {
        "id": str(model.get("id")),
        "index": index,
        "count": count,
        "bytes": downloaded,
        "total": total,
        "percent": percent,
        "rate_bytes": rate,
        "eta_seconds": eta,
    }


def format_progress(event: dict[str, Any]) -> str:
    total = int(event["total"])
    downloaded = int(event["bytes"])
    percent = float(event["percent"])
    rate = float(event["rate_bytes"])
    eta = event["eta_seconds"]
    filled = min(20, int(percent / 5))
    bar = "#" * filled + "-" * (20 - filled)
    eta_text = f"ETA {int(eta)}s" if eta is not None else "ETA --"
    return (
        f"[{event['index']}/{event['count']}] {event['id']}  {bar}  "
        f"{percent:5.1f}%  {downloaded / 1024**3:.2f}/{total / 1024**3:.2f} GiB  "
        f"{rate / 1024**2:.1f} MB/s  {eta_text}"
    )


def emit_progress(event: dict[str, Any], mode: str, sink: ProgressFn | None) -> None:
    if sink is not None:
        sink(event)
        return
    if mode == "jsonl":
        print(json.dumps(event), flush=True)
        return
    print(f"\r{format_progress(event)}", end="", flush=True)


def download(
    model: dict[str, Any],
    target: Path,
    *,
    progress: ProgressFn | None = None,
    progress_mode: str = "text",
    index: int = 1,
    count: int = 1,
) -> None:
    expected_size = int(model["size_bytes"])
    partial = target.with_name(f"{target.name}.part")
    target.parent.mkdir(parents=True, exist_ok=True)

    offset = partial.stat().st_size if partial.exists() else 0
    if offset > expected_size:
        partial.unlink()
        offset = 0

    headers = {"User-Agent": USER_AGENT}
    if offset:
        headers["Range"] = f"bytes={offset}-"

    request = urllib.request.Request(str(model["url"]), headers=headers)
    try:
        response = urllib.request.urlopen(request, timeout=60)
    except urllib.error.HTTPError as error:
        if error.code == 416 and offset == expected_size:
            partial.replace(target)
            return
        raise

    status = getattr(response, "status", response.getcode())
    if offset and status != 206:
        response.close()
        partial.unlink()
        offset = 0
        request = urllib.request.Request(
            str(model["url"]), headers={"User-Agent": USER_AGENT}
        )
        response = urllib.request.urlopen(request, timeout=60)

    mode = "ab" if offset else "wb"
    downloaded = offset
    started_at = time.monotonic()
    origin_bytes = downloaded
    with response, partial.open(mode) as handle:
        while chunk := response.read(CHUNK_SIZE):
            handle.write(chunk)
            downloaded += len(chunk)
            emit_progress(
                progress_event(
                    model,
                    downloaded,
                    expected_size,
                    index=index,
                    count=count,
                    started_at=started_at,
                    origin_bytes=origin_bytes,
                ),
                progress_mode,
                progress,
            )
        handle.flush()
        os.fsync(handle.fileno())
    if progress_mode != "jsonl" and progress is None:
        print()

    if downloaded != expected_size:
        raise RuntimeError(
            f"incomplete download for {model['id']}: "
            f"{downloaded} != {expected_size} bytes"
        )
    partial.replace(target)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--manifest",
        type=Path,
        default=Path(__file__).with_name("model-manifest.json"),
    )
    parser.add_argument("--models-dir", type=Path, required=True)
    parser.add_argument(
        "--verify-only",
        action="store_true",
        help="verify installed files without downloading",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="replace an installed file that fails verification",
    )
    parser.add_argument(
        "--only",
        help="download or verify a single model by id, ignoring --bundle",
    )
    parser.add_argument(
        "--bundle",
        action="append",
        dest="bundles",
        choices=(*sorted(VALID_BUNDLES), "all"),
        help="download or verify this model bundle (repeatable; default: image)",
    )
    parser.add_argument(
        "--progress",
        choices=("text", "jsonl"),
        default="text",
        help="progress output: a TTY bar (text) or one JSON object per update",
    )
    return parser.parse_args()


def run(
    models: list[dict[str, Any]],
    models_dir: Path,
    *,
    verify_only: bool = False,
    force: bool = False,
    progress_mode: str = "text",
    progress: ProgressFn | None = None,
) -> int:
    if not models:
        raise ValueError("no models declared for the selected bundles")
    failures = 0
    count = len(models)

    for index, model in enumerate(models, start=1):
        target = checked_target(models_dir, str(model["relative_path"]))
        valid, detail = verify(target, model)
        if valid:
            print(f"[{index}/{count}] {model['id']}: {detail} ({target})")
            continue
        if verify_only:
            print(f"[{index}/{count}] {model['id']}: {detail} ({target})", file=sys.stderr)
            failures += 1
            continue
        if target.exists() and not force:
            print(
                f"[{index}/{count}] {model['id']}: {detail}; pass --force to replace {target}",
                file=sys.stderr,
            )
            failures += 1
            continue

        if target.exists():
            target.unlink()
        print(f"[{index}/{count}] {model['id']}: downloading from immutable revision")
        download(
            model,
            target,
            progress=progress,
            progress_mode=progress_mode,
            index=index,
            count=count,
        )
        valid, detail = verify(target, model)
        if not valid:
            target.unlink(missing_ok=True)
            print(
                f"[{index}/{count}] {model['id']}: {detail}; removed invalid file",
                file=sys.stderr,
            )
            failures += 1
        else:
            print(f"[{index}/{count}] {model['id']}: verified ({target})")

    return 1 if failures else 0


def main() -> int:
    args = parse_args()
    bundles = args.bundles or ["image"]
    models = select_models(load_manifest(args.manifest), bundles, args.only)
    if not models:
        raise ValueError(f"no models declared for bundle {', '.join(bundles)}")
    return run(
        models,
        args.models_dir,
        verify_only=args.verify_only,
        force=args.force,
        progress_mode=args.progress,
    )


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, urllib.error.URLError) as error:
        print(f"model setup failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
