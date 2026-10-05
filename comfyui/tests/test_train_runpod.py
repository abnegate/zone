from __future__ import annotations

import json
import sys
import tarfile
import tempfile
import threading
import unittest
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from io import BytesIO
from pathlib import Path
from unittest import mock

COMFYUI = Path(__file__).parents[1]
if str(COMFYUI) not in sys.path:
    sys.path.insert(0, str(COMFYUI))

import train_runpod  # noqa: E402
import train_sdxl  # noqa: E402

RTX_4090 = {
    'id': 'NVIDIA GeForce RTX 4090',
    'name': 'RTX 4090',
    'memory': 24,
    'community': True,
    'secure': True,
    'price': {'community': 0.31, 'secure': 0.44},
    'availability': 'HIGH',
}
A40 = {
    'id': 'NVIDIA A40',
    'name': 'A40',
    'memory': 48,
    'community': True,
    'secure': True,
    'price': {'community': 0.35, 'secure': 0.49},
    'availability': 'HIGH',
}
A100_NONE = {
    'id': 'NVIDIA A100 80GB PCIe',
    'name': 'A100',
    'memory': 80,
    'community': True,
    'secure': True,
    'price': {'community': 1.19, 'secure': 1.59},
    'availability': 'NONE',
}
CATALOG = [RTX_4090, A40, A100_NONE]


def queued_job(method: str = 'lora', extra: dict | None = None) -> dict:
    job = {
        'schema_version': 1,
        'id': str(uuid.uuid4()),
        'name': 'jerry',
        'method': method,
        'trigger': 'ohwx',
        'checkpoint': 'lustifySDXLNSFW_ggwpV7.safetensors',
        'status': 'queued',
        'error': None,
        'provider': 'runpod',
    }
    if extra:
        job.update(extra)
    return job


def write_dataset(job_dir: Path, count: int = 1) -> None:
    dataset = job_dir / 'dataset'
    dataset.mkdir(parents=True)
    for index in range(count):
        (dataset / f'{index:04}.png').write_bytes(b'png')
        (dataset / f'{index:04}.txt').write_text('ohwx person', encoding='utf-8')


def stage_job(models: Path, method: str = 'lora', extra: dict | None = None) -> Path:
    job_dir = models / '.zone-train' / str(uuid.uuid4())
    job_dir.mkdir(parents=True)
    train_sdxl.write_job(job_dir, queued_job(method=method, extra=extra))
    write_dataset(job_dir)
    (job_dir / 'runpod.key').write_text('rp-test-key\n', encoding='utf-8')
    return job_dir


class CatalogPickTests(unittest.TestCase):
    def test_finetune_picks_a40_and_rejects_4090(self) -> None:
        ranked = train_runpod.rank_gpus(CATALOG, 'finetune')
        self.assertEqual([gpu['id'] for gpu in ranked], [A40['id']])
        self.assertNotIn(RTX_4090['id'], [gpu['id'] for gpu in ranked])
        self.assertFalse(train_runpod.is_usable(RTX_4090, 'finetune'))
        self.assertEqual(train_runpod.memory_floor('finetune'), 48)

    def test_lora_allows_4090_as_cheapest(self) -> None:
        ranked = train_runpod.rank_gpus(CATALOG, 'lora')
        self.assertEqual(ranked[0]['id'], RTX_4090['id'])
        self.assertTrue(train_runpod.is_usable(RTX_4090, 'lora'))
        ranked_video = train_runpod.rank_gpus(CATALOG, 'video')
        self.assertEqual(ranked_video[0]['id'], RTX_4090['id'])

    def test_none_availability_is_skipped(self) -> None:
        self.assertFalse(train_runpod.is_usable(A100_NONE, 'finetune'))
        self.assertNotIn(A100_NONE['id'], [gpu['id'] for gpu in train_runpod.rank_gpus(CATALOG, 'lora')])

    def test_finetune_disk_is_larger(self) -> None:
        self.assertEqual(train_runpod.disk_size('finetune'), 150)
        self.assertEqual(train_runpod.disk_size('lora'), 40)
        self.assertEqual(train_runpod.disk_size('pivotal'), 40)
        self.assertEqual(train_runpod.disk_size('video'), 40)


