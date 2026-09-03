#!/usr/bin/env python3
"""Download and verify the models declared in model-manifest.json."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

CHUNK_SIZE = 8 * 1024 * 1024
USER_AGENT = "zone-comfyui-model-setup/1"


def load_manifest(path: Path) -> list[dict[str, Any]]:
    with path.open(encoding="utf-8") as handle:
        manifest = json.load(handle)
    if manifest.get("schema_version") != 1 or not isinstance(manifest.get("models"), list):
        raise ValueError(f"unsupported model manifest: {path}")
    return manifest["models"]


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


def download(model: dict[str, Any], target: Path) -> None:
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
    with response, partial.open(mode) as handle:
        while chunk := response.read(CHUNK_SIZE):
            handle.write(chunk)
            downloaded += len(chunk)
            print(
                f"\r{model['id']}: {downloaded / 1024**3:.2f} / "
                f"{expected_size / 1024**3:.2f} GiB",
                end="",
                flush=True,
            )
        handle.flush()
        os.fsync(handle.fileno())
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
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    models = load_manifest(args.manifest)
    failures = 0

    for model in models:
        target = checked_target(args.models_dir, str(model["relative_path"]))
        valid, detail = verify(target, model)
        if valid:
            print(f"{model['id']}: {detail} ({target})")
            continue
        if args.verify_only:
            print(f"{model['id']}: {detail} ({target})", file=sys.stderr)
            failures += 1
            continue
        if target.exists() and not args.force:
            print(
                f"{model['id']}: {detail}; pass --force to replace {target}",
                file=sys.stderr,
            )
            failures += 1
            continue

        if target.exists():
            target.unlink()
        print(f"{model['id']}: downloading from immutable revision")
        download(model, target)
        valid, detail = verify(target, model)
        if not valid:
            target.unlink(missing_ok=True)
            print(f"{model['id']}: {detail}; removed invalid file", file=sys.stderr)
            failures += 1
        else:
            print(f"{model['id']}: verified ({target})")

    return 1 if failures else 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, urllib.error.URLError) as error:
        print(f"model setup failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
