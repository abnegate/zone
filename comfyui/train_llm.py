#!/usr/bin/env python3
"""Host worker for language LoRA and full MLX post-training."""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
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
}
FAMILY_TITLE = {
    'llama3.2': 'Llama-3.2',
    'llama3.1': 'Llama-3.1',
    'llama3': 'Llama-3',
    'llama2': 'Llama-2',
    'qwen2.5': 'Qwen2.5',
    'qwen2': 'Qwen2',
    'qwen3': 'Qwen3',
    'mistral': 'Mistral-7B',
    'mixtral': 'Mixtral-8x7B',
    'gemma2': 'gemma-2',
    'gemma3': 'gemma-3',
    'phi3': 'Phi-3',
    'phi4': 'Phi-4',
}


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
        tag = ''
    elif tag in {'', 'latest'}:
        mapped = OLLAMA_MLX.get(base)
        if mapped:
            return mapped
    return heuristic_mlx(base, tag)


def heuristic_mlx(base: str, tag: str) -> str:
    title = FAMILY_TITLE.get(base, base)
    size = tag if tag and tag not in {'latest', 'instruct', 'chat'} else ''
    if size:
        size_norm = size.upper()
        if re.fullmatch(r'\d+(\.\d+)?', size_norm):
            size_norm = f'{size_norm}B'
        elif size_norm.endswith('B'):
            size_norm = size_norm[:-1] + 'B'
        size_part = f'-{size_norm}'
    elif title.endswith('B'):
        size_part = ''
    else:
        size_part = ''
    if title.lower().startswith('gemma'):
        stem = f'{title}{size_part.lower()}' if size_part else title
        return f'mlx-community/{stem}-it-4bit'
    stem = f'{title}{size_part}' if size_part else title
    if 'Instruct' in stem or 'instruct' in stem:
        return f'mlx-community/{stem}-4bit'
    return f'mlx-community/{stem}-Instruct-4bit'


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
) -> list[str]:
    args = [
        '--model',
        model,
        '--adapter-path',
        str(adapter_path),
        '--save-path',
        str(save_path),
        '--dequantize',
    ]
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
    iter_match = ITER_RE.search(line)
    step = int(iter_match.group(1)) if iter_match else None
    loss_match = LOSS_RE.search(line)
    loss = float(loss_match.group(1)) if loss_match else None
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


def train(models_dir: Path, job_dir: Path, job: dict[str, Any], config: dict[str, Any]) -> None:
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
    export_gguf = exports_gguf(str(job['checkpoint']))
    fuse_command = mlx_command(
        'fuse',
        fuse_args(
            model=model,
            adapter_path=adapter_path,
            save_path=fused_path,
            export_gguf=export_gguf,
        ),
    )
    run_logged(fuse_command, lambda _line: None)
    gguf = fused_path / 'ggml-model-f16.gguf'
    source = str(gguf) if export_gguf and gguf.is_file() else str(fused_path)
    modelfile = job_dir / 'Modelfile'
    write_modelfile(modelfile, source)
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
