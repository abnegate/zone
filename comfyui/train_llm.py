#!/usr/bin/env python3
"""Host worker for language LoRA and full MLX post-training."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
import traceback
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

import train_sdxl  # noqa: E402

CONFIG_PATH = Path(__file__).with_name('train_llm_config.json')
GGUF_TYPES = {'llama', 'mixtral', 'mistral'}
LANGUAGE_METHODS = {'lora', 'finetune'}
ITER_RE = re.compile(r'\bIter(?:ation)?s?\s+(\d+)\b', re.I)
LOSS_RE = re.compile(r'\b(?:Train\s+)?loss\s+([0-9]*\.?[0-9]+)', re.I)
CSI_RE = re.compile(r'\x1b\[[0-9;]*[A-Za-z]')
SIZE_RE = re.compile(r'(\d+(?:\.\d+)?)\s*b\b', re.I)
MIXTRAL_RE = re.compile(r'(\d+)\s*x\s*(\d+(?:\.\d+)?)\s*b\b', re.I)
TEXT_SUFFIXES = {'.txt', '.md', '.text', '.csv'}
JSON_SUFFIXES = {'.json', '.jsonl'}
IMAGE_SUFFIXES = {'.png', '.jpg', '.jpeg', '.webp', '.gif'}
OLLAMA_MLX = {
    'llama3.2:1b': 'mlx-community/Llama-3.2-1B-Instruct-4bit',
    'llama3.2:3b': 'mlx-community/Llama-3.2-3B-Instruct-4bit',
    'llama3.2': 'mlx-community/Llama-3.2-3B-Instruct-4bit',
    'llama3.2:latest': 'mlx-community/Llama-3.2-3B-Instruct-4bit',
    'llama3.1:8b': 'mlx-community/Meta-Llama-3.1-8B-Instruct-4bit',
    'llama3.1': 'mlx-community/Meta-Llama-3.1-8B-Instruct-4bit',
    'llama3.1:latest': 'mlx-community/Meta-Llama-3.1-8B-Instruct-4bit',
    'llama3:8b': 'mlx-community/Meta-Llama-3-8B-Instruct-4bit',
    'llama3': 'mlx-community/Meta-Llama-3-8B-Instruct-4bit',
    'mistral': 'mlx-community/Mistral-7B-Instruct-v0.3-4bit',
    'mistral:7b': 'mlx-community/Mistral-7B-Instruct-v0.3-4bit',
    'mistral:latest': 'mlx-community/Mistral-7B-Instruct-v0.3-4bit',
    'mixtral': 'mlx-community/Mixtral-8x7B-Instruct-v0.1-4bit',
    'mixtral:8x7b': 'mlx-community/Mixtral-8x7B-Instruct-v0.1-4bit',
    'qwen2.5:0.5b': 'mlx-community/Qwen2.5-0.5B-Instruct-4bit',
    'qwen2.5:1.5b': 'mlx-community/Qwen2.5-1.5B-Instruct-4bit',
    'qwen2.5:3b': 'mlx-community/Qwen2.5-3B-Instruct-4bit',
    'qwen2.5:7b': 'mlx-community/Qwen2.5-7B-Instruct-4bit',
    'qwen2.5:14b': 'mlx-community/Qwen2.5-14B-Instruct-4bit',
    'qwen2.5:32b': 'mlx-community/Qwen2.5-32B-Instruct-4bit',
    'qwen2.5': 'mlx-community/Qwen2.5-7B-Instruct-4bit',
    'qwen2.5:latest': 'mlx-community/Qwen2.5-7B-Instruct-4bit',
    'qwen3.8:27b': 'mlx-community/Qwen3.8-27B-4bit',
    'qwen38u:32k': 'mlx-community/Qwen3.8-27B-4bit',
    'qwen3.8': 'mlx-community/Qwen3.8-27B-4bit',
}
SMALL_LLAMA_TAGS = {'llama3.2:1b', 'llama3.2:3b'}
OLLAMA_HF = {
    'llama3.2:1b': 'unsloth/Llama-3.2-1B-Instruct',
    'llama3.2:3b': 'unsloth/Llama-3.2-3B-Instruct',
    'llama3.2': 'unsloth/Llama-3.2-3B-Instruct',
    'llama3.2:latest': 'unsloth/Llama-3.2-3B-Instruct',
    'llama3.1:8b': 'unsloth/Meta-Llama-3.1-8B-Instruct',
    'llama3.1': 'unsloth/Meta-Llama-3.1-8B-Instruct',
    'llama3.1:latest': 'unsloth/Meta-Llama-3.1-8B-Instruct',
    'llama3:8b': 'unsloth/Llama-3-8B-Instruct',
    'llama3': 'unsloth/Llama-3-8B-Instruct',
    'mistral': 'unsloth/mistral-7b-instruct-v0.3',
    'mistral:7b': 'unsloth/mistral-7b-instruct-v0.3',
    'mistral:latest': 'unsloth/mistral-7b-instruct-v0.3',
    'mixtral': 'unsloth/mixtral-8x7b-instruct-v0.1',
    'mixtral:8x7b': 'unsloth/mixtral-8x7b-instruct-v0.1',
    'qwen2.5:0.5b': 'Qwen/Qwen2.5-0.5B-Instruct',
    'qwen2.5:1.5b': 'Qwen/Qwen2.5-1.5B-Instruct',
    'qwen2.5:3b': 'Qwen/Qwen2.5-3B-Instruct',
    'qwen2.5:7b': 'Qwen/Qwen2.5-7B-Instruct',
    'qwen2.5:14b': 'Qwen/Qwen2.5-14B-Instruct',
    'qwen2.5:32b': 'Qwen/Qwen2.5-32B-Instruct',
    'qwen2.5': 'Qwen/Qwen2.5-7B-Instruct',
    'qwen2.5:latest': 'Qwen/Qwen2.5-7B-Instruct',
    'qwen3.8:27b': 'Qwen/Qwen3-32B',
    'qwen38u:32k': 'Qwen/Qwen3-32B',
    'qwen3.8': 'Qwen/Qwen3-32B',
}
CUDA_QLORA_MILLION = 14000
LORA_TARGETS = ['q_proj', 'k_proj', 'v_proj', 'o_proj']


def load_config(path: Path | None = None) -> dict[str, Any]:
    source = Path(path) if path is not None else CONFIG_PATH
    return json.loads(source.read_text(encoding='utf-8'))


def steps_for(example_count: int, config: dict[str, Any]) -> int:
    budget = max(int(example_count), 0) * int(config['iters_per_example'])
    return min(max(budget, int(config['min_iters'])), int(config['max_iters']))


def llm_python() -> Path:
    return ROOT / '.venv-train-llm' / 'bin' / 'python'


def default_filename(name: str) -> str:
    name = (name or 'language').strip() or 'language'
    if name.endswith('.safetensors'):
        name = name[: -len('.safetensors')]
    slug = re.sub(r'[^A-Za-z0-9._:-]+', '-', name).strip('.-')
    return slug or 'language'


def published_name(name: str) -> str:
    name = (name or '').strip()
    if name.endswith('.safetensors'):
        name = name[: -len('.safetensors')]
    if not name or name != Path(name).name or '/' in name or '\\' in name or '..' in name:
        raise ValueError(f'invalid filename: {name!r}')
    return name


def mlx_repo(checkpoint: str) -> str:
    name = (checkpoint or '').strip()
    if not name:
        raise ValueError('missing checkpoint')
    if '/' in name:
        return name
    key = name.lower()
    mapped = OLLAMA_MLX.get(key)
    if mapped:
        return mapped
    base, separator, tag = key.partition(':')
    if not separator:
        mapped = OLLAMA_MLX.get(base)
        if mapped:
            return mapped
    elif tag in {'', 'latest'}:
        mapped = OLLAMA_MLX.get(base)
        if mapped:
            return mapped
    raise ValueError(f'no MLX mapping for {name}')


def hf_repo(checkpoint: str) -> str:
    name = (checkpoint or '').strip()
    if not name:
        raise ValueError('missing checkpoint')
    if '/' in name:
        if name.lower().startswith('mlx-community/'):
            raise ValueError(f'no Hugging Face mapping for {name}')
        return name
    key = name.lower()
    mapped = OLLAMA_HF.get(key)
    if mapped:
        return mapped
    base, separator, tag = key.partition(':')
    if not separator:
        mapped = OLLAMA_HF.get(base)
        if mapped:
            return mapped
    elif tag in {'', 'latest'}:
        mapped = OLLAMA_HF.get(base)
        if mapped:
            return mapped
    raise ValueError(f'no Hugging Face mapping for {name}')


def cuda_available() -> bool:
    try:
        import torch
    except ImportError:
        return False
    return bool(torch.cuda.is_available())


def publish_dir(models_dir: Path, job: dict[str, Any]) -> Path:
    filename = published_name(str(job.get('filename') or default_filename(str(job.get('name') or 'language'))))
    return Path(models_dir) / 'llm' / filename


def family_name(name: str) -> str:
    lowered = name.lower()
    if 'mixtral' in lowered:
        return 'mixtral'
    if 'mistral' in lowered:
        return 'mistral'
    if 'qwen' in lowered:
        return 'qwen'
    if 'llama' in lowered:
        return 'llama'
    if 'gemma' in lowered:
        return 'gemma'
    if 'phi' in lowered:
        return 'phi'
    return 'other'


def exports_gguf(checkpoint: str) -> bool:
    return family_name(checkpoint) in GGUF_TYPES


def param_millions(name: str) -> int | None:
    lowered = name.lower().replace('_', '-')
    mixtral = MIXTRAL_RE.search(lowered)
    if mixtral:
        return int(float(mixtral.group(1)) * float(mixtral.group(2)) * 1000)
    matches = SIZE_RE.findall(lowered)
    if not matches:
        return None
    return int(float(matches[-1]) * 1000)


def should_dequantize(checkpoint: str) -> bool:
    lowered = (checkpoint or '').strip().lower()
    if lowered in SMALL_LLAMA_TAGS:
        return True
    millions = param_millions(checkpoint)
    return millions is not None and millions < 8000


def fine_tune_type(method: str, checkpoint: str, config: dict[str, Any]) -> str:
    if (method or 'lora') != 'finetune':
        return 'lora'
    millions = param_millions(checkpoint)
    if millions is None:
        millions = param_millions(mlx_repo(checkpoint))
    limit = int(config['finetune_max_params_million'])
    if millions is not None and millions > limit:
        return 'lora'
    return 'full'


def infer_kind(examples: list[dict[str, Any]]) -> str:
    if not examples:
        return 'text'
    sample = examples[0]
    if 'messages' in sample:
        return 'chat'
    if 'prompt' in sample and 'completion' in sample:
        return 'completions'
    return 'text'


def format_document(kind: str) -> dict[str, Any]:
    if kind in {'chat', 'completions'}:
        return {'mask_prompt': True}
    return {'text': True}


def mask_prompt_enabled(data_dir: Path) -> bool:
    path = Path(data_dir) / 'format.json'
    if not path.is_file():
        return False
    try:
        payload = json.loads(path.read_text(encoding='utf-8'))
    except ValueError:
        return False
    if not isinstance(payload, dict):
        return False
    return payload.get('mask_prompt') is True


def should_mask_prompt(data_dir: Path) -> bool:
    if not mask_prompt_enabled(data_dir):
        return False
    train_path = Path(data_dir) / 'train.jsonl'
    if not train_path.is_file():
        return False
    kind = infer_kind(read_jsonl(train_path))
    return kind in {'chat', 'completions'}


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.is_file():
        return []
    examples: list[dict[str, Any]] = []
    for line in path.read_text(encoding='utf-8').splitlines():
        line = line.strip()
        if not line:
            continue
        payload = json.loads(line)
        if isinstance(payload, dict):
            examples.append(payload)
    return examples


def write_jsonl(path: Path, examples: list[dict[str, Any]]) -> None:
    body = ''.join(json.dumps(item, ensure_ascii=False) + '\n' for item in examples)
    train_sdxl.atomic_write(path, body)


def load_json_examples(path: Path) -> list[dict[str, Any]]:
    text = path.read_text(encoding='utf-8').strip()
    if not text:
        return []
    if text[0] == '[':
        payload = json.loads(text)
        if not isinstance(payload, list):
            return []
        return [item for item in payload if isinstance(item, dict)]
    if text[0] == '{':
        try:
            payload = json.loads(text)
        except ValueError:
            payload = None
        if isinstance(payload, dict):
            return [payload]
    return read_jsonl(path)


def list_sources(job_dir: Path) -> list[Path]:
    dataset = Path(job_dir) / 'dataset'
    if not dataset.is_dir():
        return []
    files: list[Path] = []
    for path in sorted(dataset.iterdir()):
        if not path.is_file():
            continue
        name = path.name.lower()
        if name.endswith('.mask.png') or name.endswith('.kind') or name.endswith('.pose'):
            continue
        if path.suffix.lower() in IMAGE_SUFFIXES:
            continue
        files.append(path)
    return files


def chunk_text(text: str, size: int, overlap: int) -> list[str]:
    text = text.strip()
    if not text:
        return []
    size = max(int(size), 1)
    overlap = min(max(int(overlap), 0), size - 1)
    if len(text) <= size:
        return [text]
    chunks: list[str] = []
    start = 0
    step = size - overlap
    while start < len(text):
        end = min(start + size, len(text))
        chunk = text[start:end].strip()
        if chunk:
            chunks.append(chunk)
        if end >= len(text):
            break
        start += step
    return chunks


def split_examples(
    examples: list[dict[str, Any]], fraction: float
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    if len(examples) < 2 or fraction <= 0:
        return examples, []
    valid_count = int(round(len(examples) * fraction))
    valid_count = min(max(valid_count, 1), len(examples) - 1)
    return examples[:-valid_count], examples[-valid_count:]


def load_examples(job_dir: Path, config: dict[str, Any]) -> tuple[list[dict[str, Any]], str]:
    data_dir = Path(job_dir) / 'data'
    existing = data_dir / 'train.jsonl'
    if existing.is_file():
        examples = read_jsonl(existing)
        return examples, infer_kind(examples)
    structured: list[dict[str, Any]] = []
    texts: list[str] = []
    for path in list_sources(job_dir):
        suffix = path.suffix.lower()
        if suffix in JSON_SUFFIXES:
            structured.extend(load_json_examples(path))
        elif suffix in TEXT_SUFFIXES:
            texts.append(path.read_text(encoding='utf-8'))
        else:
            try:
                texts.append(path.read_text(encoding='utf-8'))
            except UnicodeDecodeError:
                continue
    if structured:
        return structured, infer_kind(structured)
    examples: list[dict[str, Any]] = []
    for text in texts:
        for chunk in chunk_text(text, int(config['chunk_chars']), int(config['chunk_overlap'])):
            examples.append({'text': chunk})
    return examples, 'text'


def convert_dataset(job_dir: Path, config: dict[str, Any]) -> tuple[Path, int, str]:
    data_dir = Path(job_dir) / 'data'
    existing = data_dir / 'train.jsonl'
    if existing.is_file():
        examples = read_jsonl(existing)
        kind = infer_kind(examples)
        format_path = data_dir / 'format.json'
        if not format_path.is_file():
            train_sdxl.write_json(format_path, format_document(kind))
        return data_dir, len(examples), kind
    examples, kind = load_examples(job_dir, config)
    data_dir.mkdir(parents=True, exist_ok=True)
    train_examples, valid_examples = split_examples(examples, float(config['valid_fraction']))
    write_jsonl(data_dir / 'train.jsonl', train_examples)
    if valid_examples:
        write_jsonl(data_dir / 'valid.jsonl', valid_examples)
    train_sdxl.write_json(data_dir / 'format.json', format_document(kind))
    return data_dir, len(examples), kind


def write_lora_config(path: Path, config: dict[str, Any]) -> None:
    params = config['lora_parameters']
    body = (
        'lora_parameters:\n'
        f'  rank: {int(params["rank"])}\n'
        f'  dropout: {float(params["dropout"])}\n'
        f'  scale: {float(params["scale"])}\n'
    )
    train_sdxl.atomic_write(path, body)


def mlx_command(subcommand: str, args: list[str]) -> list[str]:
    return [str(llm_python()), '-m', 'mlx_lm', subcommand, *args]


def lora_args(
    *,
    model: str,
    data: Path,
    iters: int,
    batch_size: int,
    fine_tune: str,
    adapter_path: Path,
    num_layers: int,
    learning_rate: float,
    seed: int,
    save_every: int,
    config_path: Path,
    mask_prompt: bool,
) -> list[str]:
    args = [
        '--model',
        model,
        '--train',
        '--data',
        str(data),
        '--iters',
        str(iters),
        '--batch-size',
        str(batch_size),
        '--fine-tune-type',
        fine_tune,
        '--adapter-path',
        str(adapter_path),
        '--num-layers',
        str(num_layers),
        '--learning-rate',
        str(learning_rate),
        '--seed',
        str(seed),
        '--save-every',
        str(save_every),
        '--config',
        str(config_path),
    ]
    if mask_prompt:
        args.append('--mask-prompt')
    return args


def fuse_args(
    *,
    model: str,
    adapter_path: Path,
    save_path: Path,
    export_gguf: bool,
    dequantize: bool,
) -> list[str]:
    args = [
        '--model',
        model,
        '--adapter-path',
        str(adapter_path),
        '--save-path',
        str(save_path),
    ]
    if dequantize:
        args.append('--dequantize')
        if export_gguf:
            args.append('--export-gguf')
    return args


def write_modelfile(path: Path, source: str) -> None:
    train_sdxl.atomic_write(path, f'FROM {source}\n')


def normalize_job(job: dict[str, Any], job_dir: Path) -> dict[str, Any]:
    if (job.get('subject') or '') != 'language':
        raise ValueError(f'unsupported train subject: {job.get("subject")!r}')
    method = job.get('method') or 'lora'
    if method not in LANGUAGE_METHODS:
        raise ValueError(f'unsupported train method: {method!r}')
    job['schema_version'] = int(job.get('schema_version') or 1)
    job['method'] = method
    job['subject'] = 'language'
    job['name'] = str(job.get('name') or 'language')
    job['trigger'] = str(job.get('trigger') or '')
    checkpoint = str(job.get('checkpoint') or '').strip()
    if not checkpoint:
        raise ValueError('missing checkpoint')
    job['checkpoint'] = checkpoint
    job['filename'] = published_name(str(job.get('filename') or default_filename(job['name'])))
    if not job.get('recipe_id'):
        job['recipe_id'] = ''
    return job


def save_every_for(iters: int) -> int:
    return max(1, min(100, int(iters)))


def run_stub(models_dir: Path, job_dir: Path, job: dict[str, Any], config: dict[str, Any]) -> None:
    _ = models_dir
    progress = train_sdxl.Progress(job_dir, 'language', 1)
    progress.emit('converting', phase_step=0, phase_total=1)
    _data_dir, example_count, _kind = convert_dataset(job_dir, config)
    total = steps_for(example_count, config)
    job['image_count'] = example_count
    job['total'] = total
    job['step'] = 0
    job['filename'] = published_name(str(job.get('filename') or default_filename(job['name'])))
    progress = train_sdxl.Progress(job_dir, 'language', total)
    progress.emit('converting', phase_step=1, phase_total=1)
    train_sdxl.write_job(job_dir, job)
    progress.emit('loading', phase_step=1, phase_total=1)
    progress.emit('training', phase_step=total, phase_total=total)
    progress.emit('publishing', step=0, phase_step=0, phase_total=1)
    write_modelfile(Path(job_dir) / 'Modelfile', str(job['checkpoint']))
    job['step'] = total
    progress.emit('publishing', step=total, phase_step=1, phase_total=1)
    train_sdxl.write_job(job_dir, job)


def parse_iter(line: str) -> tuple[int | None, float | None]:
    # mlx TrainUI wraps tokens in CSI when make_console forces truecolor.
    line = CSI_RE.sub('', line)
    iter_match = ITER_RE.search(line)
    loss_match = LOSS_RE.search(line)
    if iter_match:
        step = int(iter_match.group(1))
        loss = float(loss_match.group(1)) if loss_match else None
        return step, loss
    tokens = line.split()
    if len(tokens) < 2:
        return None, None
    try:
        step = int(tokens[0])
        loss = float(tokens[1])
    except ValueError:
        return None, None
    return step, loss


def run_logged(command: list[str], on_line: Any) -> None:
    env = os.environ.copy()
    env['PYTHONUNBUFFERED'] = '1'
    process = subprocess.Popen(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        env=env,
    )
    assert process.stdout is not None
    buffer = ''
    while True:
        chunk = process.stdout.read(256)
        if chunk == '':
            break
        buffer += chunk
        while True:
            split_at = min(
                (index for index in (buffer.find('\n'), buffer.find('\r')) if index >= 0),
                default=-1,
            )
            if split_at < 0:
                break
            line = buffer[:split_at].strip()
            buffer = buffer[split_at + 1 :]
            if line:
                on_line(line)
    if buffer.strip():
        on_line(buffer.strip())
    code = process.wait()
    if code != 0:
        raise RuntimeError(f'command failed ({code}): {" ".join(command)}')


def require_llm_python() -> Path:
    python = llm_python()
    if not python.is_file():
        raise FileNotFoundError(f'LLM trainer venv is missing: {python}')
    return python


def ollama_create(filename: str, modelfile: Path) -> None:
    subprocess.run(['ollama', 'create', filename, '-f', str(modelfile)], check=True)


def import_published(models_dir: Path, job: dict[str, Any]) -> None:
    directory = publish_dir(models_dir, job)
    modelfile = directory / 'Modelfile'
    if not modelfile.is_file():
        return
    if not shutil.which('ollama'):
        return
    ollama_create(directory.name, modelfile)


def example_text(example: dict[str, Any], tokenizer: Any | None = None) -> str:
    messages = example.get('messages')
    if isinstance(messages, list):
        if tokenizer is not None and hasattr(tokenizer, 'apply_chat_template'):
            try:
                return str(
                    tokenizer.apply_chat_template(
                        messages, tokenize=False, add_generation_prompt=False
                    )
                )
            except Exception:
                pass
        parts = []
        for message in messages:
            if isinstance(message, dict):
                parts.append(str(message.get('content') or ''))
        return '\n'.join(part for part in parts if part)
    if 'prompt' in example or 'completion' in example:
        return f'{example.get("prompt") or ""}{example.get("completion") or ""}'
    return str(example.get('text') or '')


def ensure_bitsandbytes() -> None:
    try:
        import bitsandbytes  # noqa: F401
        return
    except ImportError:
        pass
    subprocess.run(
        [
            sys.executable,
            '-m',
            'pip',
            'install',
            '--disable-pip-version-check',
            'bitsandbytes',
        ],
        check=True,
    )
    import bitsandbytes  # noqa: F401


def train_cuda(
    models_dir: Path, job_dir: Path, job: dict[str, Any], config: dict[str, Any]
) -> None:
    import torch
    from peft import LoraConfig, get_peft_model
    from transformers import AutoModelForCausalLM, AutoTokenizer

    job_dir = Path(job_dir)
    progress = train_sdxl.Progress(job_dir, 'language', 1)
    progress.emit('converting', phase_step=0, phase_total=1)
    data_dir, example_count, _kind = convert_dataset(job_dir, config)
    if example_count <= 0:
        raise RuntimeError(f'no language examples in {job_dir / "dataset"}')
    total = steps_for(example_count, config)
    job['image_count'] = example_count
    job['total'] = total
    job['step'] = job.get('step') or 0
    job['filename'] = published_name(str(job.get('filename') or default_filename(job['name'])))
    train_sdxl.write_job(job_dir, job)
    progress = train_sdxl.Progress(job_dir, 'language', total)
    progress.emit('converting', phase_step=1, phase_total=1)

    repo = hf_repo(str(job['checkpoint']))
    millions = param_millions(str(job['checkpoint']))
    if millions is None:
        millions = param_millions(repo)
    load_kwargs: dict[str, Any] = {
        'torch_dtype': torch.bfloat16,
        'device_map': 'auto',
    }
    if millions is not None and millions >= CUDA_QLORA_MILLION:
        try:
            ensure_bitsandbytes()
            from transformers import BitsAndBytesConfig

            load_kwargs['quantization_config'] = BitsAndBytesConfig(
                load_in_4bit=True,
                bnb_4bit_quant_type='nf4',
                bnb_4bit_compute_dtype=torch.bfloat16,
            )
        except Exception as error:
            raise RuntimeError(
                'models ≥14B need bitsandbytes 4-bit QLoRA on CUDA; refusing to load full weights on this GPU'
            ) from error

    progress.emit('loading', phase_step=0, phase_total=1)
    tokenizer = AutoTokenizer.from_pretrained(repo)
    if tokenizer.pad_token is None:
        tokenizer.pad_token = tokenizer.eos_token
    model = AutoModelForCausalLM.from_pretrained(repo, **load_kwargs)
    params = config['lora_parameters']
    model = get_peft_model(
        model,
        LoraConfig(
            r=int(params['rank']),
            lora_alpha=int(params['rank']),
            lora_dropout=float(params['dropout']),
            target_modules=LORA_TARGETS,
            task_type='CAUSAL_LM',
            bias='none',
        ),
    )
    optimizer = torch.optim.AdamW(
        [parameter for parameter in model.parameters() if parameter.requires_grad],
        lr=float(config['learning_rate']),
    )
    model.train()
    progress.emit('loading', phase_step=1, phase_total=1)

    examples = read_jsonl(data_dir / 'train.jsonl')
    texts = [example_text(example, tokenizer) for example in examples]
    texts = [text for text in texts if text.strip()]
    if not texts:
        raise RuntimeError(f'no language examples in {job_dir / "dataset"}')

    progress.emit('training', phase_step=0, phase_total=total)
    for step in range(1, total + 1):
        encoded = tokenizer(
            texts[(step - 1) % len(texts)],
            return_tensors='pt',
            truncation=True,
            max_length=int(config.get('chunk_chars') or 2048),
        )
        input_ids = encoded['input_ids'].to('cuda')
        attention_mask = encoded.get('attention_mask')
        if attention_mask is not None:
            attention_mask = attention_mask.to('cuda')
        optimizer.zero_grad(set_to_none=True)
        outputs = model(
            input_ids=input_ids,
            attention_mask=attention_mask,
            labels=input_ids,
        )
        loss = outputs.loss
        loss.backward()
        optimizer.step()
        job['step'] = step
        progress.emit(
            'training',
            step=step,
            phase_step=step,
            phase_total=total,
            loss=float(loss.detach().cpu()),
        )
        if step == 1 or step % 25 == 0 or step == total:
            train_sdxl.write_job(job_dir, job)

    progress.emit('publishing', step=0, phase_step=0, phase_total=1)
    destination = publish_dir(models_dir, job)
    if destination.exists():
        shutil.rmtree(destination)
    destination.mkdir(parents=True, exist_ok=True)
    model.save_pretrained(destination)
    write_modelfile(destination / 'Modelfile', str(destination))
    write_modelfile(job_dir / 'Modelfile', str(destination))
    if shutil.which('ollama'):
        ollama_create(str(job['filename']), destination / 'Modelfile')
    job['step'] = total
    progress.emit('publishing', step=total, phase_step=1, phase_total=1)
    train_sdxl.write_job(job_dir, job)


def train(models_dir: Path, job_dir: Path, job: dict[str, Any], config: dict[str, Any]) -> None:
    if cuda_available():
        train_cuda(models_dir, job_dir, job, config)
        return
    _ = models_dir
    job_dir = Path(job_dir)
    progress = train_sdxl.Progress(job_dir, 'language', 1)
    progress.emit('converting', phase_step=0, phase_total=1)
    data_dir, example_count, _kind = convert_dataset(job_dir, config)
    if example_count <= 0:
        raise RuntimeError(f'no language examples in {job_dir / "dataset"}')
    total = steps_for(example_count, config)
    job['image_count'] = example_count
    job['total'] = total
    job['step'] = job.get('step') or 0
    job['filename'] = published_name(str(job.get('filename') or default_filename(job['name'])))
    train_sdxl.write_job(job_dir, job)
    progress = train_sdxl.Progress(job_dir, 'language', total)
    progress.emit('converting', phase_step=1, phase_total=1)

    require_llm_python()
    model = mlx_repo(str(job['checkpoint']))
    adapter_path = job_dir / 'adapters'
    fused_path = job_dir / 'fused'
    config_path = job_dir / 'lora_config.yaml'
    write_lora_config(config_path, config)
    fine_tune = fine_tune_type(str(job['method']), str(job['checkpoint']), config)
    mask_prompt = should_mask_prompt(data_dir)
    train_command = mlx_command(
        'lora',
        lora_args(
            model=model,
            data=data_dir,
            iters=total,
            batch_size=int(config['batch_size']),
            fine_tune=fine_tune,
            adapter_path=adapter_path,
            num_layers=int(config['num_layers']),
            learning_rate=float(config['learning_rate']),
            seed=0,
            save_every=save_every_for(total),
            config_path=config_path,
            mask_prompt=mask_prompt,
        ),
    )
    progress.emit('loading', phase_step=0, phase_total=1)

    def on_line(line: str) -> None:
        step, loss = parse_iter(line)
        if step is None:
            return
        step = min(max(step, 0), total)
        job['step'] = step
        progress.emit(
            'training',
            step=step,
            phase_step=step,
            phase_total=total,
            loss=loss,
        )

    progress.emit('loading', phase_step=1, phase_total=1)
    progress.emit('training', phase_step=0, phase_total=total)
    run_logged(train_command, on_line)

    progress.emit('publishing', step=0, phase_step=0, phase_total=1)
    checkpoint = str(job['checkpoint'])
    dequantize = should_dequantize(checkpoint)
    export_gguf = exports_gguf(checkpoint) and dequantize
    fuse_command = mlx_command(
        'fuse',
        fuse_args(
            model=model,
            adapter_path=adapter_path,
            save_path=fused_path,
            export_gguf=export_gguf,
            dequantize=dequantize,
        ),
    )
    run_logged(fuse_command, lambda _line: None)
    gguf = fused_path / 'ggml-model-f16.gguf'
    modelfile = job_dir / 'Modelfile'
    if gguf.is_file():
        source = str(gguf)
    elif dequantize:
        source = str(fused_path)
    else:
        source = None
    write_modelfile(modelfile, source or str(fused_path))
    if source is not None:
        ollama_create(str(job['filename']), modelfile)
    job['step'] = total
    progress.emit('publishing', step=total, phase_step=1, phase_total=1)
    train_sdxl.write_job(job_dir, job)


def stub_requested(stub: bool) -> bool:
    return bool(stub) or os.environ.get('ZONE_TRAIN_STUB', '') == '1'


def process_job(models_dir: Path, job_dir: Path, stub: bool) -> None:
    job = train_sdxl.load_job(job_dir)
    try:
        job = normalize_job(job, job_dir)
        job = train_sdxl.mark_running(job_dir, job)
        config = load_config()
        if stub_requested(stub):
            run_stub(models_dir, job_dir, job, config)
        else:
            train(models_dir, job_dir, job, config)
        train_sdxl.mark_succeeded(job_dir, job)
    except Exception as error:
        train_sdxl.mark_failed(job_dir, job, error)
        raise


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description='Zone host language trainer')
    parser.add_argument(
        '--models-dir',
        default=os.environ.get('COMFYUI_MODELS_DIR') or './models',
    )
    parser.add_argument('--once', action='store_true')
    parser.add_argument('--stub', action='store_true')
    args = parser.parse_args(argv)
    models_dir = Path(args.models_dir).expanduser()
    stub = stub_requested(args.stub)
    while True:
        job_dir = train_sdxl.find_job(models_dir)
        if job_dir is None:
            if args.once:
                return 0
            time.sleep(train_sdxl.WATCH_INTERVAL)
            continue
        try:
            print(f'processing {job_dir}', flush=True)
            process_job(models_dir, job_dir, stub)
            print(f'finished {job_dir}', flush=True)
        except Exception:
            traceback.print_exc()
            if args.once:
                return 1
        if args.once:
            return 0


if __name__ == '__main__':
    sys.exit(main())

