#!/usr/bin/env python3
"""Plan Zone feature models, gate on RAM and disk, and download them."""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import platform
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

PROJECT_ROOT = Path(__file__).resolve().parent.parent
CATALOG_PATH = Path(__file__).with_name("setup-features.json")
MANIFEST_PATH = PROJECT_ROOT / "comfyui" / "model-manifest.json"
DOWNLOAD_MODELS_PATH = PROJECT_ROOT / "comfyui" / "download-models.py"
MACOS_INSTALLER = PROJECT_ROOT / "scripts" / "setup-comfyui-macos.sh"
DEFAULT_OLLAMA_HOST = "http://127.0.0.1:11434"


class PlanError(ValueError):
    def __init__(self, message: str, *, code: str) -> None:
        super().__init__(message)
        self.code = code


@dataclass(frozen=True)
class Artifact:
    kind: str
    identifier: str
    size_bytes: int
    present: bool
    feature: str
    relative_path: str | None = None


@dataclass(frozen=True)
class Plan:
    features: tuple[str, ...]
    wants_all: bool
    preset_id: str
    artifacts: tuple[Artifact, ...]
    licenses: tuple[str, ...]
    ram_bytes: int
    disk_free_bytes: int
    disk_margin_bytes: int
    install_trainer: bool
    comfy_bundles: tuple[str, ...]
    env: dict[str, str]
    fast: str
    reason: str
    embed: str
    vision: str | None

    @property
    def total_bytes(self) -> int:
        return sum(artifact.size_bytes for artifact in self.artifacts)

    @property
    def present_bytes(self) -> int:
        return sum(
            artifact.size_bytes for artifact in self.artifacts if artifact.present
        )

    @property
    def needed_bytes(self) -> int:
        return sum(
            artifact.size_bytes for artifact in self.artifacts if not artifact.present
        )

    @property
    def required_free_bytes(self) -> int:
        if self.needed_bytes == 0:
            return 0
        return self.needed_bytes + self.disk_margin_bytes


def load_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise PlanError(f"invalid json: {path}", code="catalog")
    return payload


def load_download_models() -> Any:
    spec = importlib.util.spec_from_file_location("download_models", DOWNLOAD_MODELS_PATH)
    if spec is None or spec.loader is None:
        raise PlanError("could not load download-models.py", code="catalog")
    module = importlib.util.module_from_spec(spec)
    sys.modules["download_models"] = module
    spec.loader.exec_module(module)
    return module


def format_disk(size: int) -> str:
    gigabytes = size / 1_000_000_000
    if gigabytes >= 10:
        return f"{gigabytes:.0f} GB"
    if gigabytes >= 1:
        return f"{gigabytes:.1f} GB"
    megabytes = size / 1_000_000
    if megabytes >= 1:
        return f"{megabytes:.0f} MB"
    return f"{size} B"


def format_ram(size: int) -> str:
    gigabytes = size / 1024**3
    rounded = round(gigabytes)
    if abs(gigabytes - rounded) < 0.05:
        return f"{rounded} GB"
    return f"{gigabytes:.1f} GB"


def detect_ram_bytes() -> int:
    system = platform.system()
    if system == "Darwin":
        output = subprocess.check_output(["sysctl", "-n", "hw.memsize"], text=True)
        return int(output.strip())
    if system == "Linux":
        for line in Path("/proc/meminfo").read_text(encoding="utf-8").splitlines():
            if line.startswith("MemTotal:"):
                return int(line.split()[1]) * 1024
    raise PlanError("could not detect RAM", code="ram")


def detect_disk_free(path: Path) -> int:
    path.mkdir(parents=True, exist_ok=True)
    return shutil.disk_usage(path).free


def default_models_dir() -> Path:
    override = os.environ.get("COMFYUI_MODELS_DIR")
    if override:
        return Path(override).expanduser()
    if platform.system() == "Darwin":
        return (
            Path.home()
            / "Library"
            / "Application Support"
            / "Zone"
            / "ComfyUI"
            / "models"
        )
    return PROJECT_ROOT / "comfyui" / "models"


