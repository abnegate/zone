from __future__ import annotations

import json
import sys
import tempfile
import unittest
import uuid
from pathlib import Path

COMFYUI = Path(__file__).parents[1]
if str(COMFYUI) not in sys.path:
    sys.path.insert(0, str(COMFYUI))

import train_sdxl  # noqa: E402
import train_wan  # noqa: E402


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
        'recipe_id': 'wan-adapter',
        'hf_base': 'Comfy-Org/Wan_2.2_ComfyUI_Repackaged',
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


class StubPublishTests(unittest.TestCase):
    def test_stub_publishes_a_tiny_wan_adapter(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            write_clips(job_dir, count=2)
            (models / 'loras').mkdir()
            (models / 'loras' / 'jerry.face.png').write_bytes(b'face')
            job = train_wan.normalize_job(queued_job(), job_dir)
            destination = train_wan.publish_stub(models, job)
            self.assertEqual(destination.name, 'jerry-wan.safetensors')
            self.assertTrue(destination.is_file())
            self.assertGreater(destination.stat().st_size, 0)
            sidecar = json.loads(train_sdxl.sidecar_path(destination).read_text(encoding='utf-8'))
            self.assertEqual(sidecar['recipe_id'], 'wan-adapter')
            self.assertEqual(sidecar['architecture'], 'wan')
            self.assertEqual(sidecar['trigger'], 'ohwx')
            self.assertEqual(sidecar['hf_base'], 'Comfy-Org/Wan_2.2_ComfyUI_Repackaged')
            self.assertEqual(sidecar['face'], 'jerry.face.png')

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
            weight = models / 'loras' / 'jerry-wan.safetensors'
            self.assertTrue(weight.is_file())
            sidecar = json.loads(weight.with_name(weight.name + '.zone.json').read_text())
            self.assertEqual(sidecar['recipe_id'], 'wan-adapter')
            self.assertEqual(sidecar['architecture'], 'wan')

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
            self.assertTrue((models / 'loras' / 'jerry-wan.safetensors').is_file())


if __name__ == '__main__':
    unittest.main()
