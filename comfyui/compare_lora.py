#!/usr/bin/env python3
"""Render one prompt with and without a LoRA so identity retention can be judged."""

from __future__ import annotations

import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

WORKFLOWS = Path(__file__).with_name('workflows')
BASE_WORKFLOW = WORKFLOWS / 'flux1-schnell-fp8-api.json'
ADAPTER_WORKFLOW = WORKFLOWS / 'flux1-schnell-fp8-adapter-api.json'
PROMPT_NODE = '6'
SAMPLER_NODE = '3'
LATENT_NODE = '5'
ADAPTER_NODE = '13'


def env(name: str, default: str = '') -> str:
    return os.environ.get(name, default)


def request(url: str, payload: dict | None = None, timeout: int = 120) -> bytes:
    data = json.dumps(payload).encode() if payload is not None else None
    headers = {'Content-Type': 'application/json'} if data else {}
    call = urllib.request.Request(url, data=data, headers=headers)
    token = env('COMFYUI_API_TOKEN')
    if token:
        call.add_header('X-Zone-ComfyUI-Token', token)
    with urllib.request.urlopen(call, timeout=timeout) as response:
        return response.read()


def graph(path: Path, prompt: str, seed: int, size: int, lora: str | None) -> dict:
    workflow = json.loads(path.read_text())
    workflow[PROMPT_NODE]['inputs']['text'] = prompt
    workflow[SAMPLER_NODE]['inputs']['seed'] = seed
    workflow[LATENT_NODE]['inputs']['width'] = size
    workflow[LATENT_NODE]['inputs']['height'] = size
    if lora is not None:
        workflow[ADAPTER_NODE]['inputs']['lora_name'] = lora
        workflow[ADAPTER_NODE]['inputs']['strength_model'] = float(env('ZONE_LORA_STRENGTH', '1.0'))
    return workflow


def render(base_url: str, workflow: dict, destination: Path, timeout: int) -> None:
    queued = json.loads(request(f'{base_url}/prompt', {'prompt': workflow}).decode())
    if queued.get('error'):
        raise SystemExit(json.dumps(queued)[:2000])
    prompt_id = queued['prompt_id']
    print(f'queued {prompt_id} -> {destination.name}', flush=True)
    deadline = time.time() + timeout
    while time.time() < deadline:
        history = json.loads(request(f'{base_url}/history/{prompt_id}').decode())
        entry = history.get(prompt_id)
        if entry:
            status = ((entry.get('status') or {}).get('status_str') or '').lower()
            if status == 'error':
                raise SystemExit(json.dumps(entry.get('status'), indent=2)[:4000])
            for output in (entry.get('outputs') or {}).values():
                for image in output.get('images') or []:
                    query = urllib.parse.urlencode(
                        {
                            'filename': image['filename'],
                            'subfolder': image.get('subfolder', ''),
                            'type': image.get('type', 'temp'),
                        }
                    )
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    destination.write_bytes(request(f'{base_url}/view?{query}'))
                    print(f'wrote {destination} {destination.stat().st_size} bytes', flush=True)
                    return
        time.sleep(2)
    raise SystemExit(f'render timed out after {timeout}s')


def main() -> None:
    lora = env('ZONE_LORA_NAME')
    prompt = env('ZONE_LORA_PROMPT')
    if not lora or not prompt:
        raise SystemExit('ZONE_LORA_NAME and ZONE_LORA_PROMPT are required')
    out_dir = Path(env('ZONE_LORA_COMPARE_DIR', 'lora-compare'))
    base_url = env('COMFYUI_BASE_URL', 'http://127.0.0.1:8188').rstrip('/')
    seed = int(env('ZONE_LORA_SEED', '0'))
    size = int(env('ZONE_LORA_SIZE', '768'))
    timeout = int(env('ZONE_LORA_TIMEOUT', '900'))
    only = env('ZONE_LORA_ONLY')

    if only != 'with':
        render(base_url, graph(BASE_WORKFLOW, prompt, seed, size, None), out_dir / 'without_lora.png', timeout)
    if only != 'without':
        render(base_url, graph(ADAPTER_WORKFLOW, prompt, seed, size, lora), out_dir / 'with_lora.png', timeout)


if __name__ == '__main__':
    try:
        main()
    except urllib.error.HTTPError as error:
        raise SystemExit(f'ComfyUI HTTP {error.code}: {error.read()[:2000]!r}') from error