def feature_map(catalog: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {str(feature["id"]): feature for feature in catalog["features"]}


def all_feature_ids(catalog: dict[str, Any]) -> tuple[str, ...]:
    return tuple(str(feature["id"]) for feature in catalog["features"])


def parse_features(raw: str, catalog: dict[str, Any]) -> tuple[frozenset[str], bool]:
    known = set(all_feature_ids(catalog))
    stripped = raw.strip()
    if stripped == "" or stripped == "all":
        return frozenset(known), True
    parts = [part.strip() for part in stripped.split(",") if part.strip()]
    unknown = [part for part in parts if part not in known]
    if unknown:
        raise PlanError(
            f"unknown feature: {unknown[0]}. Known: {', '.join(all_feature_ids(catalog))}",
            code="unknown-feature",
        )
    selected = set(parts)
    selected.add("chat")
    return frozenset(selected), selected == known


def preset_by_id(catalog: dict[str, Any], preset_id: str) -> dict[str, Any]:
    for preset in catalog["chat_presets"]:
        if preset["id"] == preset_id:
            return preset
    known = ", ".join(str(preset["id"]) for preset in catalog["chat_presets"])
    raise PlanError(f"unknown chat preset: {preset_id}. Known: {known}", code="preset")


def pick_preset(catalog: dict[str, Any], ram_bytes: int) -> dict[str, Any]:
    chosen = catalog["chat_presets"][0]
    for preset in catalog["chat_presets"]:
        if ram_bytes >= int(preset["min_ram_bytes"]):
            chosen = preset
    return chosen


def ollama_size(catalog: dict[str, Any], name: str) -> int:
    models = catalog["ollama_models"]
    if name not in models:
        raise PlanError(f"missing ollama size for {name}", code="catalog")
    return int(models[name]["size_bytes"])


def comfy_models_for_bundles(download_models: Any, manifest: list[dict[str, Any]], bundles: set[str]) -> list[dict[str, Any]]:
    if not bundles:
        return []
    return download_models.select_models(manifest, sorted(bundles))


def bundles_for_features(catalog: dict[str, Any], selected: frozenset[str]) -> set[str]:
    bundles: set[str] = set()
    features = feature_map(catalog)
    for feature_id in selected:
        feature = features[feature_id]
        bundles.update(str(name) for name in feature.get("comfy_bundles") or [])
        unless = feature.get("comfy_bundles_unless") or {}
        for other, extra in unless.items():
            if other not in selected:
                bundles.update(str(name) for name in extra)
    return bundles


def ollama_names(
    catalog: dict[str, Any], selected: frozenset[str], preset: dict[str, Any]
) -> dict[str, str]:
    names = {
        "fast": str(preset["fast"]),
        "reason": str(preset["reason"]),
        "embed": str(catalog["embed"]),
    }
    if "vision" in selected:
        names["vision"] = str(catalog["vision_model"])
    return names


def artifact_present(
    artifact_kind: str,
    identifier: str,
    *,
    relative_path: str | None,
    models_dir: Path,
    ollama_tags: set[str],
    comfy_present: set[str] | None,
    download_models: Any,
    model: dict[str, Any] | None,
) -> bool:
    if artifact_kind == "ollama":
        return identifier in ollama_tags
    if comfy_present is not None:
        return identifier in comfy_present
    if model is None or relative_path is None:
        return False
    target = download_models.checked_target(models_dir, relative_path)
    valid, _detail = download_models.verify(target, model)
    return valid


def build_artifacts(
    catalog: dict[str, Any],
    manifest: list[dict[str, Any]],
    download_models: Any,
    selected: frozenset[str],
    preset: dict[str, Any],
    *,
    models_dir: Path,
    ollama_tags: set[str],
    comfy_present: set[str] | None,
) -> list[Artifact]:
    artifacts: list[Artifact] = []
    seen: set[tuple[str, str]] = set()
    features = feature_map(catalog)
    names = ollama_names(catalog, selected, preset)
    slot_feature = {"fast": "chat", "reason": "chat", "embed": "chat", "vision": "vision"}
    for slot, name in names.items():
        key = ("ollama", name)
        if key in seen:
            continue
        seen.add(key)
        artifacts.append(
            Artifact(
                kind="ollama",
                identifier=name,
                size_bytes=ollama_size(catalog, name),
                present=artifact_present(
                    "ollama",
                    name,
                    relative_path=None,
                    models_dir=models_dir,
                    ollama_tags=ollama_tags,
                    comfy_present=comfy_present,
                    download_models=download_models,
                    model=None,
                ),
                feature=slot_feature[slot],
            )
        )

    feature_of_bundle: dict[str, str] = {}
    for feature_id in selected:
        for bundle in features[feature_id].get("comfy_bundles") or []:
            feature_of_bundle.setdefault(str(bundle), feature_id)
        unless = features[feature_id].get("comfy_bundles_unless") or {}
        for other, extra in unless.items():
            if other not in selected:
                for bundle in extra:
                    feature_of_bundle.setdefault(str(bundle), feature_id)

    for model in comfy_models_for_bundles(
        download_models, manifest, bundles_for_features(catalog, selected)
    ):
        identifier = str(model["id"])
        key = ("comfy", identifier)
        if key in seen:
            continue
        seen.add(key)
        primary = str(model.get("bundle") or "image")
        artifacts.append(
            Artifact(
                kind="comfy",
                identifier=identifier,
                size_bytes=int(model["size_bytes"]),
                present=artifact_present(
                    "comfy",
                    identifier,
                    relative_path=str(model["relative_path"]),
                    models_dir=models_dir,
                    ollama_tags=ollama_tags,
                    comfy_present=comfy_present,
                    download_models=download_models,
                    model=model,
                ),
                feature=feature_of_bundle.get(primary, primary),
                relative_path=str(model["relative_path"]),
            )
        )
    return artifacts


def licenses_for(catalog: dict[str, Any], selected: frozenset[str]) -> tuple[str, ...]:
    seen: list[str] = []
    features = feature_map(catalog)
    for feature_id in all_feature_ids(catalog):
        if feature_id not in selected:
            continue
        for license_name in features[feature_id].get("licenses") or []:
            text = str(license_name)
            if text not in seen:
                seen.append(text)
    return tuple(seen)


def disk_report(plan: Plan, *, title: str) -> str:
    required = plan.required_free_bytes
    lines = [
        title,
        f"  All models:          {format_disk(plan.total_bytes)}",
        f"  Already on disk:     {format_disk(plan.present_bytes)}",
        f"  Still to download:   {format_disk(plan.needed_bytes)}",
        f"  Working space:       {format_disk(plan.disk_margin_bytes)}",
        f"  Free required:       {format_disk(required)}",
        f"  Free now:            {format_disk(plan.disk_free_bytes)}",
    ]
    if plan.disk_free_bytes < required:
        lines.append(
            f"  Short by:            {format_disk(required - plan.disk_free_bytes)}"
        )
    return "\n".join(lines)


def feature_isolation_bytes(
    catalog: dict[str, Any],
    manifest: list[dict[str, Any]],
    download_models: Any,
    feature_id: str,
    preset: dict[str, Any],
) -> int:
    selected = frozenset({"chat", feature_id})
    artifacts = build_artifacts(
        catalog,
        manifest,
        download_models,
        selected,
        preset,
        models_dir=Path("."),
        ollama_tags=set(),
        comfy_present=set(),
    )
    if feature_id == "chat":
        return sum(artifact.size_bytes for artifact in artifacts if artifact.kind == "ollama" and artifact.feature == "chat")
    return sum(
        artifact.size_bytes for artifact in artifacts if artifact.feature == feature_id
    )


def format_plan(plan: Plan, catalog: dict[str, Any], download_models: Any, manifest: list[dict[str, Any]]) -> str:
    preset = preset_by_id(catalog, plan.preset_id)
    lines = [
        "Zone setup — models",
        "",
        f"RAM:  {format_ram(plan.ram_bytes)} detected    chat preset: {plan.preset_id} ({plan.fast} / {plan.reason})",
        f"Disk: {format_disk(plan.disk_free_bytes)} free",
        "",
        f"{'Feature':<16}{'Size':<12}Notes",
    ]
    for feature in catalog["features"]:
        feature_id = str(feature["id"])
        mark = "*" if feature_id in plan.features else " "
        size = feature_isolation_bytes(
            catalog, manifest, download_models, feature_id, preset
        )
        note = str(feature.get("description") or "")
        if feature_id == "vision":
            note = f"needs {format_ram(int(catalog['vision_min_ram_bytes']))} RAM"
        lines.append(
            f"{mark} {feature_id:<14}{format_disk(size):<12}{note}"
        )
    lines.append("")
    heading = "All features" if plan.wants_all else "Selected features"
    lines.append(disk_report(plan, title=heading + ":"))
    if plan.licenses:
        lines.append("")
        lines.append("Licenses:")
        for license_name in plan.licenses:
            lines.append(f"  {license_name}")
    return "\n".join(lines)


def make_plan(
    catalog: dict[str, Any],
    manifest: list[dict[str, Any]],
    download_models: Any,
    *,
    features_raw: str,
    preset_id: str | None,
    ram_bytes: int,
    disk_free_bytes: int,
    models_dir: Path,
    ollama_tags: set[str],
    comfy_present: set[str] | None,
) -> Plan:
    selected, wants_all = parse_features(features_raw, catalog)
    preset = preset_by_id(catalog, preset_id) if preset_id else pick_preset(catalog, ram_bytes)
    features = feature_map(catalog)
    names = ollama_names(catalog, selected, preset)
    install_trainer = any(
        features[feature_id].get("install_trainer") for feature_id in selected
    )
    env = {
        "OLLAMA_MODEL_FAST": names["fast"],
        "OLLAMA_MODEL_REASON": names["reason"],
        "OLLAMA_MODEL_EMBED": names["embed"],
        "OLLAMA_MODEL_VISION": names.get("vision", ""),
        "ZONE_SETUP_RAM_BYTES": str(ram_bytes),
    }
    artifacts = tuple(
        build_artifacts(
            catalog,
            manifest,
            download_models,
            selected,
            preset,
            models_dir=models_dir,
            ollama_tags=ollama_tags,
            comfy_present=comfy_present,
        )
    )
    return Plan(
        features=tuple(
            feature_id for feature_id in all_feature_ids(catalog) if feature_id in selected
        ),
        wants_all=wants_all,
        preset_id=str(preset["id"]),
        artifacts=artifacts,
        licenses=licenses_for(catalog, selected),
        ram_bytes=ram_bytes,
        disk_free_bytes=disk_free_bytes,
        disk_margin_bytes=int(catalog["disk_margin_bytes"]),
        install_trainer=bool(install_trainer),
        comfy_bundles=tuple(sorted(bundles_for_features(catalog, selected))),
        env=env,
        fast=names["fast"],
        reason=names["reason"],
        embed=names["embed"],
        vision=names.get("vision"),
    )


def enforce_gates(plan: Plan, catalog: dict[str, Any]) -> None:
    vision_min = int(catalog["vision_min_ram_bytes"])
    if "vision" in plan.features and plan.ram_bytes < vision_min:
        if plan.wants_all:
            raise PlanError(
                "Cannot select all features: vision needs "
                f"{format_ram(vision_min)} RAM (llava:7b). This machine has "
                f"{format_ram(plan.ram_bytes)}. 8 GB can run chat only: "
                "--features chat --chat-preset 8gb",
                code="all-ram",
            )
        raise PlanError(
            "Vision needs "
            f"{format_ram(vision_min)} RAM (llava:7b). This machine has "
            f"{format_ram(plan.ram_bytes)}. Drop vision or use a machine with at least 16 GB.",
            code="vision-ram",
        )
    if plan.disk_free_bytes < plan.required_free_bytes:
        report = disk_report(
            plan,
            title="All features:" if plan.wants_all else "Selected features:",
        )
        if plan.wants_all:
            raise PlanError(
                "Cannot select all features: not enough disk.\n\n"
                f"{report}\n\n"
                "all is blocked until there is enough free space for every model. "
                "Pass a smaller list with --features (chat is always on), for example "
                "--features chat or --features chat,vision,pictures.",
                code="all-disk",
            )
        raise PlanError(
            "Not enough disk for the selected features.\n\n"
            f"{report}\n\n"
            "Drop features with --features until Free now covers Free required.",
            code="disk",
        )


def resolve_plan(
    catalog: dict[str, Any],
    manifest: list[dict[str, Any]],
    download_models: Any,
    *,
    features_raw: str,
    preset_id: str | None,
    ram_bytes: int,
    disk_free_bytes: int,
    models_dir: Path,
    ollama_tags: set[str],
    comfy_present: set[str] | None,
) -> Plan:
    plan = make_plan(
        catalog,
        manifest,
        download_models,
        features_raw=features_raw,
        preset_id=preset_id,
        ram_bytes=ram_bytes,
        disk_free_bytes=disk_free_bytes,
        models_dir=models_dir,
        ollama_tags=ollama_tags,
        comfy_present=comfy_present,
    )
    enforce_gates(plan, catalog)
    return plan


def upsert_env(path: Path, updates: dict[str, str]) -> None:
    if path.exists():
        lines = path.read_text(encoding="utf-8").splitlines()
    else:
        lines = []
    seen: set[str] = set()
    written: list[str] = []
    for line in lines:
        stripped = line.strip()
        if not stripped or stripped.startswith("#") or "=" not in line:
            written.append(line)
            continue
        key = line.split("=", 1)[0]
        if key in updates:
            written.append(f"{key}={updates[key]}")
            seen.add(key)
        else:
            written.append(line)
    for key, value in updates.items():
        if key not in seen:
            written.append(f"{key}={value}")
    path.write_text("\n".join(written) + "\n", encoding="utf-8")


def ollama_tags(host: str) -> set[str]:
    request = urllib.request.Request(f"{host.rstrip('/')}/api/tags")
    with urllib.request.urlopen(request, timeout=5) as response:
        payload = json.loads(response.read().decode())
    names: set[str] = set()
    for model in payload.get("models") or []:
        name = str(model.get("name") or "")
        if name:
            names.add(name)
    return names


def wait_for_ollama(host: str, attempts: int = 30, interval: float = 5.0) -> None:
    last = ""
    for attempt in range(1, attempts + 1):
        try:
            ollama_tags(host)
            return
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, OSError) as error:
            last = str(error)
            print(
                f"Waiting for Ollama at {host} ({attempt}/{attempts}). "
                "Start it with: ollama serve",
                file=sys.stderr,
            )
            time.sleep(interval)
    raise PlanError(
        f"Ollama is not reachable at {host}: {last}. Start it with: ollama serve",
        code="ollama",
    )