class CreateAndKeyTests(unittest.TestCase):
    def test_create_tries_community_then_secure(self) -> None:
        clouds: list[str] = []

        def create(body: dict) -> dict:
            clouds.append(body['cloud'])
            self.assertEqual(body['image'], train_runpod.POD_IMAGE)
            self.assertEqual(body['ports'], ['8888/http'])
            self.assertEqual(body['gpu'], {'id': RTX_4090['id'], 'count': 1})
            self.assertEqual(body['disk'], 40)
            self.assertTrue(str(body['name']).startswith('zone-train-'))
            if body['cloud'] == 'COMMUNITY':
                raise train_runpod.HttpError(400, 'no capacity')
            return {'id': 'pod1', 'status': 'PROVISIONING'}

        client = mock.Mock()
        client.create_pod.side_effect = create
        pod = train_runpod.place_pod(client, queued_job(), [RTX_4090])
        self.assertEqual(clouds, ['COMMUNITY', 'SECURE'])
        self.assertEqual(pod['id'], 'pod1')

    def test_finetune_create_uses_a40_and_150gb(self) -> None:
        bodies: list[dict] = []

        def create(body: dict) -> dict:
            bodies.append(body)
            return {'id': 'pod-a40', 'status': 'PROVISIONING'}

        client = mock.Mock()
        client.create_pod.side_effect = create
        train_runpod.place_pod(client, queued_job(method='finetune'), CATALOG)
        self.assertEqual(len(bodies), 1)
        self.assertEqual(bodies[0]['gpu']['id'], A40['id'])
        self.assertEqual(bodies[0]['disk'], 150)
        self.assertEqual(bodies[0]['cloud'], 'COMMUNITY')

    def test_key_file_is_unlinked(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            job_dir = Path(directory)
            path = job_dir / 'runpod.key'
            path.write_text('rp-secret\n', encoding='utf-8')
            self.assertEqual(train_runpod.load_key(job_dir), 'rp-secret')
            self.assertFalse(path.exists())

    def test_delete_called_on_train_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = stage_job(models)
            client = mock.Mock()
            client.list_gpus.return_value = [A40]
            client.create_pod.return_value = {'id': 'pod9', 'status': 'RUNNING'}
            client.get_pod.return_value = {'id': 'pod9', 'status': 'RUNNING'}
            with mock.patch.object(
                train_runpod, 'wait_ready', side_effect=RuntimeError('boom')
            ):
                with self.assertRaisesRegex(RuntimeError, 'boom'):
                    train_runpod.process_job(
                        models,
                        job_dir,
                        train_sdxl.load_config(),
                        client=client,
                        sleep=lambda _interval: None,
                    )
            client.delete_pod.assert_called_once_with('pod9')
            self.assertFalse((job_dir / 'runpod.key').exists())
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'failed')
            self.assertEqual(job['error'], 'boom')
            self.assertNotIn('rp-test-key', json.dumps(job))

    def test_process_job_unlinks_key_before_create(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = stage_job(models)
            client = mock.Mock()
            client.list_gpus.side_effect = RuntimeError('catalog down')
            with self.assertRaisesRegex(RuntimeError, 'catalog down'):
                train_runpod.process_job(
                    models,
                    job_dir,
                    train_sdxl.load_config(),
                    client=client,
                    sleep=lambda _interval: None,
                )
            self.assertFalse((job_dir / 'runpod.key').exists())

    def test_resume_skips_create_when_pod_is_running(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = stage_job(models, extra={'pod_id': 'keep-me'})
            client = mock.Mock()
            client.get_pod.return_value = {'id': 'keep-me', 'status': 'RUNNING'}
            with (
                mock.patch.object(
                    train_runpod, 'wait_ready', return_value={'status': 'running'}
                ),
                mock.patch.object(train_runpod, 'poll_until_done'),
                mock.patch.object(train_runpod, 'download_artifact'),
            ):
                train_runpod.process_job(
                    models,
                    job_dir,
                    train_sdxl.load_config(),
                    client=client,
                    sleep=lambda _interval: None,
                    proxy_base='http://127.0.0.1:9',
                )
            client.create_pod.assert_not_called()
            client.list_gpus.assert_not_called()
            client.delete_pod.assert_called_once_with('keep-me')


class HttpClientTests(unittest.TestCase):
    def serve(self, handler: type[BaseHTTPRequestHandler]) -> str:
        server = ThreadingHTTPServer(('127.0.0.1', 0), handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        return f'http://127.0.0.1:{server.server_port}'

    def test_client_lists_gpus_with_bearer_token(self) -> None:
        seen: dict[str, str] = {}

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                seen['authorization'] = self.headers.get('Authorization') or ''
                seen['path'] = self.path
                payload = json.dumps({'gpus': CATALOG}).encode()
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def log_message(self, format: str, *args: object) -> None:
                return

        base = self.serve(Handler)
        client = train_runpod.Client('rp-live', base=base)
        gpus = client.list_gpus()
        self.assertEqual(gpus[0]['id'], RTX_4090['id'])
        self.assertEqual(seen['authorization'], 'Bearer rp-live')
        self.assertIn('include=AVAILABILITY', seen['path'])
        self.assertIn('product=POD', seen['path'])

    def test_create_posts_nested_body_and_delete_is_recorded(self) -> None:
        creates: list[dict] = []
        deletes: list[str] = []

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self) -> None:
                length = int(self.headers.get('Content-Length') or 0)
                body = json.loads(self.rfile.read(length).decode())
                creates.append(body)
                payload = json.dumps({'id': 'podhttp', 'status': 'PROVISIONING'}).encode()
                self.send_response(201)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def do_DELETE(self) -> None:
                deletes.append(self.path)
                self.send_response(204)
                self.end_headers()

            def log_message(self, format: str, *args: object) -> None:
                return

        base = self.serve(Handler)
        client = train_runpod.Client('rp-live', base=base)
        created = client.create_pod(train_runpod.pod_body(queued_job(), A40, 'COMMUNITY'))
        self.assertEqual(created['id'], 'podhttp')
        self.assertEqual(creates[0]['cloud'], 'COMMUNITY')
        self.assertEqual(creates[0]['gpu'], {'id': A40['id'], 'count': 1})
        client.delete_pod('podhttp')
        self.assertEqual(deletes, ['/v2/pods/podhttp'])

    def test_process_job_uploads_polls_and_downloads_artifact(self) -> None:
        uploaded: list[int] = []

        class Api(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                if self.path.startswith('/v2/catalog/gpus'):
                    payload = json.dumps({'gpus': [A40]}).encode()
                else:
                    payload = json.dumps({'id': 'podrun', 'status': 'RUNNING'}).encode()
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def do_POST(self) -> None:
                length = int(self.headers.get('Content-Length') or 0)
                self.rfile.read(length)
                payload = json.dumps({'id': 'podrun', 'status': 'PROVISIONING'}).encode()
                self.send_response(201)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def do_DELETE(self) -> None:
                self.send_response(204)
                self.end_headers()

            def log_message(self, format: str, *args: object) -> None:
                return

        class Pod(BaseHTTPRequestHandler):
            status = 'waiting'

            def do_GET(self) -> None:
                path = self.path.split('?', 1)[0]
                if path == '/status':
                    payload = json.dumps({'status': Pod.status, 'error': None}).encode()
                    ctype = 'application/json'
                elif path == '/progress':
                    payload = json.dumps(
                        {
                            'step': 500,
                            'total': 500,
                            'phase': 'publishing',
                            'percent': 100,
                        }
                    ).encode()
                    ctype = 'application/json'
                elif path == '/artifact':
                    buffer = BytesIO()
                    with tarfile.open(fileobj=buffer, mode='w:gz') as archive:
                        data = b'weight'
                        info = tarfile.TarInfo('loras/jerry.safetensors')
                        info.size = len(data)
                        archive.addfile(info, BytesIO(data))
                        sidecar = b'{"recipe_id":"sdxl-adapter"}\n'
                        info = tarfile.TarInfo('loras/jerry.safetensors.zone.json')
                        info.size = len(sidecar)
                        archive.addfile(info, BytesIO(sidecar))
                        face = b'face'
                        info = tarfile.TarInfo('loras/jerry.face.png')
                        info.size = len(face)
                        archive.addfile(info, BytesIO(face))
                    payload = buffer.getvalue()
                    ctype = 'application/gzip'
                else:
                    self.send_response(404)
                    self.end_headers()
                    return
                self.send_response(200)
                self.send_header('Content-Type', ctype)
                self.send_header('Content-Length', str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def do_PUT(self) -> None:
                length = int(self.headers.get('Content-Length') or 0)
                uploaded.append(length)
                self.rfile.read(length)
                Pod.status = 'succeeded'
                payload = b'{"ok":true}'
                self.send_response(202)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            do_POST = do_PUT

            def log_message(self, format: str, *args: object) -> None:
                return

        api = self.serve(Api)
        pod = self.serve(Pod)
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            cache = models / '.zone-class' / 'person'
            cache.mkdir(parents=True)
            (cache / '0000.png').write_bytes(b'png')
            (cache / '0000.txt').write_text('a photo of a person', encoding='utf-8')
            job_dir = stage_job(models)
            client = train_runpod.Client('rp-live', base=api)
            train_runpod.process_job(
                models,
                job_dir,
                train_sdxl.load_config(),
                client=client,
                sleep=lambda _interval: None,
                proxy_base=pod,
            )
            self.assertTrue(uploaded and uploaded[0] > 0)
            self.assertFalse((job_dir / 'runpod.key').exists())
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'succeeded')
            self.assertNotIn('rp-live', json.dumps(job))
            progress = json.loads((job_dir / 'progress.json').read_text(encoding='utf-8'))
            self.assertEqual(progress['percent'], 100)
            self.assertEqual((models / 'loras' / 'jerry.safetensors').read_bytes(), b'weight')
            self.assertTrue((models / 'loras' / 'jerry.safetensors.zone.json').is_file())
            self.assertEqual((models / 'loras' / 'jerry.face.png').read_bytes(), b'face')

    def test_bootstrap_is_valid_python(self) -> None:
        compile(train_runpod.BOOTSTRAP, '<bootstrap>', 'exec')
        args = train_runpod.pod_args()
        self.assertTrue(args.startswith('python3 -u -c exec('))
        self.assertNotIn(' ', args.split('exec(', 1)[1])


class PackTests(unittest.TestCase):
    def test_pack_skips_host_checkpoint_and_strips_provider(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = stage_job(models)
            (models / 'checkpoints').mkdir()
            (models / 'checkpoints' / 'lustifySDXLNSFW_ggwpV7.safetensors').write_bytes(
                b'huge'
            )
            cache = models / '.zone-class' / 'person'
            cache.mkdir(parents=True)
            (cache / '0000.png').write_bytes(b'png')
            job = train_sdxl.load_job(job_dir)
            payload = train_runpod.pack_upload(
                models, job_dir, job, {'class_cache_dir': '.zone-class/person'}
            )
            names = []
            with tarfile.open(fileobj=BytesIO(payload), mode='r:gz') as archive:
                names = archive.getnames()
                remote = json.loads(archive.extractfile('job.json').read().decode())
            self.assertIn('job.json', names)
            self.assertIn('dataset/0000.png', names)
            self.assertIn('class/0000.png', names)
            self.assertIn('train_sdxl.py', names)
            self.assertIn('train_runpod.py', names)
            self.assertNotIn('provider', remote)
            self.assertEqual(remote['status'], 'queued')
            self.assertTrue(all('lustify' not in name for name in names))
            self.assertTrue(all('ggwp' not in name.lower() for name in names))


if __name__ == '__main__':
    unittest.main()
