from __future__ import annotations

import json
import os
import random
import subprocess
import sys
import tempfile
import unittest
import uuid
from pathlib import Path
from unittest import mock

COMFYUI = Path(__file__).parents[1]
if str(COMFYUI) not in sys.path:
    sys.path.insert(0, str(COMFYUI))

import train_sdxl  # noqa: E402

SCRIPT = COMFYUI / 'train_sdxl.py'


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
    }
    if extra:
        job.update(extra)
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

    def test_budget_is_unique_images_times_twenty_clamped_500_8000(self) -> None:
        self.assertEqual(self.config['passes_per_image'], 20)
        self.assertEqual(self.config['min_steps'], 500)
        self.assertEqual(self.config['max_steps'], 8000)
        self.assertEqual(train_sdxl.steps_for(1, self.config), 500)
        self.assertEqual(train_sdxl.steps_for(25, self.config), 500)
        self.assertEqual(train_sdxl.steps_for(26, self.config), 520)
        self.assertEqual(train_sdxl.steps_for(400, self.config), 8000)
        self.assertEqual(train_sdxl.steps_for(500, self.config), 8000)


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

    def test_stub_publish_writes_weights_and_sidecar(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job = queued_job()
            job = train_sdxl.normalize_job(job, models)
            destination = train_sdxl.publish_stub(models, job)
            self.assertTrue(destination.is_file())
            self.assertGreater(destination.stat().st_size, 0)
            sidecar = json.loads(train_sdxl.sidecar_path(destination).read_text(encoding='utf-8'))
            self.assertEqual(sidecar['recipe_id'], 'sdxl-adapter')
            self.assertEqual(sidecar['trigger'], 'ohwx')
            self.assertEqual(
                sidecar['hf_base'],
                'John6666/lustify-sdxl-nsfw-checkpoint-ggwp-v7-sdxl',
            )
            self.assertFalse(sidecar.get('generation'))

    def test_pivotal_stub_publish_writes_embedding_and_sidecar(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job = train_sdxl.normalize_job(queued_job(method='pivotal'), models)
            destination = train_sdxl.publish_stub(models, job)
            self.assertEqual(destination, models / 'loras' / 'jerry.safetensors')
            embedding = models / 'embeddings' / 'jerry.safetensors'
            self.assertTrue(embedding.is_file())
            sidecar = json.loads(train_sdxl.sidecar_path(destination).read_text(encoding='utf-8'))
            self.assertEqual(sidecar['recipe_id'], 'sdxl-adapter')
            self.assertEqual(sidecar['embedding'], 'jerry.safetensors')


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


class PreviewTests(unittest.TestCase):
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

    def test_stub_once_marks_running_then_succeeds_and_writes_sidecar(self) -> None:
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
            weight = models / 'loras' / 'jerry.safetensors'
            self.assertTrue(weight.is_file())
            sidecar = json.loads((models / 'loras' / 'jerry.safetensors.zone.json').read_text())
            self.assertEqual(sidecar['recipe_id'], 'sdxl-adapter')
            self.assertEqual(sidecar['trigger'], 'ohwx')

    def test_zone_train_stub_env_processes_one_queued_job(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = self.stage(models, method='finetune')
            env = os.environ.copy()
            env['ZONE_TRAIN_STUB'] = '1'
            result = self.run_worker(models, env=env)
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'succeeded')
            weight = models / 'checkpoints' / 'jerry.safetensors'
            self.assertTrue(weight.is_file())
            sidecar = json.loads(weight.with_name(weight.name + '.zone.json').read_text())
            self.assertEqual(sidecar['recipe_id'], 'sdxl')
            self.assertEqual(sidecar['trigger'], 'ohwx')

    def test_stub_once_pivotal_writes_lora_embedding_and_previews(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = self.stage(models, method='pivotal')
            result = self.run_worker(models, extra=['--stub'])
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'succeeded')
            self.assertEqual(job['recipe_id'], 'sdxl-adapter')
            weight = models / 'loras' / 'jerry.safetensors'
            embedding = models / 'embeddings' / 'jerry.safetensors'
            self.assertTrue(weight.is_file())
            self.assertTrue(embedding.is_file())
            sidecar = json.loads((models / 'loras' / 'jerry.safetensors.zone.json').read_text())
            self.assertEqual(sidecar['recipe_id'], 'sdxl-adapter')
            self.assertEqual(sidecar['embedding'], 'jerry.safetensors')
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


if __name__ == '__main__':
    unittest.main()