def pull_ollama(
    host: str,
    name: str,
    *,
    index: int,
    count: int,
    size_bytes: int,
    download_models: Any,
) -> None:
    body = json.dumps({"model": name, "stream": True}).encode()
    request = urllib.request.Request(
        f"{host.rstrip('/')}/api/pull",
        data=body,
        method="POST",
        headers={"Content-Type": "application/json"},
    )
    started = time.monotonic()
    origin = 0
    with urllib.request.urlopen(request, timeout=300) as response:
        for raw in response:
            event = json.loads(raw.decode())
            if event.get("error"):
                raise PlanError(str(event["error"]), code="ollama-pull")
            status = str(event.get("status") or "")
            completed = int(event.get("completed") or 0)
            total = int(event.get("total") or size_bytes or 0)
            if status == "success":
                completed = total or size_bytes
            payload = download_models.progress_event(
                {"id": name},
                completed,
                total or size_bytes,
                index=index,
                count=count,
                started_at=started,
                origin_bytes=origin,
            )
            print(f"\r{download_models.format_progress(payload)}", end="", flush=True)
            if status == "success":
                print()
                return
    print()
    raise PlanError(f"Ollama pull for {name} ended before success", code="ollama-pull")


def install_comfy_runtime(plan: Plan) -> None:
    if not plan.comfy_bundles:
        return
    if platform.system() == "Darwin" and platform.machine() == "arm64":
        subprocess.run(["sh", str(MACOS_INSTALLER)], check=True)
        if plan.install_trainer:
            subprocess.run(["sh", str(MACOS_INSTALLER), "--install-trainer"], check=True)
        return
    print(
        "ComfyUI runtime is not auto-installed on this platform. "
        "Weights still download into the models directory. "
        "Use --profile bundled-comfyui on Linux NVIDIA, or point "
        "COMFYUI_BASE_URL at a host ComfyUI.",
        file=sys.stderr,
    )


