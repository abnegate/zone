from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
import uuid
from pathlib import Path

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
        'checkpoint': 'RealVisXL_V5.0_fp16.safetensors',
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
                    'checkpoint': 'RealVisXL_V5.0_fp16.safetensors',
                    'status': 'queued',
                },
                job_dir,
            )
            self.assertEqual(job['filename'], 'jerry.safetensors')
            self.assertEqual(job['recipe_id'], 'sdxl-adapter')
            self.assertEqual(job['hf_base'], 'SG161222/RealVisXL_V5.0')
            self.assertEqual(job['image_count'], 3)
            finetune = train_sdxl.normalize_job(
                {
                    'name': 'jerry',
                    'method': 'finetune',
                    'trigger': 'ohwx',
                    'checkpoint': 'RealVisXL_V5.0_fp16.safetensors',
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
            self.assertEqual(sidecar['hf_base'], 'SG161222/RealVisXL_V5.0')
            self.assertFalse(sidecar.get('generation'))


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

    def test_once_with_no_job_exits_zero(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_worker(Path(directory), extra=['--stub'])
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)


if __name__ == '__main__':
    unittest.main()
