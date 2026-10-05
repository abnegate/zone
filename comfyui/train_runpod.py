#!/usr/bin/env python3
"""Host-side Runpod REST v2 client for person train jobs."""

from __future__ import annotations

import base64
import io
import json
import os
import shutil
import subprocess
import sys
import tarfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any, Callable

ROOT = Path(__file__).resolve().parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

import train_sdxl  # noqa: E402

API_BASE = 'https://api.runpod.io'
POD_IMAGE = 'runpod/pytorch:1.0.2-cu1281-torch280-ubuntu2404'
POD_PORT = 8888
POLL_INTERVAL = 2
KEY_NAME = 'runpod.key'
USABLE_AVAILABILITY = frozenset({'LOW', 'MEDIUM', 'HIGH'})
CLOUDS = ('COMMUNITY', 'SECURE')
FINETUNE_MEMORY = 48
ADAPTER_MEMORY = 24
FINETUNE_DISK = 150
ADAPTER_DISK = 40
CATALOG_QUERY = urllib.parse.urlencode(
    {'include': 'AVAILABILITY', 'product': 'POD'}
)
PACKAGES = (
    'diffusers',
    'peft',
    'accelerate',
    'transformers',
    'safetensors',
    'pillow',
    'prodigyopt',
)
SCRIPTS = (
    'train_sdxl.py',
    'train_wan.py',
    'train_wan_config.json',
    'sdxl_checkpoint.py',
    'train_runpod.py',
)

BOOTSTRAP = r'''from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import io, json, sys, tarfile, threading
ROOT = Path('/workspace')
STATE = {'status': 'waiting', 'error': None}

def send(handler, code, body, ctype='application/octet-stream'):
    payload = body if isinstance(body, bytes) else body.encode()
    handler.send_response(code)
    handler.send_header('Content-Type', ctype)
    handler.send_header('Content-Length', str(len(payload)))
    handler.end_headers()
    handler.wfile.write(payload)

def progress_bytes():
    root = ROOT / 'models' / '.zone-train'
    if root.is_dir():
        for entry in root.iterdir():
            path = entry / 'progress.json'
            if path.is_file():
                return path.read_bytes()
    return b'{}'

def go(data):
    STATE['status'] = 'running'
    try:
        with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as archive:
            archive.extractall(ROOT)
        sys.path.insert(0, str(ROOT))
        import train_runpod
        train_runpod.run_remote(ROOT)
        STATE['status'] = 'succeeded'
    except Exception as error:
        STATE['status'] = 'failed'
        STATE['error'] = str(error)

class Handler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        return
    def do_GET(self):
        path = self.path.split('?', 1)[0]
        if path == '/status':
            send(self, 200, json.dumps(STATE), 'application/json')
            return
        if path == '/progress':
            send(self, 200, progress_bytes(), 'application/json')
            return
        if path == '/artifact':
            artifact = ROOT / 'artifact.tar.gz'
            if not artifact.is_file():
                send(self, 404, b'')
                return
            send(self, 200, artifact.read_bytes(), 'application/gzip')
            return
        send(self, 404, b'')
    def do_PUT(self):
        self.receive()
    def do_POST(self):
        self.receive()
    def receive(self):
        length = int(self.headers.get('Content-Length') or 0)
        data = self.rfile.read(length)
        threading.Thread(target=go, args=(data,), daemon=True).start()
        send(self, 202, json.dumps({'ok': True}), 'application/json')

ThreadingHTTPServer(('0.0.0.0', 8888), Handler).serve_forever()
'''


class HttpError(RuntimeError):
    def __init__(self, status: int, detail: str = '') -> None:
        self.status = int(status)
        super().__init__(detail or f'HTTP {self.status}')