def download_comfy(
    plan: Plan,
    manifest: list[dict[str, Any]],
    download_models: Any,
    models_dir: Path,
    *,
    start_index: int,
    count: int,
) -> list[str]:
    failed: list[str] = []
    by_id = {str(model["id"]): model for model in manifest}
    comfy_artifacts = [artifact for artifact in plan.artifacts if artifact.kind == "comfy"]
    for offset, artifact in enumerate(comfy_artifacts):
        index = start_index + offset
        model = by_id[artifact.identifier]
        target = download_models.checked_target(models_dir, str(model["relative_path"]))
        if artifact.present:
            print(f"[{index}/{count}] {artifact.identifier}: skipped (verified)")
            continue
        print(f"[{index}/{count}] {artifact.identifier}: downloading")
        try:
            download_models.download(
                model,
                target,
                progress_mode="text",
                index=index,
                count=count,
            )
            valid, detail = download_models.verify(target, model)
            if not valid:
                target.unlink(missing_ok=True)
                print(f"[{index}/{count}] {artifact.identifier}: {detail}", file=sys.stderr)
                failed.append(artifact.identifier)
            else:
                print(f"[{index}/{count}] {artifact.identifier}: verified")
        except (OSError, RuntimeError, urllib.error.URLError, ValueError) as error:
            print(f"[{index}/{count}] {artifact.identifier}: {error}", file=sys.stderr)
            failed.append(artifact.identifier)
    return failed


