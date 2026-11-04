from __future__ import annotations

import contextlib
import io
import json
import os
import random
import subprocess
import sys
import tempfile
import threading
import unittest
import uuid
from pathlib import Path
from unittest import mock

COMFYUI = Path(__file__).parents[1]
if str(COMFYUI) not in sys.path:
    sys.path.insert(0, str(COMFYUI))

import train_sdxl  # noqa: E402

SCRIPT = COMFYUI / 'train_sdxl.py'
CLAIM = 'claimed'
BASE = 'sdxl-people'


def queued_job(
    name: str = 'jerry',
    method: str = 'lora',
    trigger: str = 'ohwx',
    extra: dict | None = None,
) -> dict:
    job = {
        'schema_version': 1,
        'id': str(uuid.uuid4()),
        'name': name,
        'method': method,
        'trigger': trigger,
        'checkpoint': 'lustifySDXLNSFW_ggwpV7.safetensors',
        'status': 'queued',
        'error': None,
        'step': None,
        'total': None,
        'pid': None,
        'started_at': '2026-10-04T00:00:00Z',
        'base': BASE,
    }
    if extra:
        job.update(extra)
    return job


def legacy_job(method: str = 'lora') -> dict:
    job = queued_job(method=method)
    del job['base']
    return job


def write_dataset(job_dir: Path, count: int = 1) -> None:
    dataset = job_dir / 'dataset'
    dataset.mkdir(parents=True)
    for index in range(count):
        (dataset / f'{index:04}.png').write_bytes(b'png')
        (dataset / f'{index:04}.txt').write_text('ohwx person, studio', encoding='utf-8')


