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

import train_flux  # noqa: E402
import train_runpod  # noqa: E402
import train_sdxl  # noqa: E402


def queued_job(name: str = 'jerry', extra: dict | None = None) -> dict:
    job = {
        'schema_version': 1,
        'id': str(uuid.uuid4()),
        'name': name,
        'method': 'lora',
        'subject': 'other',
        'trigger': 'ohwx',
        'checkpoint': 'flux1-dev-fp8.safetensors',
        'status': 'queued',
        'error': None,
        'step': None,
        'total': None,
        'pid': None,
        'started_at': '2026-10-04T00:00:00Z',
        'recipe_id': 'flux-dev-adapter',
        'hf_base': 'black-forest-labs/FLUX.1-dev',
    }
    if extra:
        job.update(extra)
    return job


def write_dataset(job_dir: Path, count: int = 2) -> None:
    dataset = job_dir / 'dataset'
    dataset.mkdir(parents=True)
    for index in range(count):
        (dataset / f'{index:04}.png').write_bytes(b'png')
        (dataset / f'{index:04}.txt').write_text('ohwx person, studio', encoding='utf-8')


class StepsForTests(unittest.TestCase):
    def setUp(self) -> None:
        self.config = train_flux.load_config()

    def test_budget_is_images_times_nineteen_clamped_150_6000(self) -> None:
        self.assertEqual(self.config['rank'], 32)
        self.assertEqual(self.config['passes_per_image'], 19)
        self.assertEqual(self.config['min_steps'], 150)
        self.assertEqual(self.config['max_steps'], 6000)
        self.assertEqual(self.config['resolution'], 512)
        self.assertEqual(self.config['learning_rate'], 0.0001)
        self.assertTrue(self.config['alpha_equals_rank'])
        self.assertFalse(self.config['train_modulation'])
        self.assertEqual(train_flux.steps_for(1, self.config), 150)
        self.assertEqual(train_flux.steps_for(7, self.config), 150)
        self.assertEqual(train_flux.steps_for(8, self.config), 152)
        self.assertEqual(train_flux.steps_for(1000, self.config), 6000)


class NormalizeTests(unittest.TestCase):
    def test_normalize_requires_other_lora(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            job_dir = Path(directory)
            write_dataset(job_dir, count=2)
            job = train_flux.normalize_job(queued_job(), job_dir)
            self.assertEqual(job['subject'], 'other')
            self.assertEqual(job['method'], 'lora')
            self.assertEqual(job['filename'], 'jerry.safetensors')
            self.assertEqual(job['recipe_id'], 'flux-dev-adapter')
            self.assertEqual(job['checkpoint'], 'flux1-dev-fp8.safetensors')
            self.assertEqual(job['image_count'], 2)
            with self.assertRaises(ValueError):
                train_flux.normalize_job(queued_job(extra={'subject': 'person'}), job_dir)
            with self.assertRaises(ValueError):
                train_flux.normalize_job(queued_job(extra={'method': 'finetune'}), job_dir)

    def test_qwen_recipe_sets_hf_base(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            job_dir = Path(directory)
            write_dataset(job_dir, count=1)
            job = train_flux.normalize_job(
                queued_job(
                    extra={
                        'recipe_id': 'qwen-image-edit-adapter',
                        'hf_base': '',
                        'checkpoint': 'qwen_image_edit_2511_fp8mixed.safetensors',
                    }
                ),
                job_dir,
            )
            self.assertTrue(train_flux.is_qwen(job))
            self.assertEqual(job['hf_base'], 'Qwen/Qwen-Image-Edit-2511')

    def test_checkpoint_repo_is_comfy_org(self) -> None:
        self.assertEqual(
            train_flux.checkpoint_repo('flux1-dev-fp8.safetensors'),
            'Comfy-Org/flux1-dev',
        )
        self.assertEqual(
            train_flux.checkpoint_repo('flux1-schnell-fp8.safetensors'),
            'Comfy-Org/flux1-schnell',
        )


class StubPublishTests(unittest.TestCase):
    def test_process_job_stub_writes_lora_and_sidecar(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            write_dataset(job_dir, count=3)
            (models / 'loras').mkdir()
            train_sdxl.write_job(job_dir, queued_job())
            train_flux.process_job(models, job_dir, stub=True)
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'succeeded')
            self.assertEqual(job['filename'], 'jerry.safetensors')
            self.assertEqual(job['subject'], 'other')
            progress = json.loads((job_dir / 'progress.json').read_text(encoding='utf-8'))
            self.assertEqual(progress['phase'], 'publishing')
            self.assertEqual(progress['percent'], 100)
            weight = models / 'loras' / 'jerry.safetensors'
            self.assertTrue(weight.is_file())
            sidecar = json.loads(weight.with_name(weight.name + '.zone.json').read_text())
            self.assertEqual(sidecar['recipe_id'], 'flux-dev-adapter')
            self.assertEqual(sidecar['architecture'], 'flux')
            self.assertEqual(sidecar['trigger'], 'ohwx')
            self.assertEqual(sidecar['hf_base'], 'black-forest-labs/FLUX.1-dev')


class RemoteScriptTests(unittest.TestCase):
    def test_remote_script_and_pack_include_train_flux(self) -> None:
        self.assertEqual(
            train_runpod.remote_script({'subject': 'other', 'method': 'lora'}),
            'train_flux.py',
        )
        self.assertIn('train_flux.py', train_runpod.SCRIPTS)
        self.assertIn('train_flux_config.json', train_runpod.SCRIPTS)


if __name__ == '__main__':
    unittest.main()