def confirm(prompt: str, assume_yes: bool) -> bool:
    if assume_yes:
        return True
    if not sys.stdin.isatty():
        raise PlanError("refusing to confirm without a TTY; pass --yes", code="confirm")
    reply = input(f"{prompt} [Y/n] ").strip().lower()
    return reply in ("", "y", "yes")


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--yes", action="store_true", help="accept defaults and licenses")
    parser.add_argument("--features", default="", help="comma-separated features, or all")
    parser.add_argument("--chat-preset", dest="chat_preset", help="8gb, 16gb, or 32gb")
    parser.add_argument("--env", type=Path, help="path to .env to update")
    parser.add_argument("--models-dir", type=Path, dest="models_dir")
    parser.add_argument("--catalog", type=Path, default=CATALOG_PATH)
    parser.add_argument("--manifest", type=Path, default=MANIFEST_PATH)
    parser.add_argument("--ollama-host", default=DEFAULT_OLLAMA_HOST)
    parser.add_argument("--ram-bytes", type=int)
    parser.add_argument("--disk-free-bytes", type=int)
    parser.add_argument("--ollama-tags", default=None, help="comma-separated installed Ollama names")
    parser.add_argument("--comfy-present", default=None, help="comma-separated installed Comfy ids")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--skip-download", action="store_true")
    parser.add_argument("--skip-comfy-install", action="store_true")
    parser.add_argument("--skip-ollama-wait", action="store_true")
    return parser.parse_args(argv)