class Client:
    def __init__(self, key: str, base: str = API_BASE) -> None:
        self.key = key
        self.base = str(base).rstrip('/')

    def request(
        self,
        method: str,
        path: str,
        body: dict[str, Any] | None = None,
        timeout: int = 60,
    ) -> tuple[int, bytes]:
        url = path if path.startswith('http://') or path.startswith('https://') else self.base + path
        headers = {'Authorization': f'Bearer {self.key}'}
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            headers['Content-Type'] = 'application/json'
        call = urllib.request.Request(url, data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(call, timeout=timeout) as response:
                return int(getattr(response, 'status', 200) or 200), response.read()
        except urllib.error.HTTPError as error:
            raw = error.read()
            raise HttpError(error.code, http_detail(raw)) from error

    def json(
        self,
        method: str,
        path: str,
        body: dict[str, Any] | None = None,
        timeout: int = 60,
    ) -> Any:
        _status, payload = self.request(method, path, body, timeout=timeout)
        if not payload:
            return {}
        return json.loads(payload.decode())

    def list_gpus(self) -> list[dict[str, Any]]:
        payload = self.json('GET', f'/v2/catalog/gpus?{CATALOG_QUERY}')
        gpus = payload.get('gpus') if isinstance(payload, dict) else None
        if not isinstance(gpus, list):
            raise RuntimeError('Runpod GPU catalog is missing gpus')
        return [gpu for gpu in gpus if isinstance(gpu, dict)]

    def create_pod(self, body: dict[str, Any]) -> dict[str, Any]:
        payload = self.json('POST', '/v2/pods', body, timeout=60)
        if not isinstance(payload, dict) or not payload.get('id'):
            raise RuntimeError('Runpod create did not return a pod id')
        return payload

    def get_pod(self, pod_id: str) -> dict[str, Any]:
        payload = self.json('GET', f'/v2/pods/{pod_id}')
        if not isinstance(payload, dict):
            raise RuntimeError('Runpod pod response is not an object')
        return payload

    def delete_pod(self, pod_id: str) -> None:
        try:
            self.request('DELETE', f'/v2/pods/{pod_id}')
        except HttpError as error:
            if error.status != 404:
                raise


def http_detail(raw: bytes) -> str:
    text = raw.decode('utf-8', 'replace').strip()
    try:
        payload = json.loads(text)
    except ValueError:
        return text
    if isinstance(payload, dict):
        return str(payload.get('detail') or payload.get('title') or text)
    return text


def load_key(job_dir: Path) -> str:
    path = Path(job_dir) / KEY_NAME
    if not path.is_file():
        raise RuntimeError('Runpod API key is missing')
    key = path.read_text(encoding='utf-8').strip()
    path.unlink()
    if not key:
        raise RuntimeError('Runpod API key is missing')
    return key


def consume_key_file(job_dir: Path) -> str | None:
    path = Path(job_dir) / KEY_NAME
    if not path.is_file():
        return None
    key = path.read_text(encoding='utf-8').strip()
    path.unlink()
    return key or None


def memory_floor(method: str) -> int:
    return FINETUNE_MEMORY if method == 'finetune' else ADAPTER_MEMORY


def disk_size(method: str) -> int:
    return FINETUNE_DISK if method == 'finetune' else ADAPTER_DISK


def gpu_memory(gpu: dict[str, Any]) -> int:
    try:
        return int(gpu.get('memory') or 0)
    except (TypeError, ValueError):
        return 0


def community_price(gpu: dict[str, Any]) -> float:
    price = gpu.get('price') or {}
    if not isinstance(price, dict):
        return float('inf')
    value = price.get('community')
    if value is None:
        return float('inf')
    try:
        return float(value)
    except (TypeError, ValueError):
        return float('inf')


def is_usable(gpu: dict[str, Any], method: str) -> bool:
    availability = str(gpu.get('availability') or 'NONE').upper()
    if availability not in USABLE_AVAILABILITY:
        return False
    if not gpu.get('id'):
        return False
    return gpu_memory(gpu) >= memory_floor(method)


def rank_gpus(gpus: list[dict[str, Any]], method: str) -> list[dict[str, Any]]:
    usable = [gpu for gpu in gpus if is_usable(gpu, method)]
    return sorted(usable, key=lambda gpu: (community_price(gpu), gpu_memory(gpu)))


def pod_name(job: dict[str, Any]) -> str:
    job_id = str(job.get('id') or 'job')
    prefix = job_id.split('-')[0] or job_id[:8]
    return f'zone-train-{prefix}'


def pod_args() -> str:
    blob = base64.b64encode(BOOTSTRAP.encode('utf-8')).decode('ascii')
    return "python3 -u -c exec(__import__('base64').b64decode('" + blob + "'))"


def offers_cloud(gpu: dict[str, Any], cloud: str) -> bool:
    key = 'community' if cloud == 'COMMUNITY' else 'secure'
    value = gpu.get(key)
    return True if value is None else bool(value)


def pod_body(job: dict[str, Any], gpu: dict[str, Any], cloud: str) -> dict[str, Any]:
    method = str(job.get('method') or 'lora')
    return {
        'name': pod_name(job),
        'image': POD_IMAGE,
        'gpu': {'id': gpu['id'], 'count': 1},
        'cloud': cloud,
        'disk': disk_size(method),
        'ports': [f'{POD_PORT}/http'],
        'env': {'PYTHONUNBUFFERED': '1'},
        'args': pod_args(),
    }


def place_pod(
    client: Client,
    job: dict[str, Any],
    gpus: list[dict[str, Any]],
) -> dict[str, Any]:
    method = str(job.get('method') or 'lora')
    candidates = rank_gpus(gpus, method)
    if not candidates:
        raise RuntimeError('no Runpod GPU meets the memory floor')
    last = 'no Runpod GPU could be placed'
    for cloud in CLOUDS:
        for gpu in candidates:
            if not offers_cloud(gpu, cloud):
                continue
            try:
                return client.create_pod(pod_body(job, gpu, cloud))
            except HttpError as error:
                last = str(error)
                if error.status == 402:
                    raise RuntimeError(last) from error
                if error.status in {400, 403}:
                    continue
                raise
    raise RuntimeError(last)


def wait_running(
    client: Client,
    pod_id: str,
    sleep: Callable[[float], None] = time.sleep,
) -> dict[str, Any]:
    while True:
        pod = client.get_pod(pod_id)
        status = str(pod.get('status') or '')
        if status == 'RUNNING':
            return pod
        if status in {'EXITED', 'ERROR', 'TERMINATED'}:
            raise RuntimeError(f'Runpod pod {pod_id} is {status}')
        sleep(POLL_INTERVAL)


def proxy_url(pod_id: str) -> str:
    return f'https://{pod_id}-{POD_PORT}.proxy.runpod.net'


def open_url(
    url: str,
    method: str = 'GET',
    data: bytes | None = None,
    headers: dict[str, str] | None = None,
    timeout: int = 60,
) -> bytes:
    call = urllib.request.Request(
        url, data=data, headers=headers or {}, method=method
    )
    try:
        with urllib.request.urlopen(call, timeout=timeout) as response:
            return response.read()
    except urllib.error.HTTPError as error:
        raw = error.read()
        raise HttpError(error.code, http_detail(raw)) from error


def read_json(url: str, timeout: int = 30) -> dict[str, Any]:
    payload = json.loads(open_url(url, timeout=timeout).decode() or '{}')
    return payload if isinstance(payload, dict) else {}


def wait_ready(
    base: str,
    sleep: Callable[[float], None] = time.sleep,
    attempts: int = 60,
) -> dict[str, Any]:
    last = 'Runpod pod HTTP is not reachable'
    for _ in range(max(int(attempts), 1)):
        try:
            return read_json(f'{base}/status')
        except (HttpError, OSError, ValueError) as error:
            last = str(error)
            sleep(POLL_INTERVAL)
    raise RuntimeError(last)


def remote_job(job: dict[str, Any]) -> dict[str, Any]:
    payload = dict(job)
    payload.pop('provider', None)
    payload.pop('pod_id', None)
    payload['status'] = 'queued'
    payload['pid'] = None
    payload['error'] = None
    return payload


def add_bytes(archive: tarfile.TarFile, name: str, data: bytes) -> None:
    info = tarfile.TarInfo(name)
    info.size = len(data)
    archive.addfile(info, io.BytesIO(data))


def pack_upload(
    models_dir: Path,
    job_dir: Path,
    job: dict[str, Any],
    config: dict[str, Any],
) -> bytes:
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode='w:gz') as archive:
        add_bytes(
            archive,
            'job.json',
            (json.dumps(remote_job(job), indent=2) + '\n').encode(),
        )
        add_bytes(
            archive,
            'train_sdxl_config.json',
            (json.dumps(config, indent=2) + '\n').encode(),
        )
        for name in SCRIPTS:
            path = ROOT / name
            if path.is_file():
                archive.add(path, arcname=name)
        dataset = Path(job_dir) / 'dataset'
        if dataset.is_dir():
            archive.add(dataset, arcname='dataset')
        clips = Path(job_dir) / 'clips'
        if clips.is_dir():
            archive.add(clips, arcname='clips')
        cache = train_sdxl.class_cache_path(models_dir, config)
        for png in train_sdxl.list_class_pngs(cache):
            archive.add(png, arcname=f'class/{png.name}')
            caption = png.with_suffix('.txt')
            if caption.is_file():
                archive.add(caption, arcname=f'class/{caption.name}')
        stem = Path(str(job.get('filename') or job.get('name') or 'person')).stem
        face = Path(models_dir) / 'loras' / f'{stem}.face.png'
        if face.is_file():
            archive.add(face, arcname=f'loras/{face.name}')
    return buffer.getvalue()