class ProgressTests(unittest.TestCase):
    def test_weighted_percent_covers_fine_tune_prep_phases(self) -> None:
        self.assertEqual(train_sdxl.overall_percent('lora', 'loading', 0, 1), 0)
        self.assertEqual(train_sdxl.overall_percent('lora', 'loading', 1, 1), 6)
        self.assertEqual(train_sdxl.overall_percent('lora', 'class_images', 8, 8), 14)
        self.assertEqual(train_sdxl.overall_percent('lora', 'encoding', 4, 8), 18)
        self.assertEqual(train_sdxl.overall_percent('lora', 'training', 50, 100), 58)
        self.assertEqual(train_sdxl.overall_percent('lora', 'publishing', 1, 1), 100)
        self.assertEqual(train_sdxl.overall_percent('pivotal', 'encoding', 4, 8), 18)
        self.assertEqual(train_sdxl.overall_percent('finetune', 'class_images', 0, 10), 4)
        self.assertEqual(train_sdxl.overall_percent('finetune', 'class_images', 5, 10), 10)
        self.assertEqual(train_sdxl.overall_percent('finetune', 'encoding', 0, 10), 16)
        self.assertEqual(train_sdxl.overall_percent('finetune', 'training', 0, 8000), 22)
        self.assertEqual(train_sdxl.overall_percent('finetune', 'training', 4000, 8000), 58)
        self.assertEqual(train_sdxl.overall_percent('finetune', 'publishing', 1, 1), 100)

    def test_eta_scales_remaining_steps_by_elapsed_time(self) -> None:
        self.assertEqual(train_sdxl.eta_seconds(50, 10, 20), 50)
        self.assertIsNone(train_sdxl.eta_seconds(5, 0, 20))
        self.assertIsNone(train_sdxl.eta_seconds(5, 20, 20))

    def test_emit_writes_phase_message_percent_and_keeps_step_zero_during_prep(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            job_dir = Path(directory)
            progress = train_sdxl.Progress(job_dir, 'finetune', 8000)
            progress.emit('class_images', phase_step=12, phase_total=2000)
            payload = json.loads((job_dir / 'progress.json').read_text(encoding='utf-8'))
            self.assertEqual(payload['step'], 0)
            self.assertEqual(payload['total'], 8000)
            self.assertEqual(payload['phase'], 'class_images')
            self.assertEqual(payload['message'], 'Generating class image 12 of 2000')
            self.assertEqual(payload['percent'], 4)
            self.assertEqual(payload['phase_step'], 12)
            self.assertEqual(payload['phase_total'], 2000)


class StepsForTests(unittest.TestCase):
    def setUp(self) -> None:
        self.config = train_sdxl.load_config()

    def test_budget_is_unique_images_times_twenty_clamped_500_40000(self) -> None:
        self.assertEqual(self.config['passes_per_image'], 20)
        self.assertEqual(self.config['min_steps'], 500)
        self.assertEqual(self.config['max_steps'], 40000)
        self.assertEqual(train_sdxl.steps_for(1, self.config), 500)
        self.assertEqual(train_sdxl.steps_for(25, self.config), 500)
        self.assertEqual(train_sdxl.steps_for(26, self.config), 520)
        self.assertEqual(train_sdxl.steps_for(400, self.config), 8000)
        self.assertEqual(train_sdxl.steps_for(500, self.config), 10000)
        self.assertEqual(train_sdxl.steps_for(2000, self.config), 40000)
        self.assertEqual(train_sdxl.steps_for(2500, self.config), 40000)


class FindJobTests(unittest.TestCase):
    def test_finds_a_queued_job_in_zone_train(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            train_sdxl.write_job(job_dir, queued_job())
            write_dataset(job_dir)
            self.assertEqual(train_sdxl.find_job(models), job_dir)

    def test_ignores_succeeded_jobs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            job = queued_job()
            job['status'] = 'succeeded'
            train_sdxl.write_job(job_dir, job)
            self.assertIsNone(train_sdxl.find_job(models))

    def test_skips_a_running_job_with_a_live_pid(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            job = queued_job()
            job['status'] = 'running'
            job['pid'] = os.getpid()
            train_sdxl.write_job(job_dir, job)
            self.assertIsNone(train_sdxl.find_job(models))

    def test_resumes_a_running_job_with_a_dead_pid(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            job = queued_job()
            job['status'] = 'running'
            job['pid'] = train_sdxl.unused_pid()
            train_sdxl.write_job(job_dir, job)
            self.assertEqual(train_sdxl.find_job(models), job_dir)

    def test_queued_jobs_win_over_dead_running_jobs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            running = models / '.zone-train' / str(uuid.uuid4())
            queued = models / '.zone-train' / str(uuid.uuid4())
            running.mkdir(parents=True)
            queued.mkdir(parents=True)
            dead = queued_job(extra={'started_at': '2026-10-04T00:00:00Z'})
            dead['status'] = 'running'
            dead['pid'] = train_sdxl.unused_pid()
            train_sdxl.write_job(running, dead)
            train_sdxl.write_job(
                queued, queued_job(extra={'started_at': '2026-10-04T00:00:01Z'})
            )
            self.assertEqual(train_sdxl.find_job(models), queued)


class NormalizeAndPublishTests(unittest.TestCase):
    def test_older_jobs_get_filename_recipe_and_image_count(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            job_dir = Path(directory)
            write_dataset(job_dir, count=3)
            job = train_sdxl.normalize_job(
                {
                    'name': 'jerry',
                    'method': 'lora',
                    'trigger': 'ohwx',
                    'checkpoint': 'lustifySDXLNSFW_ggwpV7.safetensors',
                    'status': 'queued',
                },
                job_dir,
            )
            self.assertEqual(job['filename'], 'jerry.safetensors')
            self.assertEqual(job['recipe_id'], 'sdxl-adapter')
            self.assertEqual(
                job['hf_base'],
                'John6666/lustify-sdxl-nsfw-checkpoint-ggwp-v7-sdxl',
            )
            self.assertEqual(job['image_count'], 3)
            pivotal = train_sdxl.normalize_job(
                {
                    'name': 'jerry',
                    'method': 'pivotal',
                    'trigger': 'ohwx',
                    'checkpoint': 'lustifySDXLNSFW_ggwpV7.safetensors',
                    'status': 'queued',
                },
                job_dir,
            )
            self.assertEqual(pivotal['recipe_id'], 'sdxl-adapter')
            self.assertEqual(pivotal['filename'], 'jerry.safetensors')
            finetune = train_sdxl.normalize_job(
                {
                    'name': 'jerry',
                    'method': 'finetune',
                    'trigger': 'ohwx',
                    'checkpoint': 'lustifySDXLNSFW_ggwpV7.safetensors',
                    'status': 'queued',
                },
                job_dir,
            )
            self.assertEqual(finetune['recipe_id'], 'sdxl')

    def test_stub_writes_the_weight_into_the_job_directory_without_publishing_it(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / 'job'
            job = train_sdxl.normalize_job(queued_job(), job_dir)
            destination = train_sdxl.write_stub(models, job_dir, job)
            self.assertEqual(destination, job_dir / 'jerry.safetensors')
            self.assertTrue(destination.is_file())
            self.assertGreater(destination.stat().st_size, 0)
            self.assertFalse((models / 'loras').exists())
            self.assertEqual(list(job_dir.glob('*.zone.json')), [])

    def test_pivotal_stub_writes_the_embedding_beside_the_models(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / 'job'
            job = train_sdxl.normalize_job(queued_job(method='pivotal'), job_dir)
            destination = train_sdxl.write_stub(models, job_dir, job)
            self.assertEqual(destination, job_dir / 'jerry.safetensors')
            self.assertTrue((models / 'embeddings' / 'jerry.safetensors').is_file())
            self.assertFalse((models / 'loras').exists())


class CaptionDropoutTests(unittest.TestCase):
    def test_dropout_changes_some_captions_seed_stable(self) -> None:
        captions = ['ohwx person, studio'] * 100
        class_prompt = 'a photo of a person'
        first = []
        rng = random.Random(0)
        for caption in captions:
            first.append(train_sdxl.drop_caption(caption, class_prompt, 0.1, rng))
        rng = random.Random(0)
        second = [
            train_sdxl.drop_caption(caption, class_prompt, 0.1, rng) for caption in captions
        ]
        self.assertEqual(first, second)
        dropped = sum(1 for caption in first if caption == class_prompt)
        kept = sum(1 for caption in first if caption == 'ohwx person, studio')
        self.assertGreater(dropped, 0)
        self.assertGreater(kept, 0)
        self.assertEqual(dropped + kept, 100)
        unchanged = [
            train_sdxl.drop_caption(caption, class_prompt, 0.0, random.Random(0))
            for caption in captions
        ]
        self.assertEqual(unchanged, captions)


class MaskedLossTests(unittest.TestCase):
    def test_masked_loss_ignores_zero_mask_pixels(self) -> None:
        predicted = [0.0, 10.0, 0.0, 10.0]
        target = [0.0, 0.0, 0.0, 0.0]
        mask = [1.0, 0.0, 0.19, 0.0]
        self.assertEqual(train_sdxl.masked_mse(predicted, target, mask), 0.0)
        self.assertEqual(train_sdxl.masked_mse(predicted, target, None), 50.0)
        predicted = [3.0, 10.0]
        target = [0.0, 0.0]
        mask = [1.0, 0.0]
        self.assertEqual(train_sdxl.masked_mse(predicted, target, mask), 9.0)
        mask = [0.2, 0.0]
        self.assertEqual(train_sdxl.masked_mse(predicted, target, mask), 9.0)
        self.assertEqual(train_sdxl.masked_mse(predicted, target, [0.0, 0.0]), 0.0)


class PoseSamplingTests(unittest.TestCase):
    def test_missing_pose_files_stay_uniform(self) -> None:
        frames = [{'pose': '', 'kind': 'body'} for _ in range(8)]
        self.assertEqual(
            train_sdxl.pose_sample_weights(frames, {'kind_boost': 1.25, 'rebalance_share': 0.4}),
            [1.0] * 8,
        )

    def test_inverse_frequency_and_kind_boost(self) -> None:
        frames = [{'pose': 'standing', 'kind': 'body'}] * 4
        frames.append({'pose': 'sitting', 'kind': 'body'})
        weights = train_sdxl.pose_sample_weights(
            frames, {'kind_boost': 1.25, 'rebalance_share': 0.4}
        )
        self.assertAlmostEqual(weights[4], weights[0] * 4)
        mixed = [
            {'pose': 'standing', 'kind': 'body'},
            {'pose': 'standing', 'kind': 'hand'},
            {'pose': 'standing', 'kind': 'head'},
        ]
        boosted = train_sdxl.pose_sample_weights(
            mixed, {'kind_boost': 1.25, 'rebalance_share': 0.4}
        )
        self.assertAlmostEqual(boosted[1], boosted[0] * 1.25)
        self.assertAlmostEqual(boosted[2], boosted[0] * 1.25)

    def test_cap_limits_a_dominant_cluster(self) -> None:
        weights = [1.0] * 10
        clusters = ['standing'] * 8 + ['sitting', 'lying']
        capped = train_sdxl.cap_cluster_weights(weights, clusters, 0.4)
        standing = sum(capped[:8])
        total = sum(capped)
        self.assertLessEqual(standing / total, 0.4 + 1e-6)


class ClassCacheTests(unittest.TestCase):
    def test_class_cache_is_reused_when_warm(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            cache = train_sdxl.class_cache_path(
                models, {'class_cache_dir': '.zone-class/person'}
            )
            cache.mkdir(parents=True)
            for index in range(4):
                (cache / f'{index:04}.png').write_bytes(b'png')
                (cache / f'{index:04}.txt').write_text(
                    train_sdxl.class_prompt_for('a photo of a person', index),
                    encoding='utf-8',
                )

            class Boom:
                def to(self, device):
                    raise AssertionError('warm cache should not move the pipeline')

                def __call__(self, *args, **kwargs):
                    raise AssertionError('warm cache should not generate')

            first = train_sdxl.ensure_class_images(
                Boom(), cache, 4, 'a photo of a person', 'cpu'
            )
            second = train_sdxl.ensure_class_images(
                Boom(), cache, 4, 'a photo of a person', 'cpu'
            )
            self.assertEqual(len(first), 4)
            self.assertEqual(first, second)
            self.assertTrue(all(caption.startswith('a photo of a person, ') for _, caption in first))

    def test_cold_cache_writes_pose_diverse_prompts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)

            class FakeImage:
                def save(self, path):
                    Path(path).write_bytes(b'png')

            class FakePipeline:
                def __init__(self) -> None:
                    self.calls: list[str] = []

                def to(self, device):
                    return self

                def __call__(self, prompt, **kwargs):
                    self.calls.append(prompt)

                    class Result:
                        images = [FakeImage()]

                    return Result()

            pipeline = FakePipeline()
            pairs = train_sdxl.ensure_class_images(
                pipeline, cache, 3, 'a photo of a person', 'cpu'
            )
            self.assertEqual(len(pipeline.calls), 3)
            self.assertEqual(len(pairs), 3)
            self.assertEqual(pipeline.calls[0], 'a photo of a person, standing')
            self.assertEqual(pipeline.calls[1], 'a photo of a person, sitting')
            self.assertTrue((cache / '0000.txt').is_file())
            pipeline.calls.clear()
            again = train_sdxl.ensure_class_images(
                pipeline, cache, 3, 'a photo of a person', 'cpu'
            )
            self.assertEqual(pipeline.calls, [])
            self.assertEqual(len(again), 3)


class DeviceSelectionTests(unittest.TestCase):
    def fake_torch(
        self,
        *,
        cuda: bool,
        mps: bool = False,
        memory: int | None = None,
    ) -> mock.MagicMock:
        module = mock.MagicMock()
        module.cuda.is_available.return_value = cuda
        module.backends.mps.is_available.return_value = mps
        module.device.side_effect = lambda name: self.make_device(name)
        if memory is not None:
            module.cuda.get_device_properties.return_value = mock.Mock(total_memory=memory)
        return module

    def make_device(self, kind: str, index: int | None = 0) -> mock.Mock:
        device = mock.Mock()
        device.type = kind
        device.index = index
        return device

    def test_cuda_preferred_when_the_backend_reports_available(self) -> None:
        module = self.fake_torch(cuda=True, mps=True)
        with mock.patch.dict(sys.modules, {'torch': module}):
            device = train_sdxl.select_device()
        self.assertEqual(device.type, 'cuda')
        module.device.assert_called_with('cuda')

    def test_mps_selected_when_cuda_is_not(self) -> None:
        module = self.fake_torch(cuda=False, mps=True)
        with mock.patch.dict(sys.modules, {'torch': module}):
            device = train_sdxl.select_device()
        self.assertEqual(device.type, 'mps')
        module.device.assert_called_with('mps')

    def test_finetune_on_a_12gb_fake_cuda_device_raises(self) -> None:
        module = self.fake_torch(cuda=True, memory=12 * 1024**3)
        with mock.patch.dict(sys.modules, {'torch': module}):
            with self.assertRaisesRegex(RuntimeError, '48 GB card for fine-tune'):
                train_sdxl.assert_device_capacity(self.make_device('cuda'), 'finetune')

    def test_lora_on_a_24gb_fake_cuda_device_passes(self) -> None:
        module = self.fake_torch(cuda=True, memory=24 * 1024**3)
        with mock.patch.dict(sys.modules, {'torch': module}):
            train_sdxl.assert_device_capacity(self.make_device('cuda'), 'lora')

    def test_tf32_flags_are_cleared_when_device_is_cuda(self) -> None:
        module = self.fake_torch(cuda=True)
        module.backends.cuda.matmul.allow_tf32 = True
        module.backends.cudnn.allow_tf32 = True
        with mock.patch.dict(sys.modules, {'torch': module}):
            train_sdxl.select_device()
        self.assertFalse(module.backends.cuda.matmul.allow_tf32)
        self.assertFalse(module.backends.cudnn.allow_tf32)

    def test_cpu_and_mps_skip_vram_checks(self) -> None:
        train_sdxl.assert_device_capacity(self.make_device('cpu'), 'finetune')
        train_sdxl.assert_device_capacity(self.make_device('mps'), 'finetune')


class OptimizerTests(unittest.TestCase):
    def test_prodigy_path_is_config_gated(self) -> None:
        config = train_sdxl.load_config()
        self.assertEqual(config['optimizer'], 'prodigy')
        self.assertEqual(config['finetune_optimizer'], 'adamw')
        self.assertEqual(train_sdxl.optimizer_kind('lora', config), 'prodigy')
        self.assertEqual(train_sdxl.optimizer_kind('pivotal', config), 'prodigy')
        self.assertEqual(train_sdxl.optimizer_kind('finetune', config), 'adamw')
        overridden = dict(config)
        overridden['optimizer'] = 'adamw'
        self.assertEqual(train_sdxl.optimizer_kind('lora', overridden), 'adamw')
        self.assertEqual(train_sdxl.optimizer_kind('pivotal', overridden), 'adamw')
        self.assertEqual(train_sdxl.optimizer_kind('finetune', overridden), 'adamw')

    def test_missing_prodigy_raises_a_clear_error(self) -> None:
        with mock.patch.object(
            train_sdxl,
            'load_prodigy',
            side_effect=RuntimeError(
                'Prodigy is not installed in the train venv; pip install prodigyopt'
            ),
        ):
            with self.assertRaisesRegex(RuntimeError, 'pip install prodigyopt'):
                train_sdxl.make_optimizer(
                    'lora',
                    [{'params': [object()], 'lr': 1.0}],
                    {'optimizer': 'prodigy'},
                )

    def test_prodigy_shares_one_adaptive_lr_across_groups(self) -> None:
        captured: dict[str, list] = {}

        class FakeProdigy:
            def __init__(self, groups: list[dict]) -> None:
                captured['groups'] = groups

        output = io.StringIO()
        with mock.patch.object(train_sdxl, 'load_prodigy', return_value=FakeProdigy):
            with contextlib.redirect_stdout(output):
                optimizer = train_sdxl.make_optimizer(
                    'lora',
                    [
                        {'params': [object()], 'lr': 0.0001},
                        {'params': [object()], 'lr': 0.00001},
                    ],
                    {'optimizer': 'prodigy'},
                )
        self.assertIsInstance(optimizer, FakeProdigy)
        self.assertEqual([group['lr'] for group in captured['groups']], [1.0, 1.0])
        self.assertIn('prodigy ignores lr=0.0001, lr=1e-05', output.getvalue())

    def test_prodigy_says_it_ignores_the_configured_rates(self) -> None:
        class FakeProdigy:
            def __init__(self, groups: list[dict]) -> None:
                self.groups = groups

        output = io.StringIO()
        with mock.patch.object(train_sdxl, 'load_prodigy', return_value=FakeProdigy):
            with contextlib.redirect_stdout(output):
                train_sdxl.make_optimizer(
                    'lora',
                    [
                        {'params': [object()], 'lr': 0.0001},
                        {'params': [object()], 'lr': 0.00001},
                    ],
                    {'optimizer': 'prodigy', 'unet_lr': 0.0001, 'text_encoder_lr': 0.00001},
                )
        self.assertEqual(
            output.getvalue().strip(),
            'prodigy ignores unet_lr=0.0001, text_encoder_lr=1e-05: every parameter '
            'group steps at one adaptive rate (lr=1.0), so the text encoder moves as '
            'fast as the unet',
        )

    def test_prodigy_stays_quiet_when_every_group_already_uses_its_rate(self) -> None:
        class FakeProdigy:
            def __init__(self, groups: list[dict]) -> None:
                self.groups = groups

        output = io.StringIO()
        with mock.patch.object(train_sdxl, 'load_prodigy', return_value=FakeProdigy):
            with contextlib.redirect_stdout(output):
                train_sdxl.make_optimizer(
                    'lora',
                    [{'params': [object()], 'lr': train_sdxl.PRODIGY_LR}],
                    {'optimizer': 'prodigy'},
                )
        self.assertEqual(output.getvalue(), '')

    def test_adamw_does_not_claim_its_rates_are_ignored(self) -> None:
        class FakeAdamW:
            def __init__(self, groups: list[dict]) -> None:
                self.groups = groups

        fake_torch = mock.MagicMock()
        fake_torch.optim.AdamW = FakeAdamW
        output = io.StringIO()
        with mock.patch.dict(sys.modules, {'torch': fake_torch}):
            with contextlib.redirect_stdout(output):
                train_sdxl.make_optimizer(
                    'lora',
                    [{'params': [object()], 'lr': 0.0001}],
                    {'optimizer': 'adamw', 'unet_lr': 0.0001},
                )
        self.assertEqual(output.getvalue(), '')

    def test_adamw_keeps_configured_group_rates(self) -> None:
        captured: dict[str, list] = {}

        class FakeAdamW:
            def __init__(self, groups: list[dict]) -> None:
                captured['groups'] = groups

        fake_torch = mock.MagicMock()
        fake_torch.optim.AdamW = FakeAdamW
        with mock.patch.dict(sys.modules, {'torch': fake_torch}):
            optimizer = train_sdxl.make_optimizer(
                'lora',
                [
                    {'params': [object()], 'lr': 0.0001},
                    {'params': [object()], 'lr': 0.00001},
                ],
                {'optimizer': 'adamw'},
            )
        self.assertIsInstance(optimizer, FakeAdamW)
        self.assertEqual(
            [group['lr'] for group in captured['groups']],
            [0.0001, 0.00001],
        )


class PreviewTests(unittest.TestCase):
    def test_packaged_preview_steps_match_sdxl_people_sampler(self) -> None:
        config = train_sdxl.load_config()
        self.assertEqual(config['preview_steps'], 25)

    def test_render_previews_use_configured_sampler_steps(self) -> None:
        captured: list[dict] = []

        class FakeImage:
            def save(self, path: Path) -> None:
                Path(path).write_bytes(b'\x89PNG\r\n\x1a\n')

        class FakeResult:
            images = [FakeImage()]

        class FakeModule:
            def __init__(self) -> None:
                self.training = True

            def eval(self) -> None:
                self.training = False

            def train(self) -> None:
                self.training = True

        class FakePipeline:
            def __init__(self) -> None:
                self.unet = FakeModule()
                self.text_encoder = FakeModule()
                self.vae = mock.Mock()

            def __call__(self, prompt: str, **kwargs: object) -> FakeResult:
                captured.append({'prompt': prompt, **kwargs})
                return FakeResult()

        fake_torch = mock.MagicMock()
        fake_torch.inference_mode.return_value = mock.MagicMock()
        with tempfile.TemporaryDirectory() as directory:
            job_dir = Path(directory)
            with mock.patch.dict(sys.modules, {'torch': fake_torch}):
                paths = train_sdxl.render_previews(
                    FakePipeline(),
                    job_dir,
                    250,
                    'emily',
                    'cpu',
                    {'preview_count': 4, 'preview_steps': 25, 'resolution': 1024},
                )
        self.assertEqual(len(captured), 4)
        self.assertTrue(all(call['num_inference_steps'] == 25 for call in captured))
        self.assertTrue(all(call['guidance_scale'] == 5.0 for call in captured))
        self.assertTrue(captured[0]['prompt'].startswith('identity portrait of emily person'))
        self.assertEqual(
            paths,
            [
                'previews/step-250-0.png',
                'previews/step-250-1.png',
                'previews/step-250-2.png',
                'previews/step-250-3.png',
            ],
        )

    def test_render_previews_default_to_twenty_five_steps(self) -> None:
        captured: list[dict] = []

        class FakeImage:
            def save(self, path: Path) -> None:
                Path(path).write_bytes(b'\x89PNG\r\n\x1a\n')

        class FakeResult:
            images = [FakeImage()]

        class FakeModule:
            training = True

            def eval(self) -> None:
                pass

            def train(self) -> None:
                pass

        class FakePipeline:
            def __init__(self) -> None:
                self.unet = FakeModule()
                self.text_encoder = FakeModule()
                self.vae = mock.Mock()

            def __call__(self, prompt: str, **kwargs: object) -> FakeResult:
                captured.append(kwargs)
                return FakeResult()

        fake_torch = mock.MagicMock()
        fake_torch.inference_mode.return_value = mock.MagicMock()
        with tempfile.TemporaryDirectory() as directory:
            with mock.patch.dict(sys.modules, {'torch': fake_torch}):
                train_sdxl.render_previews(
                    FakePipeline(),
                    Path(directory),
                    500,
                    'emily',
                    'cpu',
                    {'preview_count': 1},
                )
        self.assertEqual(captured[0]['num_inference_steps'], 25)

    def test_placeholder_previews_are_written_for_a_fake_snapshot(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            job_dir = Path(directory)
            paths = train_sdxl.write_preview_placeholders(job_dir, 250)
            self.assertEqual(
                paths,
                [
                    'previews/step-250-0.png',
                    'previews/step-250-1.png',
                    'previews/step-250-2.png',
                    'previews/step-250-3.png',
                ],
            )
            for relative in paths:
                payload = (job_dir / relative).read_bytes()
                self.assertTrue(payload.startswith(b'\x89PNG'))
            progress = train_sdxl.Progress(job_dir, 'lora', 500)
            progress.emit('training', step=250, phase_step=250, phase_total=500, previews=paths)
            sidecar = json.loads((job_dir / 'progress.json').read_text(encoding='utf-8'))
            self.assertEqual(sidecar['previews'], paths)
            self.assertEqual(sidecar['phase'], 'training')


class DatasetListingTests(unittest.TestCase):
    def test_mask_pngs_are_not_counted_as_frames(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            job_dir = Path(directory)
            write_dataset(job_dir, count=2)
            dataset = job_dir / 'dataset'
            (dataset / '0000.mask.png').write_bytes(b'mask')
            (dataset / '0000.kind').write_text('head', encoding='utf-8')
            (dataset / '0000.pose').write_text('standing', encoding='utf-8')
            self.assertEqual(train_sdxl.count_images(dataset), 2)
            frames = train_sdxl.list_frames(dataset)
            self.assertEqual(len(frames), 2)
            self.assertEqual(frames[0]['kind'], 'head')
            self.assertEqual(frames[0]['pose'], 'standing')
            self.assertEqual(frames[0]['mask'], dataset / '0000.mask.png')
            self.assertIsNone(frames[1]['mask'])
            job = train_sdxl.normalize_job(
                queued_job(extra={'image_count': 99}), job_dir
            )
            self.assertEqual(job['image_count'], 2)


class StubOnceTests(unittest.TestCase):
    def run_worker(self, models: Path, extra: list[str] | None = None, env: dict | None = None) -> subprocess.CompletedProcess[str]:
        command = [
            sys.executable,
            str(SCRIPT),
            '--models-dir',
            str(models),
            '--once',
            *(extra or []),
        ]
        return subprocess.run(command, capture_output=True, text=True, env=env)

    def stage(self, models: Path, method: str = 'lora') -> Path:
        job_dir = models / '.zone-train' / str(uuid.uuid4())
        job_dir.mkdir(parents=True)
        train_sdxl.write_job(job_dir, queued_job(method=method))
        write_dataset(job_dir, count=2)
        (models / 'loras').mkdir(exist_ok=True)
        (models / 'checkpoints').mkdir(exist_ok=True)
        return job_dir

    def assert_left_in_job_directory(self, models: Path, job_dir: Path) -> None:
        job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
        self.assertEqual(job['status'], 'succeeded')
        self.assertTrue(
            (job_dir / job['filename']).is_file(),
            'abnegate-comfy publishes the weight it finds in the job directory',
        )
        for folder in ('loras', 'checkpoints'):
            self.assertEqual(
                sorted(path.name for path in (models / folder).iterdir()),
                [],
                f'the trainer published into {folder}/ itself',
            )

    def test_stub_once_leaves_every_still_weight_for_abnegate_comfy_to_publish(self) -> None:
        for method in ('lora', 'pivotal', 'finetune'):
            with self.subTest(method=method), tempfile.TemporaryDirectory() as directory:
                models = Path(directory)
                job_dir = self.stage(models, method=method)
                result = self.run_worker(models, extra=['--stub'])
                self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
                self.assert_left_in_job_directory(models, job_dir)

    def test_stub_once_marks_running_then_succeeds(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = self.stage(models)
            result = self.run_worker(models, extra=['--stub'])
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'succeeded')
            self.assertEqual(job['filename'], 'jerry.safetensors')
            self.assertIsNone(job['error'])
            progress = json.loads((job_dir / 'progress.json').read_text(encoding='utf-8'))
            self.assertEqual(progress['step'], progress['total'])
            self.assertEqual(progress['total'], 500)
            self.assertEqual(progress['phase'], 'publishing')
            self.assertEqual(progress['percent'], 100)
            self.assertTrue((job_dir / 'jerry.safetensors').is_file())

    def test_zone_train_stub_env_processes_one_queued_job(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = self.stage(models, method='finetune')
            env = os.environ.copy()
            env['ZONE_TRAIN_STUB'] = '1'
            result = self.run_worker(models, env=env)
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assert_left_in_job_directory(models, job_dir)

    def test_stub_once_pivotal_writes_lora_embedding_and_previews(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = self.stage(models, method='pivotal')
            result = self.run_worker(models, extra=['--stub'])
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'succeeded')
            self.assertEqual(job['recipe_id'], 'sdxl-adapter')
            self.assertTrue((job_dir / 'jerry.safetensors').is_file())
            self.assertTrue((models / 'embeddings' / 'jerry.safetensors').is_file())
            progress = json.loads((job_dir / 'progress.json').read_text(encoding='utf-8'))
            self.assertEqual(len(progress['previews']), 4)
            for relative in progress['previews']:
                self.assertTrue((job_dir / relative).is_file())
                self.assertTrue((job_dir / relative).read_bytes().startswith(b'\x89PNG'))

    def test_stub_writes_preview_placeholders(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = self.stage(models)
            result = self.run_worker(models, extra=['--stub'])
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            progress = json.loads((job_dir / 'progress.json').read_text(encoding='utf-8'))
            self.assertEqual(
                progress['previews'],
                [
                    'previews/step-500-0.png',
                    'previews/step-500-1.png',
                    'previews/step-500-2.png',
                    'previews/step-500-3.png',
                ],
            )
            for relative in progress['previews']:
                self.assertTrue((job_dir / relative).is_file())

    def test_once_with_no_job_exits_zero(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_worker(Path(directory), extra=['--stub'])
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)


class LegacyPublishTests(unittest.TestCase):
    def run_stub(self, models: Path, method: str) -> Path:
        job_dir = models / '.zone-train' / str(uuid.uuid4())
        job_dir.mkdir(parents=True)
        train_sdxl.write_job(job_dir, legacy_job(method=method))
        write_dataset(job_dir, count=2)
        train_sdxl.process_job(models, job_dir, train_sdxl.load_config(), stub=True)
        self.assertEqual(train_sdxl.load_job(job_dir)['status'], 'succeeded')
        self.assertFalse(
            (job_dir / 'jerry.safetensors').exists(),
            'a job queued before the upgrade has no base, so nothing publishes it from the job directory',
        )
        return job_dir

    def sidecar(self, weight: Path) -> dict:
        self.assertTrue(weight.is_file(), f'{weight} was not published')
        return json.loads(
            weight.with_name(weight.name + '.zone.json').read_text(encoding='utf-8')
        )

    def test_a_lora_queued_before_the_upgrade_publishes_with_its_sidecar(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            self.run_stub(models, 'lora')
            sidecar = self.sidecar(models / 'loras' / 'jerry.safetensors')
            sidecar.pop('generation')
            self.assertEqual(
                sidecar,
                {
                    'recipe_id': 'sdxl-adapter',
                    'hf_base': train_sdxl.DEFAULT_HF_BASE,
                    'trigger': 'ohwx',
                },
            )

    def test_a_lora_queued_before_the_upgrade_can_train_again_without_a_restart(self) -> None:
        for method, folder in (
            ('lora', 'loras'),
            ('pivotal', 'loras'),
            ('finetune', 'checkpoints'),
        ):
            with self.subTest(method=method), tempfile.TemporaryDirectory() as directory:
                models = Path(directory)
                self.run_stub(models, method)
                sidecar = self.sidecar(models / folder / 'jerry.safetensors')
                self.assertIn(
                    'generation',
                    sidecar,
                    'abnegate-comfy refuses to replace a weight whose sidecar has no generation',
                )
                uuid.UUID(sidecar['generation'])

    def test_each_legacy_publish_is_a_new_generation(self) -> None:
        generations = set()
        for _ in range(2):
            with tempfile.TemporaryDirectory() as directory:
                models = Path(directory)
                self.run_stub(models, 'lora')
                generations.add(self.sidecar(models / 'loras' / 'jerry.safetensors')['generation'])
        self.assertEqual(len(generations), 2, generations)

    def test_a_pivotal_lora_queued_before_the_upgrade_keeps_its_embedding(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            self.run_stub(models, 'pivotal')
            sidecar = self.sidecar(models / 'loras' / 'jerry.safetensors')
            self.assertEqual(sidecar['embedding'], 'jerry.safetensors')
            self.assertEqual(sidecar['recipe_id'], 'sdxl-adapter')
            self.assertTrue((models / 'embeddings' / 'jerry.safetensors').is_file())

    def test_a_fine_tune_queued_before_the_upgrade_publishes_among_the_checkpoints(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            self.run_stub(models, 'finetune')
            sidecar = self.sidecar(models / 'checkpoints' / 'jerry.safetensors')
            self.assertEqual(sidecar['recipe_id'], 'sdxl')
            self.assertEqual(sidecar['trigger'], 'ohwx')
            self.assertFalse((models / 'loras' / 'jerry.safetensors').exists())

    def test_a_job_with_a_base_is_left_in_the_job_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            train_sdxl.write_job(job_dir, queued_job())
            write_dataset(job_dir, count=2)
            train_sdxl.process_job(models, job_dir, train_sdxl.load_config(), stub=True)
            self.assertTrue((job_dir / 'jerry.safetensors').is_file())
            self.assertFalse((models / 'loras').exists())


class ClaimTests(unittest.TestCase):
    def stage(self, models: Path, started_at: str = '2026-10-04T00:00:00Z') -> Path:
        job_dir = models / '.zone-train' / str(uuid.uuid4())
        job_dir.mkdir(parents=True)
        train_sdxl.write_job(job_dir, queued_job(extra={'started_at': started_at}))
        write_dataset(job_dir, count=2)
        (models / 'loras').mkdir(exist_ok=True)
        return job_dir

    def test_the_claim_is_taken_before_the_job_is_marked_running(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = self.stage(models)
            claimed_when_running: list[bool] = []
            mark_running = train_sdxl.mark_running

            def spy(path: Path, job: dict) -> dict:
                claimed_when_running.append((Path(path) / CLAIM).is_file())
                return mark_running(path, job)

            with mock.patch.object(train_sdxl, 'mark_running', side_effect=spy):
                train_sdxl.process_job(models, job_dir, train_sdxl.load_config(), stub=True)
            self.assertEqual(claimed_when_running, [True])
            self.assertEqual(train_sdxl.load_job(job_dir)['status'], 'succeeded')

    def test_a_job_claimed_elsewhere_is_left_alone(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = self.stage(models)
            (job_dir / CLAIM).touch()
            before = (job_dir / 'job.json').read_bytes()
            with mock.patch.object(train_sdxl, 'run_stub') as run_stub:
                train_sdxl.process_job(models, job_dir, train_sdxl.load_config(), stub=True)
            run_stub.assert_not_called()
            self.assertEqual((job_dir / 'job.json').read_bytes(), before)
            self.assertFalse((job_dir / 'pid').exists())

    def test_two_trainers_holding_the_same_queued_job_run_it_once(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = self.stage(models)
            stale = train_sdxl.load_job(job_dir)
            config = train_sdxl.load_config()
            with mock.patch.object(train_sdxl, 'run_stub', wraps=train_sdxl.run_stub) as run_stub:
                train_sdxl.process_job(models, job_dir, config, stub=True)
                with mock.patch.object(train_sdxl, 'load_job', return_value=dict(stale)):
                    train_sdxl.process_job(models, job_dir, config, stub=True)
            self.assertEqual(run_stub.call_count, 1)
            self.assertEqual(train_sdxl.load_job(job_dir)['status'], 'succeeded')

    def test_a_queued_job_is_claimed_once(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = self.stage(models)
            job = train_sdxl.load_job(job_dir)
            self.assertTrue(train_sdxl.claim(job_dir, job))
            self.assertFalse(train_sdxl.claim(job_dir, job))
            self.assertTrue((job_dir / CLAIM).is_file())

    def test_a_job_resumed_after_a_crash_runs_under_its_own_claim(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = self.stage(models)
            job = train_sdxl.load_job(job_dir)
            job['status'] = 'running'
            job['pid'] = train_sdxl.unused_pid()
            train_sdxl.write_job(job_dir, job)
            (job_dir / CLAIM).touch()
            self.assertEqual(train_sdxl.find_job(models), job_dir)
            train_sdxl.process_job(models, job_dir, train_sdxl.load_config(), stub=True)
            self.assertEqual(train_sdxl.load_job(job_dir)['status'], 'succeeded')

    def test_a_job_that_is_neither_queued_nor_running_is_not_revived(self) -> None:
        for status in ('failed', 'succeeded'):
            with self.subTest(status=status), tempfile.TemporaryDirectory() as directory:
                models = Path(directory)
                job_dir = self.stage(models)
                job = train_sdxl.load_job(job_dir)
                job['status'] = status
                train_sdxl.write_job(job_dir, job)
                before = (job_dir / 'job.json').read_bytes()
                self.assertFalse(train_sdxl.claim(job_dir, job))
                with mock.patch.object(train_sdxl, 'run_stub') as run_stub:
                    train_sdxl.process_job(models, job_dir, train_sdxl.load_config(), stub=True)
                run_stub.assert_not_called()
                self.assertEqual((job_dir / 'job.json').read_bytes(), before)
                self.assertFalse((job_dir / 'pid').exists())

    def test_find_job_passes_over_a_claimed_queued_job(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            claimed = self.stage(models, started_at='2026-10-04T00:00:00Z')
            unclaimed = self.stage(models, started_at='2026-10-04T00:00:01Z')
            (claimed / CLAIM).touch()
            self.assertEqual(train_sdxl.find_job(models), unclaimed)
            (unclaimed / CLAIM).touch()
            self.assertIsNone(train_sdxl.find_job(models))

    def test_once_trains_the_unclaimed_job_and_leaves_the_claimed_one_queued(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            claimed = self.stage(models, started_at='2026-10-04T00:00:00Z')
            unclaimed = self.stage(models, started_at='2026-10-04T00:00:01Z')
            (claimed / CLAIM).touch()
            result = subprocess.run(
                [sys.executable, str(SCRIPT), '--models-dir', str(models), '--once', '--stub'],
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertEqual(train_sdxl.load_job(claimed)['status'], 'queued')
            self.assertEqual(train_sdxl.load_job(unclaimed)['status'], 'succeeded')
            self.assertTrue((unclaimed / CLAIM).is_file())

class CrashResumeTests(unittest.TestCase):
    def test_latent_sidecar_sits_next_to_the_png(self) -> None:
        self.assertEqual(
            train_sdxl.latent_sidecar(Path('/data/0001.png')),
            Path('/data/0001.latent.pt'),
        )

    def test_sidecar_is_current_when_sidecar_is_newer(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / '0000.png'
            sidecar = train_sdxl.latent_sidecar(image)
            image.write_bytes(b'png')
            sidecar.write_bytes(b'latent')
            later = image.stat().st_mtime + 5
            os.utime(sidecar, (later, later))
            self.assertTrue(train_sdxl.sidecar_is_current(sidecar, image))
            newer = sidecar.stat().st_mtime + 5
            image.write_bytes(b'png2')
            os.utime(image, (newer, newer))
            self.assertFalse(train_sdxl.sidecar_is_current(sidecar, image))

    def test_load_or_encode_reuses_current_sidecar(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / '0000.png'
            image.write_bytes(b'png')
            sidecar = train_sdxl.latent_sidecar(image)
            sidecar.write_bytes(b'latent')
            later = image.stat().st_mtime + 5
            os.utime(sidecar, (later, later))
            loaded = (object(), 64, 80)
            with mock.patch.object(train_sdxl, 'read_latent_sidecar', return_value=loaded):
                with mock.patch.object(train_sdxl, 'encode_latents') as encode:
                    with mock.patch.object(train_sdxl, 'write_latent_sidecar') as write:
                        result = train_sdxl.load_or_encode_latents('vae', image, 'cpu')
            self.assertEqual(result, loaded)
            encode.assert_not_called()
            write.assert_not_called()

    def test_load_or_encode_writes_sidecar_when_missing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / '0000.png'
            image.write_bytes(b'png')
            encoded = (object(), 32, 48)
            with mock.patch.object(train_sdxl, 'encode_latents', return_value=encoded):
                with mock.patch.object(train_sdxl, 'write_latent_sidecar') as write:
                    result = train_sdxl.load_or_encode_latents('vae', image, 'cpu')
            self.assertEqual(result, encoded)
            write.assert_called_once()

    def test_stale_sidecar_reencodes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / '0000.png'
            sidecar = train_sdxl.latent_sidecar(image)
            sidecar.write_bytes(b'old')
            later = sidecar.stat().st_mtime + 5
            image.write_bytes(b'png')
            os.utime(image, (later, later))
            encoded = (object(), 16, 16)
            with mock.patch.object(train_sdxl, 'encode_latents', return_value=encoded):
                with mock.patch.object(train_sdxl, 'write_latent_sidecar') as write:
                    with mock.patch.object(train_sdxl, 'read_latent_sidecar') as read:
                        result = train_sdxl.load_or_encode_latents('vae', image, 'cpu')
            self.assertEqual(result, encoded)
            read.assert_not_called()
            write.assert_called_once()

    def test_corrupt_sidecar_reencodes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / '0000.png'
            image.write_bytes(b'png')
            sidecar = train_sdxl.latent_sidecar(image)
            sidecar.write_bytes(b'not-a-tensor')
            later = image.stat().st_mtime + 5
            os.utime(sidecar, (later, later))
            encoded = (object(), 8, 8)
            with mock.patch.object(train_sdxl, 'read_latent_sidecar', return_value=None):
                with mock.patch.object(train_sdxl, 'encode_latents', return_value=encoded):
                    with mock.patch.object(train_sdxl, 'write_latent_sidecar') as write:
                        result = train_sdxl.load_or_encode_latents('vae', image, 'cpu')
            self.assertEqual(result, encoded)
            write.assert_called_once()

    def test_resume_prefers_rolling_latest(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / 'step-000250.pt').write_bytes(b'old')
            (path / 'latest.pt').write_bytes(b'new')
            self.assertEqual(train_sdxl.resume_snapshot_path(path), path / 'latest.pt')

    def test_resume_falls_back_to_numbered_snapshot(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / 'step-000100.pt').write_bytes(b'a')
            (path / 'step-000250.pt').write_bytes(b'b')
            self.assertEqual(
                train_sdxl.resume_snapshot_path(path),
                train_sdxl.snapshot_path(path, 250),
            )

    def test_empty_latest_falls_back_to_numbered(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / 'latest.pt').write_bytes(b'')
            (path / 'step-000010.pt').write_bytes(b'x')
            self.assertEqual(
                train_sdxl.resume_snapshot_path(path),
                train_sdxl.snapshot_path(path, 10),
            )

    def test_missing_snapshots_resume_from_the_start(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            self.assertIsNone(train_sdxl.resume_snapshot_path(Path(directory)))

    def test_prune_keeps_rolling_latest(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            latest = path / 'latest.pt'
            latest.write_bytes(b'keep')
            for step in (100, 200, 300, 400):
                train_sdxl.snapshot_path(path, step).write_bytes(b'x')
            train_sdxl.prune_snapshots(path, 2, None)
            self.assertTrue(latest.is_file())
            self.assertEqual(train_sdxl.snapshot_steps(path), [300, 400])

    def test_clear_incomplete_saves_drops_tmp_only(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            keep = path / 'latest.pt'
            tmp = path / 'latest.pt.tmp'
            keep.write_bytes(b'keep')
            tmp.write_bytes(b'tmp')
            train_sdxl.clear_incomplete_saves(path)
            self.assertTrue(keep.is_file())
            self.assertFalse(tmp.is_file())

    def test_oom_runtime_error_is_transient(self) -> None:
        self.assertTrue(train_sdxl.is_transient_crash(MemoryError()))
        self.assertTrue(
            train_sdxl.is_transient_crash(RuntimeError('MPS backend out of memory'))
        )
        self.assertFalse(train_sdxl.is_transient_crash(ValueError('bad checkpoint')))

    def test_memory_error_keeps_job_running(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            write_dataset(job_dir)
            train_sdxl.write_job(job_dir, queued_job(method='finetune'))
            with mock.patch.object(train_sdxl, 'train', side_effect=MemoryError('mps')):
                with self.assertRaises(MemoryError):
                    train_sdxl.process_job(
                        models, job_dir, train_sdxl.load_config(), stub=False
                    )
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'running')

    def test_other_errors_mark_failed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            write_dataset(job_dir)
            train_sdxl.write_job(job_dir, queued_job(method='finetune'))
            with mock.patch.object(train_sdxl, 'train', side_effect=ValueError('bad')):
                with self.assertRaises(ValueError):
                    train_sdxl.process_job(
                        models, job_dir, train_sdxl.load_config(), stub=False
                    )
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'failed')
            self.assertEqual(job['error'], 'bad')

    def test_persist_at_step_one_writes_latest_without_numbered_snapshot(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            job_dir = Path(directory)
            checkpoint_dir = job_dir / 'checkpoints'
            checkpoint_dir.mkdir()
            job = queued_job(method='finetune')
            progress = train_sdxl.Progress(job_dir, 'finetune', 10)
            with mock.patch.object(train_sdxl, 'save_snapshot') as save:
                with mock.patch.object(train_sdxl, 'copy_snapshot') as copy:
                    with mock.patch.object(train_sdxl, 'write_snapshot_previews') as previews:
                        train_sdxl.persist_training_step(
                            checkpoint_dir,
                            job_dir,
                            job,
                            method='finetune',
                            step=1,
                            total=10,
                            loss=0.5,
                            best_loss=0.5,
                            best_step=1,
                            optimizer=object(),
                            unet=object(),
                            text_encoder=object(),
                            token_id=None,
                            sample_rng=random.Random(0),
                            dropout_rng=random.Random(1),
                            every=250,
                            keep=2,
                            pipeline=object(),
                            device='cpu',
                            config={'preview_count': 0},
                            progress=progress,
                        )
            save.assert_called_once()
            self.assertEqual(
                save.call_args.args[0],
                train_sdxl.latest_snapshot_file(checkpoint_dir),
            )
            copy.assert_not_called()
            previews.assert_not_called()
            self.assertFalse(train_sdxl.snapshot_steps(checkpoint_dir))

    def test_persist_writes_latest_every_step_and_numbers_on_interval(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            job_dir = Path(directory)
            checkpoint_dir = job_dir / 'checkpoints'
            checkpoint_dir.mkdir()
            job = queued_job(method='finetune')
            progress = train_sdxl.Progress(job_dir, 'finetune', 10)
            with mock.patch.object(train_sdxl, 'save_snapshot') as save:
                with mock.patch.object(train_sdxl, 'copy_snapshot') as copy:
                    with mock.patch.object(
                        train_sdxl, 'write_snapshot_previews'
                    ) as previews:
                        train_sdxl.persist_training_step(
                            checkpoint_dir,
                            job_dir,
                            job,
                            method='finetune',
                            step=1,
                            total=10,
                            loss=0.5,
                            best_loss=0.5,
                            best_step=1,
                            optimizer=object(),
                            unet=object(),
                            text_encoder=object(),
                            token_id=None,
                            sample_rng=random.Random(0),
                            dropout_rng=random.Random(1),
                            every=250,
                            keep=2,
                            pipeline=object(),
                            device='cpu',
                            config={'preview_count': 0},
                            progress=progress,
                        )
                        copy.assert_not_called()
                        previews.assert_not_called()
                        train_sdxl.persist_training_step(
                            checkpoint_dir,
                            job_dir,
                            job,
                            method='finetune',
                            step=250,
                            total=8000,
                            loss=0.4,
                            best_loss=0.4,
                            best_step=250,
                            optimizer=object(),
                            unet=object(),
                            text_encoder=object(),
                            token_id=None,
                            sample_rng=random.Random(0),
                            dropout_rng=random.Random(1),
                            every=250,
                            keep=2,
                            pipeline=object(),
                            device='cpu',
                            config={'preview_count': 0},
                            progress=progress,
                        )
            self.assertEqual(save.call_count, 2)
            self.assertEqual(
                save.call_args_list[0].args[0],
                train_sdxl.latest_snapshot_file(checkpoint_dir),
            )
            copy.assert_called_once()
            previews.assert_called_once()
            written = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(written['step'], 250)

    def persist_kwargs(self, job_dir: Path, checkpoint_dir: Path, **overrides: object) -> dict:
        job = queued_job(method=str(overrides.get('method') or 'finetune'))
        kwargs: dict = {
            'checkpoint_dir': checkpoint_dir,
            'job_dir': job_dir,
            'job': job,
            'method': 'finetune',
            'step': 1,
            'total': 10,
            'loss': 0.5,
            'best_loss': 0.5,
            'best_step': 1,
            'optimizer': object(),
            'unet': object(),
            'text_encoder': object(),
            'token_id': None,
            'sample_rng': random.Random(0),
            'dropout_rng': random.Random(1),
            'every': 250,
            'keep': 2,
            'pipeline': object(),
            'device': 'cpu',
            'config': {'preview_count': 0},
            'progress': train_sdxl.Progress(job_dir, 'finetune', 10),
        }
        kwargs.update(overrides)
        return kwargs

    def test_cuda_finetune_latest_save_skips_when_in_flight(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            job_dir = Path(directory)
            checkpoint_dir = job_dir / 'checkpoints'
            checkpoint_dir.mkdir()
            device = mock.Mock()
            device.type = 'cuda'
            started = threading.Event()
            release = threading.Event()
            calls = {'count': 0}

            def blocking_save(*_args: object, **_kwargs: object) -> None:
                calls['count'] += 1
                started.set()
                self.assertTrue(release.wait(timeout=2))

            with mock.patch.object(train_sdxl, 'save_snapshot', side_effect=blocking_save):
                with mock.patch.object(train_sdxl, 'copy_snapshot'):
                    with mock.patch.object(train_sdxl, 'write_snapshot_previews'):
                        try:
                            train_sdxl.persist_training_step(
                                **self.persist_kwargs(
                                    job_dir, checkpoint_dir, device=device, step=1
                                )
                            )
                            self.assertTrue(started.wait(timeout=2))
                            train_sdxl.persist_training_step(
                                **self.persist_kwargs(
                                    job_dir, checkpoint_dir, device=device, step=2
                                )
                            )
                            self.assertEqual(calls['count'], 1)
                        finally:
                            release.set()
                            train_sdxl.wait_for_latest_save()
                        self.assertEqual(calls['count'], 1)

    def test_lora_latest_save_stays_synchronous_on_cuda(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            job_dir = Path(directory)
            checkpoint_dir = job_dir / 'checkpoints'
            checkpoint_dir.mkdir()
            device = mock.Mock()
            device.type = 'cuda'
            with mock.patch.object(train_sdxl, 'start_latest_save') as start:
                with mock.patch.object(train_sdxl, 'save_snapshot') as save:
                    with mock.patch.object(train_sdxl, 'copy_snapshot'):
                        with mock.patch.object(train_sdxl, 'write_snapshot_previews'):
                            train_sdxl.persist_training_step(
                                **self.persist_kwargs(
                                    job_dir,
                                    checkpoint_dir,
                                    method='lora',
                                    device=device,
                                    progress=train_sdxl.Progress(job_dir, 'lora', 10),
                                )
                            )
            start.assert_not_called()
            save.assert_called_once()

    def test_python_rng_state_roundtrips_lists(self) -> None:
        rng = random.Random(7)
        state = rng.getstate()
        as_lists = [state[0], list(state[1]), state[2]]
        restored = random.Random()
        restored.setstate(train_sdxl.python_rng_state(as_lists))
        self.assertEqual(rng.random(), restored.random())

    def test_copy_snapshot_replaces_atomically(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            source = path / 'latest.pt'
            destination = path / 'step-000001.pt'
            source.write_bytes(b'snapshot')
            train_sdxl.copy_snapshot(source, destination)
            self.assertEqual(destination.read_bytes(), b'snapshot')
            self.assertFalse((path / 'step-000001.pt.tmp').exists())


class RunpodDispatchTests(unittest.TestCase):
    def _dispatch(self, extra: dict, stub: bool = True):
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            write_dataset(job_dir)
            train_sdxl.write_job(job_dir, queued_job(extra=extra))
            config = train_sdxl.load_config()
            with (
                mock.patch('train_runpod.process_job') as remote,
                mock.patch('train_llm.process_job') as language,
                mock.patch('train_flux.process_job') as flux,
                mock.patch.object(train_sdxl, 'train') as train,
                mock.patch.object(train_sdxl, 'run_stub') as run_stub,
                mock.patch.object(train_sdxl, 'normalize_job') as normalize,
            ):
                train_sdxl.process_job(models, job_dir, config, stub=stub)
            return remote, language, flux, train, run_stub, normalize, models, job_dir, config

    def test_process_job_dispatches_runpod_provider(self) -> None:
        remote, language, flux, train, run_stub, normalize, models, job_dir, config = self._dispatch(
            {'provider': 'runpod'}
        )
        remote.assert_called_once_with(models, job_dir, config, True)
        language.assert_not_called()
        flux.assert_not_called()
        train.assert_not_called()
        run_stub.assert_not_called()
        normalize.assert_not_called()

    def test_language_runpod_dispatches_to_runpod_not_llm(self) -> None:
        remote, language, flux, train, run_stub, normalize, models, job_dir, config = self._dispatch(
            {'provider': 'runpod', 'subject': 'language', 'checkpoint': 'qwen2.5:7b'}
        )
        remote.assert_called_once_with(models, job_dir, config, True)
        language.assert_not_called()
        flux.assert_not_called()
        train.assert_not_called()
        run_stub.assert_not_called()
        normalize.assert_not_called()

    def test_other_runpod_dispatches_to_runpod(self) -> None:
        remote, language, flux, train, run_stub, normalize, models, job_dir, config = self._dispatch(
            {
                'provider': 'runpod',
                'subject': 'other',
                'checkpoint': 'flux1-dev-fp8.safetensors',
            }
        )
        remote.assert_called_once_with(models, job_dir, config, True)
        language.assert_not_called()
        flux.assert_not_called()
        train.assert_not_called()
        run_stub.assert_not_called()
        normalize.assert_not_called()

    def test_language_local_dispatches_to_train_llm(self) -> None:
        remote, language, flux, train, run_stub, normalize, models, job_dir, config = self._dispatch(
            {'subject': 'language', 'checkpoint': 'qwen2.5:7b'}
        )
        language.assert_called_once_with(models, job_dir, True)
        remote.assert_not_called()
        flux.assert_not_called()
        train.assert_not_called()
        run_stub.assert_not_called()
        normalize.assert_not_called()


def import_in_clean_process(
    module: str,
    preset: dict[str, str] | None = None,
    preload_torch: bool = False,
) -> subprocess.CompletedProcess[str]:
    environment = {
        key: value
        for key, value in os.environ.items()
        if key not in train_sdxl.METAL_WATERMARK_VARIABLES
    }
    environment.update(preset or {})
    preload = "import sys, types; sys.modules['torch'] = types.ModuleType('torch'); "
    variables = train_sdxl.METAL_WATERMARK_VARIABLES
    code = (
        f"{preload if preload_torch else ''}import json, os, {module}; "
        f'print(json.dumps({{key: os.environ.get(key) for key in {variables!r}}}))'
    )
    return subprocess.run(
        [sys.executable, '-c', code],
        cwd=COMFYUI,
        env=environment,
        capture_output=True,
        text=True,
        check=True,
    )


class MetalWatermarkTests(unittest.TestCase):
    def test_import_caps_metal_at_the_recommended_working_set(self) -> None:
        result = import_in_clean_process('train_sdxl')

        self.assertEqual(
            json.loads(result.stdout),
            {
                train_sdxl.METAL_HIGH_WATERMARK_VARIABLE: train_sdxl.METAL_HIGH_WATERMARK,
                train_sdxl.METAL_LOW_WATERMARK_VARIABLE: train_sdxl.METAL_LOW_WATERMARK,
            },
        )
        self.assertNotIn(train_sdxl.METAL_WATERMARK_TOO_LATE, result.stderr)

    def test_every_trainer_that_imports_train_sdxl_inherits_the_cap(self) -> None:
        for module in ('train_flux', 'train_wan', 'train_runpod'):
            with self.subTest(module=module):
                result = import_in_clean_process(module)
                self.assertEqual(
                    json.loads(result.stdout)[train_sdxl.METAL_HIGH_WATERMARK_VARIABLE],
                    train_sdxl.METAL_HIGH_WATERMARK,
                )

    def test_import_keeps_preset_values(self) -> None:
        preset = {
            train_sdxl.METAL_HIGH_WATERMARK_VARIABLE: '0.9',
            train_sdxl.METAL_LOW_WATERMARK_VARIABLE: '0.5',
        }

        result = import_in_clean_process('train_sdxl', preset, preload_torch=True)

        self.assertEqual(json.loads(result.stdout), preset)
        self.assertNotIn(train_sdxl.METAL_WATERMARK_TOO_LATE, result.stderr)

    def test_import_after_torch_warns(self) -> None:
        result = import_in_clean_process('train_sdxl', preload_torch=True)

        self.assertIn(train_sdxl.METAL_WATERMARK_TOO_LATE, result.stderr)

    def test_low_is_capped_to_high(self) -> None:
        environment = {
            train_sdxl.METAL_HIGH_WATERMARK_VARIABLE: '0.5',
            train_sdxl.METAL_LOW_WATERMARK_VARIABLE: '0.9',
        }

        train_sdxl.apply_metal_watermarks(environment)

        self.assertEqual(
            environment[train_sdxl.METAL_LOW_WATERMARK_VARIABLE],
            train_sdxl.METAL_LOW_WATERMARK_FLOOR,
        )

    def test_low_defaults_below_high(self) -> None:
        environment = {train_sdxl.METAL_HIGH_WATERMARK_VARIABLE: '1.2'}

        train_sdxl.apply_metal_watermarks(environment)

        self.assertEqual(
            environment[train_sdxl.METAL_LOW_WATERMARK_VARIABLE],
            train_sdxl.METAL_LOW_WATERMARK,
        )

    def test_disabled_high_leaves_low_alone(self) -> None:
        environment = {train_sdxl.METAL_HIGH_WATERMARK_VARIABLE: '0.0'}

        train_sdxl.apply_metal_watermarks(environment)

        self.assertNotIn(train_sdxl.METAL_LOW_WATERMARK_VARIABLE, environment)


if __name__ == '__main__':
    unittest.main()