def split_csv(raw: str | None) -> set[str] | None:
    if raw is None:
        return None
    return {part.strip() for part in raw.split(",") if part.strip()}


def main(argv: list[str] | None = None) -> int:
    try:
        return run_setup(argv)
    except PlanError as error:
        print(str(error), file=sys.stderr)
        return 2


def run_setup(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    catalog = load_json(args.catalog)
    if catalog.get("schema_version") != 1:
        raise PlanError("unsupported setup-features.json schema", code="catalog")
    download_models = load_download_models()
    manifest = download_models.load_manifest(args.manifest)
    models_dir = (args.models_dir or default_models_dir()).expanduser()
    ram_bytes = args.ram_bytes if args.ram_bytes is not None else detect_ram_bytes()
    disk_free_bytes = (
        args.disk_free_bytes
        if args.disk_free_bytes is not None
        else detect_disk_free(models_dir)
    )
    tags = split_csv(args.ollama_tags)
    comfy_present = split_csv(args.comfy_present)
    if tags is None and not args.dry_run and not args.skip_ollama_wait:
        wait_for_ollama(args.ollama_host)
        tags = ollama_tags(args.ollama_host)
    if tags is None:
        tags = set()

    features_raw = args.features
    if not args.yes and sys.stdin.isatty() and not args.features:
        preview = make_plan(
            catalog,
            manifest,
            download_models,
            features_raw="all",
            preset_id=args.chat_preset,
            ram_bytes=ram_bytes,
            disk_free_bytes=disk_free_bytes,
            models_dir=models_dir,
            ollama_tags=tags,
            comfy_present=comfy_present,
        )
        print(format_plan(preview, catalog, download_models, manifest))
        print()
        try:
            enforce_gates(preview, catalog)
            default_hint = "all"
        except PlanError as error:
            print(str(error), file=sys.stderr)
            print()
            default_hint = "chat"
        reply = input(f"Features [{default_hint}]: ").strip()
        features_raw = reply if reply else default_hint
        if args.chat_preset is None:
            suggested = pick_preset(catalog, ram_bytes)["id"]
            chosen = input(f"Chat preset [{suggested}]: ").strip()
            args.chat_preset = chosen or suggested

    if not features_raw:
        features_raw = "all"

    plan = resolve_plan(
        catalog,
        manifest,
        download_models,
        features_raw=features_raw,
        preset_id=args.chat_preset,
        ram_bytes=ram_bytes,
        disk_free_bytes=disk_free_bytes,
        models_dir=models_dir,
        ollama_tags=tags,
        comfy_present=comfy_present,
    )
    print(format_plan(plan, catalog, download_models, manifest))
    print()
    if args.dry_run:
        return 0
    if not confirm("Download these models and accept the licenses?", args.yes):
        print("Setup cancelled.")
        return 1
    if args.env is not None:
        upsert_env(args.env, plan.env)
    if args.skip_download:
        if args.env is not None and plan.comfy_bundles:
            upsert_env(args.env, {"COMFYUI_ENABLED": "true"})
        return 0

    count = len(plan.artifacts)
    print(f"Downloading {count} artifacts ({format_disk(plan.needed_bytes)} remaining)")
    failed: list[str] = []
    ollama_artifacts = [artifact for artifact in plan.artifacts if artifact.kind == "ollama"]
    for index, artifact in enumerate(ollama_artifacts, start=1):
        if artifact.present:
            print(f"[{index}/{count}] {artifact.identifier}: skipped (installed)")
            continue
        try:
            pull_ollama(
                args.ollama_host,
                artifact.identifier,
                index=index,
                count=count,
                size_bytes=artifact.size_bytes,
                download_models=download_models,
            )
        except PlanError as error:
            print(f"[{index}/{count}] {artifact.identifier}: {error}", file=sys.stderr)
            failed.append(artifact.identifier)

    if plan.comfy_bundles and not args.skip_comfy_install:
        install_comfy_runtime(plan)
    failed.extend(
        download_comfy(
            plan,
            manifest,
            download_models,
            models_dir,
            start_index=len(ollama_artifacts) + 1,
            count=count,
        )
    )

    comfy_ok = [
        artifact
        for artifact in plan.artifacts
        if artifact.kind == "comfy" and artifact.identifier not in failed
    ]
    if args.env is not None:
        enabled = "true" if comfy_ok else "false"
        upsert_env(args.env, {"COMFYUI_ENABLED": enabled})
    if failed:
        print("Some models failed: " + ", ".join(failed), file=sys.stderr)
        return 1
    print("Model setup complete.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except PlanError as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(2) from error
    except subprocess.CalledProcessError as error:
        print(f"ComfyUI installer failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