def upload_bundle(base: str, payload: bytes) -> None:
    open_url(
        f'{base}/',
        method='PUT',
        data=payload,
        headers={
            'Content-Type': 'application/gzip',
            'Content-Length': str(len(payload)),
        },
        timeout=600,
    )


def extract_tar(archive: tarfile.TarFile, dest: Path) -> None:
    dest.mkdir(parents=True, exist_ok=True)
    try:
        archive.extractall(dest, filter='data')
    except TypeError:
        archive.extractall(dest)


def poll_until_done(
    base: str,
    job_dir: Path,
    sleep: Callable[[float], None] = time.sleep,
) -> None:
    while True:
        status = read_json(f'{base}/status')
        try:
            progress = read_json(f'{base}/progress')
        except (HttpError, OSError, ValueError):
            progress = {}
        if progress:
            train_sdxl.write_json(Path(job_dir) / 'progress.json', progress, pretty=False)
        state = str(status.get('status') or '')
        if state == 'succeeded':
            return
        if state == 'failed':
            raise RuntimeError(str(status.get('error') or 'Runpod training failed'))
        sleep(POLL_INTERVAL)


def download_artifact(base: str, models_dir: Path) -> None:
    payload = open_url(f'{base}/artifact', timeout=600)
    with tarfile.open(fileobj=io.BytesIO(payload), mode='r:gz') as archive:
        extract_tar(archive, Path(models_dir))


