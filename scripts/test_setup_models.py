#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPT = Path(__file__).with_name("setup-models.py")
CATALOG = Path(__file__).with_name("setup-features.json")
MANIFEST = Path(__file__).resolve().parent.parent / "comfyui" / "model-manifest.json"

SPEC = importlib.util.spec_from_file_location("setup_models", SCRIPT)
assert SPEC and SPEC.loader
setup_models = importlib.util.module_from_spec(SPEC)
sys.modules["setup_models"] = setup_models
SPEC.loader.exec_module(setup_models)

GIB = 1024**3
GB = 1_000_000_000


def load() -> tuple[dict, list, object]:
    catalog = setup_models.load_json(CATALOG)
    download_models = setup_models.load_download_models()
    manifest = download_models.load_manifest(MANIFEST)
    return catalog, manifest, download_models


class SetupModelsTest(unittest.TestCase):
    def setUp(self) -> None:
        self.catalog, self.manifest, self.download_models = load()

    def plan(
        self,
        features: str,
        *,
        ram: int,
        disk: int,
        preset: str | None = None,
        tags: set[str] | None = None,
        comfy_present: set[str] | None = None,
    ) -> setup_models.Plan:
        return setup_models.resolve_plan(
            self.catalog,
            self.manifest,
            self.download_models,
            features_raw=features,
            preset_id=preset,
            ram_bytes=ram,
            disk_free_bytes=disk,
            models_dir=Path("."),
            ollama_tags=tags or set(),
            comfy_present=comfy_present if comfy_present is not None else set(),
        )

    def test_catalog_bundles_and_models_exist(self) -> None:
        valid = self.download_models.VALID_BUNDLES
        ollama_models = self.catalog["ollama_models"]
        for feature in self.catalog["features"]:
            for bundle in feature.get("comfy_bundles") or []:
                self.assertIn(bundle, valid, feature["id"])
            unless = feature.get("comfy_bundles_unless") or {}
            for extra in unless.values():
                for bundle in extra:
                    self.assertIn(bundle, valid, feature["id"])
        for preset in self.catalog["chat_presets"]:
            self.assertIn(preset["fast"], ollama_models)
            self.assertIn(preset["reason"], ollama_models)
        self.assertIn(self.catalog["embed"], ollama_models)
        self.assertIn(self.catalog["vision_model"], ollama_models)
        vision = next(feature for feature in self.catalog['features'] if feature['id'] == 'vision')
        self.assertEqual(vision['min_ram_bytes'], 16 * GIB)
        self.assertNotIn('vision_min_ram_bytes', self.catalog)

    def test_empty_features_means_all(self) -> None:
        selected, wants_all = setup_models.parse_features("", self.catalog)
        self.assertTrue(wants_all)
        self.assertEqual(selected, set(setup_models.all_feature_ids(self.catalog)))

    def test_chat_is_always_included(self) -> None:
        selected, wants_all = setup_models.parse_features("pictures", self.catalog)
        self.assertFalse(wants_all)
        self.assertIn("chat", selected)
        self.assertIn("pictures", selected)
        self.assertNotIn("video", selected)

    def test_unknown_feature_is_rejected(self) -> None:
        with self.assertRaises(setup_models.PlanError) as raised:
            setup_models.parse_features("chat,nope", self.catalog)
        self.assertEqual(raised.exception.code, "unknown-feature")

    def test_all_on_8gb_ram_is_blocked(self) -> None:
        with self.assertRaises(setup_models.PlanError) as raised:
            self.plan("all", ram=8 * GIB, disk=500 * GB, preset="8gb")
        self.assertEqual(raised.exception.code, "all-ram")
        self.assertIn("Cannot select all features", str(raised.exception))
        self.assertIn("16 GB", str(raised.exception))
        self.assertIn("llava:7b", str(raised.exception))

    def test_vision_on_8gb_ram_is_blocked(self) -> None:
        with self.assertRaises(setup_models.PlanError) as raised:
            self.plan("chat,vision", ram=8 * GIB, disk=500 * GB, preset="8gb")
        self.assertEqual(raised.exception.code, "vision-ram")
        self.assertIn("llava:7b", str(raised.exception))

    def test_vision_on_16gb_is_allowed(self) -> None:
        plan = self.plan("chat,vision", ram=16 * GIB, disk=500 * GB, preset="16gb")
        self.assertEqual(plan.vision, "llava:7b")
        self.assertEqual(plan.env["OLLAMA_MODEL_VISION"], "llava:7b")
        self.assertEqual(plan.env["ZONE_SETUP_RAM_BYTES"], str(16 * GIB))

    def test_video_needs_64_gib_ram(self) -> None:
        with self.assertRaises(setup_models.PlanError) as raised:
            self.plan('chat,video', ram=32 * GIB, disk=500 * GB, preset='32gb')
        self.assertEqual(raised.exception.code, 'video-ram')
        self.assertIn('64 GB', str(raised.exception))
        self.assertIn('Wan 2.2 A14B', str(raised.exception))

    def test_video_on_exactly_64_gib_is_allowed(self) -> None:
        plan = self.plan('chat,video', ram=68719476736, disk=500 * GB, preset='32gb')
        self.assertIn('video', plan.features)
        self.assertIn('video', plan.comfy_bundles)

    def test_all_on_32gb_ram_is_blocked_by_video(self) -> None:
        with self.assertRaises(setup_models.PlanError) as raised:
            self.plan('all', ram=32 * GIB, disk=500 * GB, preset='32gb')
        self.assertEqual(raised.exception.code, 'all-ram')
        self.assertIn('Cannot select all features', str(raised.exception))
        self.assertIn('video needs 64 GB', str(raised.exception))

    def bare_video(self) -> dict:
        video = next(feature for feature in self.catalog['features'] if feature['id'] == 'video')
        return {
            key: value
            for key, value in video.items()
            if key not in ('ram_model', 'ram_fallback')
        }

    def unchecked_plan(self, features: str, *, ram: int) -> setup_models.Plan:
        return setup_models.make_plan(
            self.catalog,
            self.manifest,
            self.download_models,
            features_raw=features,
            preset_id='32gb',
            ram_bytes=ram,
            disk_free_bytes=500 * GB,
            models_dir=Path('.'),
            ollama_tags=set(),
            comfy_present=set(),
        )

    def test_a_gated_feature_without_a_model_names_its_label(self) -> None:
        plan = self.unchecked_plan('chat,video', ram=32 * GIB)

        error = setup_models.ram_error(plan, self.bare_video())

        self.assertEqual(error.code, 'video-ram')
        self.assertIn('Video needs 64 GB RAM (Video)', str(error))

    def test_a_gated_feature_without_a_fallback_still_says_how_to_drop_it(self) -> None:
        plan = self.unchecked_plan('all', ram=32 * GIB)

        error = setup_models.ram_error(plan, self.bare_video())

        self.assertEqual(error.code, 'all-ram')
        features = str(error).split('--features ', 1)[1].split()[0]
        selected, _ = setup_models.parse_features(features, self.catalog)
        self.assertNotIn('video', selected)
        self.assertEqual(
            selected,
            set(setup_models.all_feature_ids(self.catalog)) - {'video'},
        )

    def test_every_ram_fallback_drops_its_feature(self) -> None:
        for feature in self.catalog['features']:
            if 'min_ram_bytes' not in feature:
                continue
            with self.subTest(feature=feature['id']):
                fallback = feature['ram_fallback']
                features = fallback.split('--features ', 1)[1].split()[0]
                selected, _ = setup_models.parse_features(features, self.catalog)
                self.assertNotIn(feature['id'], selected)

    def test_subset_on_short_disk_is_blocked(self) -> None:
        with self.assertRaises(setup_models.PlanError) as raised:
            self.plan("pictures", ram=64 * GIB, disk=1 * GB, preset="32gb")
        self.assertEqual(raised.exception.code, "disk")
        self.assertIn("Not enough disk", str(raised.exception))
        self.assertNotIn("Cannot select all features", str(raised.exception))

    def test_chat_only_on_8gb_is_allowed(self) -> None:
        plan = self.plan("chat", ram=8 * GIB, disk=500 * GB, preset="8gb")
        self.assertEqual(plan.features, ("chat",))
        self.assertEqual(plan.fast, "llama3.2:3b")
        self.assertEqual(plan.reason, "deepseek-r1:7b")
        self.assertIsNone(plan.vision)
        self.assertEqual(plan.comfy_bundles, ())
        self.assertEqual(plan.env["OLLAMA_MODEL_VISION"], "")

    def test_all_on_short_disk_is_blocked(self) -> None:
        with self.assertRaises(setup_models.PlanError) as raised:
            self.plan("all", ram=64 * GIB, disk=20 * GB, preset="32gb")
        self.assertEqual(raised.exception.code, "all-disk")
        message = str(raised.exception)
        self.assertIn("Cannot select all features", message)
        self.assertIn("Free required", message)
        self.assertIn("Free now", message)
        self.assertIn("Short by", message)
        self.assertIn("all is blocked", message)

    def test_pictures_plan_only_image_people_bundle(self) -> None:
        plan = self.plan(
            "pictures", ram=64 * GIB, disk=500 * GB, preset="32gb"
        )
        self.assertEqual(plan.comfy_bundles, ("image-people",))
        identifiers = [artifact.identifier for artifact in plan.artifacts]
        self.assertIn("lustify-sdxl-ggwp-v7", identifiers)
        self.assertNotIn("flux1-schnell-fp8", identifiers)
        self.assertIn("llama3.1:8b", identifiers)
        self.assertNotIn("llava:7b", identifiers)
        self.assertNotIn("wan2.2-ti2v-5b", identifiers)

    def test_edits_and_train_share_dev_once(self) -> None:
        plan = self.plan("edits,train", ram=64 * GIB, disk=500 * GB, preset="32gb")
        identifiers = [artifact.identifier for artifact in plan.artifacts if artifact.kind == "comfy"]
        self.assertEqual(identifiers.count("flux1-dev-fp8"), 1)
        self.assertIn("image-dev", plan.comfy_bundles)
        self.assertIn("image-people", plan.comfy_bundles)
        self.assertTrue(plan.install_trainer)

    def test_train_without_edits_adds_image_dev(self) -> None:
        plan = self.plan("train", ram=64 * GIB, disk=500 * GB, preset="32gb")
        self.assertIn("image-dev", plan.comfy_bundles)

    def test_present_files_reduce_needed_bytes(self) -> None:
        missing = self.plan(
            "pictures", ram=64 * GIB, disk=500 * GB, preset="32gb", comfy_present=set()
        )
        present = self.plan(
            "pictures",
            ram=64 * GIB,
            disk=500 * GB,
            preset="32gb",
            comfy_present={"lustify-sdxl-ggwp-v7"},
            tags={"llama3.1:8b", "deepseek-r1:32b", "qwen3-embedding:0.6b"},
        )
        self.assertGreater(missing.needed_bytes, present.needed_bytes)
        self.assertEqual(present.needed_bytes, 0)
        self.assertEqual(present.required_free_bytes, 0)

    def test_format_plan_lists_disk_and_ram(self) -> None:
        plan = self.plan("pictures,vision", ram=64 * GIB, disk=500 * GB, preset="32gb")
        text = setup_models.format_plan(
            plan, self.catalog, self.download_models, self.manifest
        )
        self.assertIn("Free required", text)
        self.assertIn("Free now", text)
        self.assertIn("vision", text)
        self.assertIn("16 GB RAM", text)
        self.assertNotIn("FLUX.1-dev Non-Commercial License", text)
        self.assertIn("CreativeML Open RAIL-M", text)
        self.assertNotIn("Apache-2.0", text)

    def test_format_plan_only_lists_restricted_licenses(self) -> None:
        plan = self.plan("all", ram=64 * GIB, disk=500 * GB, preset="32gb")
        text = setup_models.format_plan(
            plan, self.catalog, self.download_models, self.manifest
        )
        self.assertNotIn("Apache-2.0", text)
        self.assertNotIn("BSD-3-Clause", text)
        self.assertIn("FLUX.1-dev Non-Commercial License", text)
        self.assertIn("CreativeML Open RAIL-M (FLUX uncensored LoRA)", text)
        self.assertIn("CreativeML Open RAIL++-M (Qwen edit LoRA)", text)
        self.assertIn("CreativeML Open RAIL-M (SDXL people checkpoint)", text)
        self.assertEqual(
            list(plan.licenses),
            [
                "CreativeML Open RAIL-M (SDXL people checkpoint)",
                "FLUX.1-dev Non-Commercial License",
                "CreativeML Open RAIL++-M (Qwen edit LoRA)",
                "CreativeML Open RAIL-M (FLUX uncensored LoRA)",
            ],
        )

    def test_models_directory_setting_overrides_the_default(self) -> None:
        with mock.patch.dict(
            os.environ, {"COMFYUI_MODELS_DIRECTORY": "/srv/comfyui/models"}, clear=False
        ):
            os.environ.pop("COMFYUI_MODELS_DIR", None)
            self.assertEqual(setup_models.default_models_dir(), Path("/srv/comfyui/models"))

    def test_the_abbreviated_models_name_is_no_longer_read(self) -> None:
        with mock.patch.dict(
            os.environ, {"COMFYUI_MODELS_DIR": "/srv/comfyui/models"}, clear=False
        ):
            os.environ.pop("COMFYUI_MODELS_DIRECTORY", None)
            self.assertNotEqual(setup_models.default_models_dir(), Path("/srv/comfyui/models"))

    def test_pick_preset_follows_ram(self) -> None:
        self.assertEqual(setup_models.pick_preset(self.catalog, 8 * GIB)["id"], "8gb")
        self.assertEqual(setup_models.pick_preset(self.catalog, 16 * GIB)["id"], "16gb")
        self.assertEqual(setup_models.pick_preset(self.catalog, 64 * GIB)["id"], "32gb")

    def test_dry_run_all_fails_closed_on_short_disk(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            env_path = Path(directory) / ".env"
            env_path.write_text("COMFYUI_ENABLED=false\n", encoding="utf-8")
            code = setup_models.main(
                [
                    "--yes",
                    "--dry-run",
                    "--features",
                    "all",
                    "--chat-preset",
                    "32gb",
                    "--ram-bytes",
                    str(64 * GIB),
                    "--disk-free-bytes",
                    str(20 * GB),
                    "--ollama-tags",
                    "",
                    "--comfy-present",
                    "",
                    "--env",
                    str(env_path),
                    "--models-dir",
                    directory,
                    "--skip-ollama-wait",
                ]
            )
        self.assertEqual(code, 2)

    def test_skip_download_writes_env(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            env_path = Path(directory) / ".env"
            env_path.write_text("COMFYUI_ENABLED=false\n", encoding="utf-8")
            code = setup_models.main(
                [
                    "--yes",
                    "--features",
                    "chat,pictures",
                    "--chat-preset",
                    "16gb",
                    "--ram-bytes",
                    str(32 * GIB),
                    "--disk-free-bytes",
                    str(500 * GB),
                    "--ollama-tags",
                    "",
                    "--comfy-present",
                    "",
                    "--env",
                    str(env_path),
                    "--models-dir",
                    directory,
                    "--skip-download",
                    "--skip-ollama-wait",
                    "--skip-comfy-install",
                ]
            )
            self.assertEqual(code, 0)
            text = env_path.read_text(encoding="utf-8")
        self.assertIn("OLLAMA_MODEL_FAST=llama3.1:8b", text)
        self.assertIn("OLLAMA_MODEL_REASON=deepseek-r1:14b", text)
        self.assertIn("OLLAMA_MODEL_EMBED=qwen3-embedding:0.6b", text)
        self.assertIn("OLLAMA_MODEL_VISION=", text)
        self.assertIn("COMFYUI_ENABLED=true", text)

    def test_all_union_is_every_manifest_bundle_except_optional_flux_schnell(self) -> None:
        plan = make_all_plan(self)
        self.assertEqual(
            set(plan.comfy_bundles),
            set(self.download_models.VALID_BUNDLES) - {"image"},
        )
        self.assertGreater(plan.total_bytes, 160 * GB)
        self.assertLess(plan.total_bytes, 170 * GB)
        self.assertGreater(plan.required_free_bytes, 170 * GB)
        self.assertLess(plan.required_free_bytes, 180 * GB)


def make_all_plan(test: SetupModelsTest) -> setup_models.Plan:
    return setup_models.make_plan(
        test.catalog,
        test.manifest,
        test.download_models,
        features_raw="all",
        preset_id="32gb",
        ram_bytes=64 * GIB,
        disk_free_bytes=500 * GB,
        models_dir=Path("."),
        ollama_tags=set(),
        comfy_present=set(),
    )


if __name__ == "__main__":
    unittest.main()
