from __future__ import annotations

import json
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
import train_wan  # noqa: E402

CLAIM = 'claimed'


def queued_job(name: str = 'jerry', trigger: str = 'ohwx', extra: dict | None = None) -> dict:
    job = {
        'schema_version': 1,
        'id': str(uuid.uuid4()),
        'name': name,
        'method': 'video',
        'trigger': trigger,
        'checkpoint': 'lustifySDXLNSFW_ggwpV7.safetensors',
        'status': 'queued',
        'error': None,
        'step': None,
        'total': None,
        'pid': None,
        'started_at': '2026-10-04T00:00:00Z',
        'hf_base': 'Comfy-Org/Wan_2.2_ComfyUI_Repackaged',
        'base': 'wan-people',
    }
    if extra:
        job.update(extra)
    return job


def write_clips(job_dir: Path, count: int = 2) -> None:
    clips = job_dir / 'clips'
    clips.mkdir(parents=True)
    for index in range(count):
        (clips / f'{index:04}.mp4').write_bytes(b'mp4')
        (clips / f'{index:04}.json').write_text(
            json.dumps({'start_s': index * 2.0, 'end_s': index * 2.0 + 2.0, 'pose': f'pose-{index}'}),
            encoding='utf-8',
        )


class StepsForTests(unittest.TestCase):
    def setUp(self) -> None:
        self.config = train_wan.load_config()

    def test_budget_is_windows_times_twenty_clamped_150_2000(self) -> None:
        self.assertEqual(self.config['rank'], 16)
        self.assertEqual(self.config['passes_per_clip'], 20)
        self.assertEqual(self.config['width'], 832)
        self.assertEqual(self.config['height'], 480)
        self.assertEqual(self.config['frames'], 49)
        self.assertEqual(self.config['fps'], 24)
        self.assertEqual(train_wan.steps_for(1, self.config), 150)
        self.assertEqual(train_wan.steps_for(7, self.config), 150)
        self.assertEqual(train_wan.steps_for(8, self.config), 160)
        self.assertEqual(train_wan.steps_for(100, self.config), 2000)


class FilenameTests(unittest.TestCase):
    def test_default_filename_adds_wan_suffix(self) -> None:
        self.assertEqual(train_wan.default_filename('jerry'), 'jerry-wan.safetensors')
        self.assertEqual(train_wan.default_filename('jerry-wan'), 'jerry-wan.safetensors')
        self.assertEqual(
            train_wan.default_filename('jerry-wan.safetensors'), 'jerry-wan.safetensors'
        )


class StubTrainingTests(unittest.TestCase):
    def test_process_job_stub_writes_progress_and_adapter(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            write_clips(job_dir, count=3)
            (models / 'loras').mkdir()
            train_sdxl.write_job(job_dir, queued_job())
            train_wan.process_job(models, job_dir, stub=True)
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'succeeded')
            self.assertEqual(job['filename'], 'jerry-wan.safetensors')
            progress = json.loads((job_dir / 'progress.json').read_text(encoding='utf-8'))
            self.assertEqual(progress['phase'], 'publishing')
            self.assertEqual(progress['percent'], 100)
            self.assertIn(progress['phase'], {'loading', 'encoding', 'training', 'publishing'})
            self.assertNotIn('recipe_id', job)
            weight = job_dir / 'jerry-wan.safetensors'
            self.assertTrue(weight.is_file())
            self.assertGreater(weight.stat().st_size, 0)
            self.assertEqual(list((models / 'loras').iterdir()), [])

    def test_sdxl_worker_dispatches_video_jobs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            write_clips(job_dir, count=1)
            (models / 'loras').mkdir()
            train_sdxl.write_job(job_dir, queued_job())
            train_sdxl.process_job(models, job_dir, train_sdxl.load_config(), stub=True)
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'succeeded')
            self.assertTrue((job_dir / 'jerry-wan.safetensors').is_file())


class LegacyJobTests(unittest.TestCase):
    def test_a_video_lora_queued_before_the_upgrade_fails_without_training_or_publishing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            write_clips(job_dir, count=2)
            (models / 'loras').mkdir()
            job = queued_job(extra={'filename': 'jerry-wan.safetensors'})
            del job['base']
            train_sdxl.write_job(job_dir, job)
            with mock.patch.object(train_wan, 'run_stub') as run_stub:
                with self.assertRaises(ValueError):
                    train_sdxl.process_job(models, job_dir, train_sdxl.load_config(), stub=True)
            run_stub.assert_not_called()
            failed = train_sdxl.load_job(job_dir)
            self.assertEqual(failed['status'], 'failed')
            self.assertEqual(failed['error'], train_wan.VIDEO_REFUSAL)
            self.assertEqual(list((models / 'loras').iterdir()), [])
            self.assertFalse((job_dir / 'jerry-wan.safetensors').exists())


class ClaimTests(unittest.TestCase):
    def stage(self, models: Path) -> Path:
        job_dir = models / '.zone-train' / str(uuid.uuid4())
        job_dir.mkdir(parents=True)
        write_clips(job_dir, count=3)
        (models / 'loras').mkdir(exist_ok=True)
        train_sdxl.write_job(job_dir, queued_job())
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
                train_wan.process_job(models, job_dir, stub=True)
            self.assertEqual(claimed_when_running, [True])
            self.assertEqual(train_sdxl.load_job(job_dir)['status'], 'succeeded')

    def test_a_job_claimed_elsewhere_is_left_alone(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = self.stage(models)
            (job_dir / CLAIM).touch()
            before = (job_dir / 'job.json').read_bytes()
            with mock.patch.object(train_wan, 'run_stub') as run_stub:
                train_wan.process_job(models, job_dir, stub=True)
            run_stub.assert_not_called()
            self.assertEqual((job_dir / 'job.json').read_bytes(), before)


if __name__ == '__main__':
    unittest.main()