def huggingface_url(repo: str, filename: str) -> str:
    return f'https://huggingface.co/{repo}/resolve/main/{filename}'


def fetch_file(url: str, dest: Path) -> None:
    dest = Path(dest)
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.is_file() and dest.stat().st_size > 0:
        return
    temporary = dest.with_name(dest.name + '.tmp')
    call = urllib.request.Request(url, headers={'User-Agent': 'zone-train'})
    with urllib.request.urlopen(call, timeout=600) as response:
        with temporary.open('wb') as handle:
            while True:
                chunk = response.read(1024 * 1024)
                if not chunk:
                    break
                handle.write(chunk)
    os.replace(temporary, dest)


def download_base(job: dict[str, Any], models_dir: Path) -> None:
    method = str(job.get('method') or 'lora')
    if method == 'video':
        import train_wan

        repo = str(job.get('hf_base') or train_wan.DEFAULT_HF_BASE)
        files = (
            (
                Path(models_dir) / 'diffusion_models' / train_wan.WAN_TRANSFORMER,
                train_wan.WAN_TRANSFORMER,
            ),
            (Path(models_dir) / 'vae' / train_wan.WAN_VAE, train_wan.WAN_VAE),
        )
        for dest, filename in files:
            fetch_file(huggingface_url(repo, filename), dest)
        return
    filename = str(job.get('checkpoint') or train_sdxl.DEFAULT_CHECKPOINT)
    repo = str(job.get('hf_base') or train_sdxl.DEFAULT_HF_BASE)
    dest = Path(models_dir) / 'checkpoints' / filename
    fetch_file(huggingface_url(repo, filename), dest)


def ensure_packages() -> None:
    subprocess.run(
        [
            sys.executable,
            '-m',
            'pip',
            'install',
            '--disable-pip-version-check',
            *PACKAGES,
        ],
        check=True,
    )


def published_files(models_dir: Path, job: dict[str, Any]) -> list[Path]:
    models_dir = Path(models_dir)
    method = str(job.get('method') or 'lora')
    if method == 'video':
        import train_wan

        weight = train_wan.publish_path(models_dir, job)
    else:
        weight = train_sdxl.publish_path(models_dir, job)
    files = []
    for path in (weight, train_sdxl.sidecar_path(weight)):
        if path.is_file():
            files.append(path)
    if method == 'pivotal':
        embedding = train_sdxl.embedding_path(models_dir, job)
        if embedding.is_file():
            files.append(embedding)
    stem = Path(str(job.get('filename') or job.get('name') or '')).stem
    name = str(job.get('name') or stem)
    for candidate in dict.fromkeys([stem, name]):
        if not candidate:
            continue
        face = models_dir / 'loras' / f'{candidate}.face.png'
        if face.is_file():
            files.append(face)
    return files


