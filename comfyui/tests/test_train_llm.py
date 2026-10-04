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

import train_llm  # noqa: E402
import train_sdxl  # noqa: E402


def queued_job(
    name: str = 'jerry',
    method: str = 'lora',
    extra: dict | None = None,
) -> dict:
    job = {
        'schema_version': 1,
        'id': str(uuid.uuid4()),
        'name': name,
        'method': method,
        'subject': 'language',
        'trigger': '',
        'checkpoint': 'llama3.2:1b',
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


def write_texts(job_dir: Path, count: int = 2) -> None:
    dataset = job_dir / 'dataset'
    dataset.mkdir(parents=True)
    for index in range(count):
        (dataset / f'{index:04}.txt').write_text(
            f'example text {index} for language training.\n' * 4,
            encoding='utf-8',
        )


def write_chat(job_dir: Path, count: int = 3) -> None:
    dataset = job_dir / 'dataset'
    dataset.mkdir(parents=True)
    lines = []
    for index in range(count):
        lines.append(
            json.dumps(
                {
                    'messages': [
                        {'role': 'user', 'content': f'hello {index}'},
                        {'role': 'assistant', 'content': f'hi {index}'},
                    ]
                }
            )
        )
    (dataset / 'turns.jsonl').write_text('\n'.join(lines) + '\n', encoding='utf-8')


def write_still_dataset(job_dir: Path, count: int = 1) -> None:
    dataset = job_dir / 'dataset'
    dataset.mkdir(parents=True)
    for index in range(count):
        (dataset / f'{index:04}.png').write_bytes(b'png')
        (dataset / f'{index:04}.txt').write_text('ohwx person, studio', encoding='utf-8')


def still_job(name: str = 'jerry', extra: dict | None = None) -> dict:
    job = {
        'schema_version': 1,
        'id': str(uuid.uuid4()),
        'name': name,
        'method': 'lora',
        'trigger': 'ohwx',
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


class StepsForTests(unittest.TestCase):
    def setUp(self) -> None:
        self.config = train_llm.load_config()

    def test_budget_is_examples_times_one_clamped_200_1000(self) -> None:
        self.assertEqual(self.config['iters_per_example'], 1)
        self.assertEqual(self.config['min_iters'], 200)
        self.assertEqual(self.config['max_iters'], 1000)
        self.assertEqual(self.config['lora_parameters']['rank'], 16)
        self.assertEqual(train_llm.steps_for(1, self.config), 200)
        self.assertEqual(train_llm.steps_for(200, self.config), 200)
        self.assertEqual(train_llm.steps_for(201, self.config), 201)
        self.assertEqual(train_llm.steps_for(1000, self.config), 1000)
        self.assertEqual(train_llm.steps_for(5000, self.config), 1000)
        self.assertEqual(train_llm.steps_for(0, self.config), 200)


class MappingTests(unittest.TestCase):
    def test_ollama_names_resolve_through_mlx_community(self) -> None:
        self.assertEqual(
            train_llm.mlx_repo('llama3.2:1b'),
            'mlx-community/Llama-3.2-1B-Instruct-4bit',
        )
        self.assertEqual(
            train_llm.mlx_repo('mlx-community/Llama-3.2-1B-Instruct-4bit'),
            'mlx-community/Llama-3.2-1B-Instruct-4bit',
        )
        self.assertTrue(train_llm.mlx_repo('qwen2.5:7b').startswith('mlx-community/'))
        self.assertIn('Qwen', train_llm.mlx_repo('qwen2.5:7b'))

    def test_gguf_export_is_llama_mixtral_mistral_only(self) -> None:
        self.assertTrue(train_llm.exports_gguf('llama3.2:1b'))
        self.assertTrue(train_llm.exports_gguf('mistral'))
        self.assertTrue(train_llm.exports_gguf('mixtral:8x7b'))
        self.assertFalse(train_llm.exports_gguf('qwen2.5:7b'))
        self.assertFalse(train_llm.exports_gguf('mlx-community/Qwen2.5-7B-Instruct-4bit'))

    def test_finetune_type_stays_full_under_the_param_cap(self) -> None:
        config = train_llm.load_config()
        self.assertEqual(train_llm.fine_tune_type('lora', 'llama3.2:1b', config), 'lora')
        self.assertEqual(train_llm.fine_tune_type('finetune', 'llama3.2:1b', config), 'full')
        self.assertEqual(train_llm.fine_tune_type('finetune', 'qwen2.5:7b', config), 'full')
        self.assertEqual(train_llm.fine_tune_type('finetune', 'qwen2.5:14b', config), 'lora')


class CommandTests(unittest.TestCase):
    def test_lora_args_use_yaml_rank_and_mask_prompt_only_for_chat(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            data = root / 'data'
            data.mkdir()
            (data / 'train.jsonl').write_text(
                json.dumps(
                    {
                        'messages': [
                            {'role': 'user', 'content': 'hi'},
                            {'role': 'assistant', 'content': 'hello'},
                        ]
                    }
                )
                + '\n',
                encoding='utf-8',
            )
            (data / 'format.json').write_text(
                json.dumps({'mask_prompt': True}), encoding='utf-8'
            )
            args = train_llm.lora_args(
                model='mlx-community/Llama-3.2-1B-Instruct-4bit',
                data=data,
                iters=200,
                batch_size=1,
                fine_tune='lora',
                adapter_path=root / 'adapters',
                num_layers=16,
                learning_rate=1e-5,
                seed=0,
                save_every=100,
                config_path=root / 'lora_config.yaml',
                mask_prompt=train_llm.should_mask_prompt(data),
            )
            self.assertNotIn('--rank', args)
            self.assertIn('--mask-prompt', args)
            self.assertIn('--fine-tune-type', args)
            self.assertEqual(args[args.index('--fine-tune-type') + 1], 'lora')
            command = train_llm.mlx_command('lora', args)
            self.assertEqual(command[0], str(train_llm.llm_python()))
            self.assertEqual(command[1:4], ['-m', 'mlx_lm', 'lora'])
            self.assertTrue(str(train_llm.llm_python()).endswith('.venv-train-llm/bin/python'))

            text_data = root / 'text'
            text_data.mkdir()
            (text_data / 'train.jsonl').write_text(
                json.dumps({'text': 'plain document'}) + '\n', encoding='utf-8'
            )
            (text_data / 'format.json').write_text(
                json.dumps({'text': True}), encoding='utf-8'
            )
            text_args = train_llm.lora_args(
                model='mlx-community/Llama-3.2-1B-Instruct-4bit',
                data=text_data,
                iters=200,
                batch_size=1,
                fine_tune='lora',
                adapter_path=root / 'adapters',
                num_layers=16,
                learning_rate=1e-5,
                seed=0,
                save_every=100,
                config_path=root / 'lora_config.yaml',
                mask_prompt=train_llm.should_mask_prompt(text_data),
            )
            self.assertNotIn('--mask-prompt', text_args)

    def test_fuse_skips_export_gguf_for_qwen(self) -> None:
        llama = train_llm.fuse_args(
            model=train_llm.mlx_repo('llama3.2:1b'),
            adapter_path=Path('/tmp/adapters'),
            save_path=Path('/tmp/fused'),
            export_gguf=train_llm.exports_gguf('llama3.2:1b'),
        )
        qwen = train_llm.fuse_args(
            model=train_llm.mlx_repo('qwen2.5:7b'),
            adapter_path=Path('/tmp/adapters'),
            save_path=Path('/tmp/fused'),
            export_gguf=train_llm.exports_gguf('qwen2.5:7b'),
        )
        self.assertIn('--dequantize', llama)
        self.assertIn('--export-gguf', llama)
        self.assertIn('--dequantize', qwen)
        self.assertNotIn('--export-gguf', qwen)


class ProgressWeightTests(unittest.TestCase):
    def test_language_weights_skip_class_images(self) -> None:
        self.assertEqual(train_sdxl.overall_percent('language', 'converting', 1, 1), 10)
        self.assertEqual(train_sdxl.overall_percent('language', 'loading', 1, 1), 20)
        self.assertEqual(train_sdxl.overall_percent('language', 'training', 0, 200), 20)
        self.assertEqual(train_sdxl.overall_percent('lora', 'training', 0, 200), 22)
        self.assertEqual(train_sdxl.overall_percent('language', 'publishing', 1, 1), 100)


class ConvertTests(unittest.TestCase):
    def test_text_files_chunk_without_mask_prompt(self) -> None:
        config = train_llm.load_config()
        with tempfile.TemporaryDirectory() as directory:
            job_dir = Path(directory)
            write_texts(job_dir, count=2)
            data_dir, count, kind = train_llm.convert_dataset(job_dir, config)
            self.assertEqual(kind, 'text')
            self.assertGreater(count, 0)
            format_payload = json.loads((data_dir / 'format.json').read_text(encoding='utf-8'))
            self.assertNotEqual(format_payload.get('mask_prompt'), True)
            self.assertTrue((data_dir / 'train.jsonl').is_file())
            self.assertFalse(train_llm.should_mask_prompt(data_dir))

    def test_chat_jsonl_sets_mask_prompt(self) -> None:
        config = train_llm.load_config()
        with tempfile.TemporaryDirectory() as directory:
            job_dir = Path(directory)
            write_chat(job_dir, count=12)
            data_dir, count, kind = train_llm.convert_dataset(job_dir, config)
            self.assertEqual(kind, 'chat')
            self.assertEqual(count, 12)
            format_payload = json.loads((data_dir / 'format.json').read_text(encoding='utf-8'))
            self.assertEqual(format_payload, {'mask_prompt': True})
            self.assertTrue(train_llm.should_mask_prompt(data_dir))
            valid = train_llm.read_jsonl(data_dir / 'valid.jsonl')
            self.assertEqual(len(valid), 1)


class FilenameTests(unittest.TestCase):
    def test_published_name_strips_safetensors_and_keeps_ollama_tags(self) -> None:
        self.assertEqual(train_llm.default_filename('jerry'), 'jerry')
        self.assertEqual(train_llm.published_name('jerry.safetensors'), 'jerry')
        self.assertEqual(train_llm.published_name('llama3.2:1b'), 'llama3.2:1b')


class StubPublishTests(unittest.TestCase):
    def test_stub_progress_reaches_100_without_comfy_weights(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            write_texts(job_dir, count=3)
            (models / 'loras').mkdir()
            (models / 'checkpoints').mkdir()
            train_sdxl.write_job(job_dir, queued_job())
            train_llm.process_job(models, job_dir, stub=True)
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'succeeded')
            self.assertEqual(job['subject'], 'language')
            self.assertEqual(job['method'], 'lora')
            self.assertEqual(job['filename'], 'jerry')
            self.assertFalse(job['filename'].endswith('.safetensors'))
            self.assertEqual(job['total'], 200)
            self.assertEqual(job['step'], 200)
            progress = json.loads((job_dir / 'progress.json').read_text(encoding='utf-8'))
            self.assertEqual(progress['phase'], 'publishing')
            self.assertEqual(progress['percent'], 100)
            self.assertEqual(progress['step'], progress['total'])
            self.assertTrue((job_dir / 'Modelfile').is_file())
            self.assertIn('FROM ', (job_dir / 'Modelfile').read_text(encoding='utf-8'))
            self.assertEqual(list((models / 'loras').glob('*')), [])
            self.assertEqual(list((models / 'checkpoints').glob('*')), [])

    def test_process_job_method_lora_subject_language_skips_sdxl(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            write_texts(job_dir, count=1)
            train_sdxl.write_job(job_dir, queued_job())
            with (
                mock.patch.object(
                    train_sdxl, 'normalize_job', side_effect=AssertionError('sdxl normalize')
                ) as normalize,
                mock.patch.object(
                    train_sdxl, 'train', side_effect=AssertionError('sdxl train')
                ) as train,
                mock.patch.object(
                    train_sdxl, 'run_stub', side_effect=AssertionError('sdxl stub')
                ) as run_stub,
            ):
                train_sdxl.process_job(models, job_dir, train_sdxl.load_config(), stub=True)
            normalize.assert_not_called()
            train.assert_not_called()
            run_stub.assert_not_called()
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'succeeded')
            self.assertEqual(job['filename'], 'jerry')

    def test_still_lora_with_missing_or_empty_subject_stays_sdxl(self) -> None:
        for extra in ({}, {'subject': ''}):
            with self.subTest(extra=extra):
                with tempfile.TemporaryDirectory() as directory:
                    models = Path(directory)
                    job_dir = models / '.zone-train' / str(uuid.uuid4())
                    job_dir.mkdir(parents=True)
                    write_still_dataset(job_dir, count=2)
                    (models / 'loras').mkdir()
                    train_sdxl.write_job(job_dir, still_job(extra=extra))
                    train_sdxl.process_job(
                        models, job_dir, train_sdxl.load_config(), stub=True
                    )
                    job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
                    self.assertEqual(job['status'], 'succeeded')
                    self.assertEqual(job['filename'], 'jerry.safetensors')
                    self.assertTrue((models / 'loras' / 'jerry.safetensors').is_file())

    def test_finetune_language_does_not_call_weight_name_on_ollama_tag(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / str(uuid.uuid4())
            job_dir.mkdir(parents=True)
            write_texts(job_dir, count=1)
            train_sdxl.write_job(
                job_dir,
                queued_job(method='finetune', extra={'checkpoint': 'llama3.2:1b'}),
            )
            with mock.patch.object(
                train_sdxl, 'weight_name', wraps=train_sdxl.weight_name
            ) as weight_name:
                train_sdxl.process_job(
                    models, job_dir, train_sdxl.load_config(), stub=True
                )
            for args, _kwargs in weight_name.call_args_list:
                self.assertNotIn('llama3.2:1b', args)
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'succeeded')
            self.assertEqual(job['method'], 'finetune')
            self.assertEqual(job['checkpoint'], 'llama3.2:1b')
            self.assertEqual(job['filename'], 'jerry')


if __name__ == '__main__':
    unittest.main()