def write_artifact(root: Path, models_dir: Path, job: dict[str, Any]) -> None:
    models_dir = Path(models_dir).resolve()
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode='w:gz') as archive:
        for path in published_files(models_dir, job):
            archive.add(path, arcname=str(path.resolve().relative_to(models_dir)))
    (Path(root) / 'artifact.tar.gz').write_bytes(buffer.getvalue())


def run_remote(root: Path) -> None:
    root = Path(root)
    job = json.loads((root / 'job.json').read_text(encoding='utf-8'))
    if not isinstance(job, dict):
        raise RuntimeError('job.json is not an object')
    job_id = str(job.get('id') or 'job')
    models = root / 'models'
    job_dir = models / train_sdxl.TRAIN_ROOT / job_id
    job_dir.mkdir(parents=True, exist_ok=True)
    train_sdxl.write_job(job_dir, remote_job(job))
    dataset = root / 'dataset'
    if dataset.is_dir():
        shutil.copytree(dataset, job_dir / 'dataset', dirs_exist_ok=True)
    clips = root / 'clips'
    if clips.is_dir():
        shutil.copytree(clips, job_dir / 'clips', dirs_exist_ok=True)
    class_src = root / 'class'
    if class_src.is_dir():
        dest = train_sdxl.class_cache_path(models, {'class_cache_dir': '.zone-class/person'})
        dest.mkdir(parents=True, exist_ok=True)
        for entry in class_src.iterdir():
            if entry.is_file():
                shutil.copy2(entry, dest / entry.name)
    loras = root / 'loras'
    if loras.is_dir():
        (models / 'loras').mkdir(parents=True, exist_ok=True)
        for entry in loras.iterdir():
            if entry.is_file():
                shutil.copy2(entry, models / 'loras' / entry.name)
    download_base(job, models)
    ensure_packages()
    script = root / (
        'train_wan.py' if str(job.get('method') or '') == 'video' else 'train_sdxl.py'
    )
    subprocess.run(
        [sys.executable, str(script), '--once', '--models-dir', str(models)],
        check=True,
        cwd=str(root),
    )
    write_artifact(root, models, job)


def normalize(job: dict[str, Any], job_dir: Path) -> dict[str, Any]:
    if str(job.get('method') or '') == 'video':
        import train_wan

        return train_wan.normalize_job(job, job_dir)
    return train_sdxl.normalize_job(job, job_dir)


def resume_pod(client: Client, job: dict[str, Any]) -> dict[str, Any] | None:
    pod_id = str(job.get('pod_id') or '')
    if not pod_id:
        return None
    try:
        pod = client.get_pod(pod_id)
    except HttpError:
        return None
    if str(pod.get('status') or '') == 'RUNNING':
        return pod
    return None


def process_job(
    models_dir: Path,
    job_dir: Path,
    config: dict[str, Any],
    stub: bool = False,
    *,
    client: Client | None = None,
    sleep: Callable[[float], None] = time.sleep,
    proxy_base: str | None = None,
) -> None:
    job = train_sdxl.load_job(job_dir)
    pod_id = str(job.get('pod_id') or '') or None
    active = client
    try:
        key = consume_key_file(job_dir)
        if active is None:
            if not key:
                raise RuntimeError('Runpod API key is missing')
            active = Client(key)
        job = normalize(job, job_dir)
        job = train_sdxl.mark_running(job_dir, job)
        pod = resume_pod(active, job)
        if pod is None:
            pod = place_pod(active, job, active.list_gpus())
            pod_id = str(pod['id'])
            job['pod_id'] = pod_id
            train_sdxl.write_job(job_dir, job)
            wait_running(active, pod_id, sleep)
        else:
            pod_id = str(pod.get('id') or pod_id)
        base = proxy_base or proxy_url(str(pod_id))
        status = wait_ready(base, sleep)
        if str(status.get('status') or 'waiting') == 'waiting':
            upload_bundle(base, pack_upload(models_dir, job_dir, job, config))
        poll_until_done(base, job_dir, sleep)
        download_artifact(base, Path(models_dir))
        train_sdxl.mark_succeeded(job_dir, job)
    except Exception as error:
        train_sdxl.mark_failed(job_dir, job, error)
        raise
    finally:
        if pod_id and active is not None:
            try:
                active.delete_pod(pod_id)
            except Exception:
                pass
