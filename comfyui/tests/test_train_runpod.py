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

CLAIM = 'claimed'
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
        'base': 'sdxl-people',
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


def download_weight(_base: str, _models: Path, job_dir: Path, _job: dict) -> None:
    (job_dir / 'jerry.safetensors').write_bytes(b'weight')


def legacy_artifact(members: dict[str, bytes]) -> bytes:
    buffer = BytesIO()
    with tarfile.open(fileobj=buffer, mode='w:gz') as archive:
        for name, data in members.items():
            add_member(archive, name, data)
    return buffer.getvalue()


def legacy_link(name: str, target: str, kind: bytes = tarfile.SYMTYPE) -> bytes:
    buffer = BytesIO()
    with tarfile.open(fileobj=buffer, mode='w:gz') as archive:
        add_member(archive, 'loras/jerry.safetensors', b'weight')
        info = tarfile.TarInfo(name)
        info.type = kind
        info.linkname = target
        archive.addfile(info)
    return buffer.getvalue()


def old_format_job(pod_id: str) -> dict:
    return {
        'schema_version': 1,
        'id': str(uuid.uuid4()),
        'name': 'jerry',
        'method': 'lora',
        'subject': 'person',
        'trigger': 'ohwx',
        'checkpoint': 'lustifySDXLNSFW_ggwpV7.safetensors',
        'filename': 'jerry.safetensors',
        'status': 'running',
        'error': None,
        'pid': None,
        'provider': 'runpod',
        'pod_id': pod_id,
    }


def without_filter(extractall):
    def extract(self, path='.', members=None, *, numeric_owner=False, **options):
        if 'filter' in options:
            raise TypeError("extractall() got an unexpected keyword argument 'filter'")
        return extractall(self, path, members, numeric_owner=numeric_owner)

    return extract


def add_member(archive: tarfile.TarFile, name: str, data: bytes) -> None:
    info = tarfile.TarInfo(name)
    info.size = len(data)
    archive.addfile(info, BytesIO(data))


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
        self.assertEqual(train_runpod.disk_size('lora', 'other'), 80)
        self.assertEqual(train_runpod.disk_size('lora', 'language'), 80)
        self.assertEqual(train_runpod.disk_size('finetune', 'language'), 150)
        self.assertEqual(train_runpod.memory_floor('lora'), 24)
        self.assertEqual(train_runpod.memory_floor('finetune'), 48)


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
        pod, gpu = train_runpod.place_pod(client, queued_job(), [RTX_4090])
        self.assertEqual(clouds, ['COMMUNITY', 'SECURE'])
        self.assertEqual(pod['id'], 'pod1')
        self.assertEqual(gpu['id'], RTX_4090['id'])

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
            self.assertTrue(path.exists())
            train_runpod.drop_key(job_dir)
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

    def test_process_job_keeps_the_key_until_the_job_ends(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = stage_job(models)
            key = job_dir / 'runpod.key'
            client = mock.Mock()
            client.list_gpus.return_value = [A40]
            client.create_pod.return_value = {'id': 'pod9', 'status': 'RUNNING'}
            client.get_pod.return_value = {'id': 'pod9', 'status': 'RUNNING'}

            def wait_ready(*_args: object, **_kwargs: object) -> dict:
                self.assertTrue(key.exists())
                raise RuntimeError('boom')

            with mock.patch.object(train_runpod, 'wait_ready', side_effect=wait_ready):
                with self.assertRaisesRegex(RuntimeError, 'boom'):
                    train_runpod.process_job(
                        models,
                        job_dir,
                        train_sdxl.load_config(),
                        client=client,
                        sleep=lambda _interval: None,
                    )
            self.assertFalse(key.exists())

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
                mock.patch.object(train_runpod, 'download_artifact', side_effect=download_weight),
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

    def test_an_artifact_without_the_weight_fails_the_job(self) -> None:
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
                with self.assertRaisesRegex(RuntimeError, 'no trained weight'):
                    train_runpod.process_job(
                        models,
                        job_dir,
                        train_sdxl.load_config(),
                        client=client,
                        sleep=lambda _interval: None,
                        proxy_base='http://127.0.0.1:9',
                    )
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'failed')
            self.assertEqual(job['error'], 'the Runpod artifact holds no trained weight')
            client.delete_pod.assert_called_once_with('keep-me')


class ClaimTests(unittest.TestCase):
    def test_the_claim_is_taken_before_the_job_is_marked_running(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = stage_job(models, extra={'pod_id': 'keep-me'})
            client = mock.Mock()
            client.get_pod.return_value = {'id': 'keep-me', 'status': 'RUNNING'}
            claimed_when_running: list[bool] = []
            mark_running = train_sdxl.mark_running

            def spy(path: Path, job: dict) -> dict:
                claimed_when_running.append((Path(path) / CLAIM).is_file())
                return mark_running(path, job)

            with (
                mock.patch.object(train_sdxl, 'mark_running', side_effect=spy),
                mock.patch.object(
                    train_runpod, 'wait_ready', return_value={'status': 'running'}
                ),
                mock.patch.object(train_runpod, 'poll_until_done'),
                mock.patch.object(train_runpod, 'download_artifact', side_effect=download_weight),
            ):
                train_runpod.process_job(
                    models,
                    job_dir,
                    train_sdxl.load_config(),
                    client=client,
                    sleep=lambda _interval: None,
                    proxy_base='http://127.0.0.1:9',
                )
            self.assertEqual(claimed_when_running, [True])
            self.assertEqual(train_sdxl.load_job(job_dir)['status'], 'succeeded')

    def test_a_job_claimed_elsewhere_is_left_alone(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = stage_job(models)
            (job_dir / CLAIM).touch()
            before = (job_dir / 'job.json').read_bytes()
            client = mock.Mock()
            train_runpod.process_job(
                models,
                job_dir,
                train_sdxl.load_config(),
                client=client,
                sleep=lambda _interval: None,
            )
            self.assertEqual(client.method_calls, [])
            self.assertEqual((job_dir / 'job.json').read_bytes(), before)
            self.assertTrue(
                (job_dir / 'runpod.key').is_file(),
                'the key belongs to whoever holds the claim',
            )


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
                        add_member(archive, 'job/jerry.safetensors', b'weight')
                        add_member(archive, 'models/loras/jerry.face.png', b'face')
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
            self.assertEqual(job['gpu'], 'A40')
            self.assertNotIn('rp-live', json.dumps(job))
            progress = json.loads((job_dir / 'progress.json').read_text(encoding='utf-8'))
            self.assertEqual(progress['percent'], 100)
            self.assertEqual((job_dir / 'jerry.safetensors').read_bytes(), b'weight')
            self.assertFalse((models / 'loras' / 'jerry.safetensors').exists())
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
                remote_config = json.loads(
                    archive.extractfile('train_sdxl_config.json').read().decode()
                )
            self.assertIn('job.json', names)
            self.assertIn('dataset/0000.png', names)
            self.assertIn('class/0000.png', names)
            self.assertIn('train_sdxl.py', names)
            self.assertIn('train_runpod.py', names)
            self.assertIn('train_llm.py', names)
            self.assertIn('train_llm_config.json', names)
            self.assertIn('train_flux.py', names)
            self.assertIn('train_flux_config.json', names)
            self.assertNotIn('provider', remote)
            self.assertEqual(remote['status'], 'queued')
            self.assertEqual(
                remote_config,
                {'class_cache_dir': '.zone-class/person', 'free_space_margin_gib': 1},
            )
            self.assertTrue(all('lustify' not in name for name in names))
            self.assertTrue(all('ggwp' not in name.lower() for name in names))


class RemoteScriptTests(unittest.TestCase):
    def test_remote_script_picks_language_other_video_person(self) -> None:
        self.assertEqual(
            train_runpod.remote_script({'subject': 'language', 'method': 'lora'}),
            'train_llm.py',
        )
        self.assertEqual(
            train_runpod.remote_script({'subject': 'other', 'method': 'lora'}),
            'train_flux.py',
        )
        self.assertEqual(
            train_runpod.remote_script({'method': 'video'}),
            'train_wan.py',
        )
        self.assertEqual(
            train_runpod.remote_script({'method': 'lora'}),
            'train_sdxl.py',
        )
        self.assertEqual(
            train_runpod.remote_script(
                {'subject': 'language', 'method': 'video'}
            ),
            'train_llm.py',
        )


class DownloadBaseTests(unittest.TestCase):
    def test_language_skips_weight_fetch(self) -> None:
        with mock.patch.object(train_runpod, 'fetch_file') as fetch:
            train_runpod.download_base(
                {'subject': 'language', 'checkpoint': 'qwen2.5:7b', 'method': 'lora'},
                Path('/tmp'),
            )
        fetch.assert_not_called()

    def test_qwen_other_skips_weight_fetch(self) -> None:
        with mock.patch.object(train_runpod, 'fetch_file') as fetch:
            train_runpod.download_base(
                {
                    'subject': 'other',
                    'recipe_id': 'qwen-image-edit-adapter',
                    'hf_base': 'Qwen/Qwen-Image-Edit-2511',
                    'method': 'lora',
                },
                Path('/tmp'),
            )
        fetch.assert_not_called()

    def test_flux_fetches_comfy_org_fp8(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            with mock.patch.object(train_runpod, 'fetch_file') as fetch:
                train_runpod.download_base(
                    {
                        'subject': 'other',
                        'recipe_id': 'flux-dev-adapter',
                        'checkpoint': 'flux1-dev-fp8.safetensors',
                        'method': 'lora',
                    },
                    models,
                )
            fetch.assert_called_once()
            url, dest = fetch.call_args.args
            self.assertIn('Comfy-Org/flux1-dev', url)
            self.assertIn('flux1-dev-fp8.safetensors', url)
            self.assertEqual(dest, models / 'checkpoints' / 'flux1-dev-fp8.safetensors')


class ArtifactTests(unittest.TestCase):
    def test_the_artifact_brings_the_weight_back_into_the_local_job_directory(self) -> None:
        with (
            tempfile.TemporaryDirectory() as remote_directory,
            tempfile.TemporaryDirectory() as local_directory,
        ):
            remote = Path(remote_directory)
            remote_models = remote / 'models'
            remote_job = remote_models / '.zone-train' / 'remote-job'
            remote_job.mkdir(parents=True)
            job = train_sdxl.normalize_job(queued_job(method='pivotal'), remote_job)
            (remote_job / 'jerry.safetensors').write_bytes(b'weight')
            (remote_models / 'embeddings').mkdir()
            (remote_models / 'embeddings' / 'jerry.safetensors').write_bytes(b'embedding')
            (remote_models / 'loras').mkdir()
            (remote_models / 'loras' / 'jerry.face.png').write_bytes(b'face')
            train_runpod.write_artifact(remote, remote_models, remote_job, job)

            local_models = Path(local_directory)
            local_job = local_models / '.zone-train' / 'local-job'
            local_job.mkdir(parents=True)
            artifact = (remote / 'artifact.tar.gz').read_bytes()
            with mock.patch.object(train_runpod, 'open_url', return_value=artifact):
                train_runpod.download_artifact('http://pod', local_models, local_job, job)

            self.assertEqual((local_job / 'jerry.safetensors').read_bytes(), b'weight')
            self.assertEqual(
                (local_models / 'embeddings' / 'jerry.safetensors').read_bytes(),
                b'embedding',
            )
            self.assertEqual((local_models / 'loras' / 'jerry.face.png').read_bytes(), b'face')
            self.assertFalse((local_models / 'loras' / 'jerry.safetensors').exists())
            self.assertFalse((local_models / '.zone-train' / 'remote-job').exists())


class LegacyArtifactTests(unittest.TestCase):
    def download(self, models: Path, job_dir: Path, job: dict, artifact: bytes) -> None:
        with mock.patch.object(train_runpod, 'open_url', return_value=artifact):
            train_runpod.download_artifact('http://pod', models, job_dir, job)

    def staged_job(self, models: Path, job: dict) -> tuple[Path, dict]:
        job_dir = models / '.zone-train' / 'local-job'
        job_dir.mkdir(parents=True)
        return job_dir, train_sdxl.normalize_job(job, job_dir)

    def test_an_old_layout_lora_artifact_lands_its_weight_in_the_job_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir, job = self.staged_job(models, queued_job(method='pivotal'))
            (models / 'embeddings').mkdir()
            (models / 'embeddings' / 'jerry.safetensors').write_bytes(b'stale embedding')
            self.download(
                models,
                job_dir,
                job,
                legacy_artifact(
                    {
                        'loras/jerry.safetensors': b'weight',
                        'loras/jerry.safetensors.zone.json': b'{"trigger": "ohwx"}',
                        'embeddings/jerry.safetensors': b'embedding',
                        'loras/jerry.face.png': b'face',
                    }
                ),
            )

            self.assertEqual((job_dir / 'jerry.safetensors').read_bytes(), b'weight')
            self.assertEqual(
                (models / 'embeddings' / 'jerry.safetensors').read_bytes(), b'embedding'
            )
            self.assertEqual((models / 'loras' / 'jerry.face.png').read_bytes(), b'face')
            self.assertFalse((models / 'loras' / 'jerry.safetensors').exists())
            self.assertFalse((models / 'loras' / 'jerry.safetensors.zone.json').exists())
            self.assertEqual(sorted(path.name for path in job_dir.iterdir()), ['jerry.safetensors'])

    def test_an_old_layout_fine_tune_artifact_lands_its_checkpoint_in_the_job_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir, job = self.staged_job(models, queued_job(method='finetune'))
            self.download(
                models,
                job_dir,
                job,
                legacy_artifact(
                    {
                        'checkpoints/jerry.safetensors': b'checkpoint',
                        'checkpoints/jerry.safetensors.zone.json': b'{}',
                    }
                ),
            )

            self.assertEqual((job_dir / 'jerry.safetensors').read_bytes(), b'checkpoint')
            self.assertFalse((models / 'checkpoints').exists())

    def test_an_old_layout_language_artifact_lands_in_the_language_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = models / '.zone-train' / 'local-job'
            job_dir.mkdir(parents=True)
            job = {'subject': 'language', 'filename': 'jerry', 'name': 'jerry', 'method': 'lora'}
            self.download(
                models,
                job_dir,
                job,
                legacy_artifact(
                    {
                        'llm/jerry/Modelfile': b'FROM /workspace/models/llm/jerry\n',
                        'llm/jerry/adapter_model.safetensors': b'adapter',
                    }
                ),
            )

            self.assertEqual(
                (models / 'llm' / 'jerry' / 'adapter_model.safetensors').read_bytes(), b'adapter'
            )
            self.assertTrue((models / 'llm' / 'jerry' / 'Modelfile').is_file())
            self.assertEqual(list(job_dir.iterdir()), [])

    def test_an_old_layout_artifact_keeps_files_the_job_does_not_publish(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir, job = self.staged_job(models, queued_job())
            (models / 'loras').mkdir()
            (models / 'loras' / 'tom.safetensors').write_bytes(b'mine')
            self.download(
                models,
                job_dir,
                job,
                legacy_artifact(
                    {
                        'loras/jerry.safetensors': b'weight',
                        'loras/tom.safetensors': b'theirs',
                    }
                ),
            )

            self.assertEqual((models / 'loras' / 'tom.safetensors').read_bytes(), b'mine')
            self.assertEqual((job_dir / 'jerry.safetensors').read_bytes(), b'weight')

    def test_an_unsafe_old_layout_member_is_refused(self) -> None:
        artifacts = {
            'parent': legacy_artifact(
                {'loras/jerry.safetensors': b'weight', '../escape.safetensors': b'x'}
            ),
            'absolute': legacy_artifact(
                {'loras/jerry.safetensors': b'weight', '/tmp/escape.safetensors': b'x'}
            ),
            'symlink': legacy_link('loras/link.safetensors', '../../outside'),
            'hardlink': legacy_link(
                'loras/link.safetensors', 'loras/jerry.safetensors', tarfile.LNKTYPE
            ),
        }
        for label, artifact in artifacts.items():
            with self.subTest(member=label), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                models = root / 'models'
                job_dir, job = self.staged_job(models, queued_job())
                with self.assertRaisesRegex(RuntimeError, 'unsafe'):
                    self.download(models, job_dir, job, artifact)
                self.assertFalse((job_dir / 'jerry.safetensors').exists())
                self.assertEqual(sorted(path.name for path in root.iterdir()), ['models'])
                self.assertEqual(list(job_dir.iterdir()), [])

    def resume_old_pod(self, models: Path, method: str, members: dict[str, bytes]) -> Path:
        job_dir = models / '.zone-train' / str(uuid.uuid4())
        job_dir.mkdir(parents=True)
        job = old_format_job('keep-me')
        job['method'] = method
        train_sdxl.write_job(job_dir, job)
        write_dataset(job_dir)
        (job_dir / 'runpod.key').write_text('rp-test-key\n', encoding='utf-8')
        client = mock.Mock()
        client.get_pod.return_value = {'id': 'keep-me', 'status': 'RUNNING'}
        with (
            mock.patch.object(train_runpod, 'wait_ready', return_value={'status': 'running'}),
            mock.patch.object(train_runpod, 'poll_until_done'),
            mock.patch.object(
                train_runpod, 'open_url', return_value=legacy_artifact(members)
            ),
        ):
            train_runpod.process_job(
                models,
                job_dir,
                train_sdxl.load_config(),
                client=client,
                sleep=lambda _interval: None,
                proxy_base='http://127.0.0.1:9',
            )
        job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
        self.assertEqual(job['status'], 'succeeded', job.get('error'))
        client.delete_pod.assert_called_once_with('keep-me')
        return job_dir

    def test_a_pod_resumed_across_the_upgrade_publishes_its_old_layout_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            sidecar = {
                'recipe_id': 'sdxl-adapter',
                'hf_base': train_sdxl.DEFAULT_HF_BASE,
                'trigger': 'ohwx',
            }
            job_dir = self.resume_old_pod(
                models,
                'lora',
                {
                    'loras/jerry.safetensors': b'weight',
                    'loras/jerry.safetensors.zone.json': json.dumps(sidecar).encode(),
                    'loras/jerry.face.png': b'face',
                },
            )
            self.assertEqual((models / 'loras' / 'jerry.safetensors').read_bytes(), b'weight')
            published = json.loads(
                (models / 'loras' / 'jerry.safetensors.zone.json').read_text(encoding='utf-8')
            )
            published.pop('generation')
            self.assertEqual(published, sidecar)
            self.assertEqual((models / 'loras' / 'jerry.face.png').read_bytes(), b'face')
            self.assertFalse((job_dir / 'jerry.safetensors').exists())

    def test_a_pod_resumed_across_the_upgrade_leaves_a_name_that_trains_again(self) -> None:
        for method, folder, recipe in (
            ('lora', 'loras', 'sdxl-adapter'),
            ('finetune', 'checkpoints', 'sdxl'),
        ):
            with self.subTest(method=method), tempfile.TemporaryDirectory() as directory:
                models = Path(directory)
                sidecar = {
                    'recipe_id': recipe,
                    'hf_base': train_sdxl.DEFAULT_HF_BASE,
                    'trigger': 'ohwx',
                }
                self.resume_old_pod(
                    models,
                    method,
                    {
                        f'{folder}/jerry.safetensors': b'weight',
                        f'{folder}/jerry.safetensors.zone.json': json.dumps(sidecar).encode(),
                    },
                )
                published = json.loads(
                    (models / folder / 'jerry.safetensors.zone.json').read_text(encoding='utf-8')
                )
                self.assertIn(
                    'generation',
                    published,
                    'abnegate-comfy refuses to replace a weight whose sidecar has no generation',
                )
                uuid.UUID(published['generation'])

    def test_a_resumed_pod_stamps_only_its_own_sidecar(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            other = json.dumps({'recipe_id': 'sdxl-adapter', 'trigger': 'sks'}).encode()
            self.resume_old_pod(
                models,
                'lora',
                {
                    'loras/jerry.safetensors': b'weight',
                    'loras/jerry.safetensors.zone.json': json.dumps(
                        {'recipe_id': 'sdxl-adapter', 'trigger': 'ohwx'}
                    ).encode(),
                    'loras/tom.safetensors.zone.json': other,
                },
            )
            self.assertEqual(
                (models / 'loras' / 'tom.safetensors.zone.json').read_bytes(), other
            )

    def test_a_reinstalled_trainer_resumes_a_pod_whose_job_has_a_base(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = stage_job(models, extra={'pod_id': 'keep-me'})
            client = mock.Mock()
            client.get_pod.return_value = {'id': 'keep-me', 'status': 'RUNNING'}
            artifact = legacy_artifact(
                {
                    'loras/jerry.safetensors': b'weight',
                    'loras/jerry.safetensors.zone.json': b'{}',
                }
            )
            with (
                mock.patch.object(
                    train_runpod, 'wait_ready', return_value={'status': 'running'}
                ),
                mock.patch.object(train_runpod, 'poll_until_done'),
                mock.patch.object(train_runpod, 'open_url', return_value=artifact),
            ):
                train_runpod.process_job(
                    models,
                    job_dir,
                    train_sdxl.load_config(),
                    client=client,
                    sleep=lambda _interval: None,
                    proxy_base='http://127.0.0.1:9',
                )
            job = json.loads((job_dir / 'job.json').read_text(encoding='utf-8'))
            self.assertEqual(job['status'], 'succeeded')
            self.assertEqual((job_dir / 'jerry.safetensors').read_bytes(), b'weight')
            self.assertFalse((models / 'loras' / 'jerry.safetensors.zone.json').exists())


class LegacyRemoteTests(unittest.TestCase):
    def test_a_job_queued_before_the_upgrade_comes_back_published_from_a_new_pod(self) -> None:
        with (
            tempfile.TemporaryDirectory() as remote_directory,
            tempfile.TemporaryDirectory() as local_directory,
        ):
            remote = Path(remote_directory)
            remote_models = remote / 'models'
            remote_job = remote_models / '.zone-train' / 'remote-job'
            remote_job.mkdir(parents=True)
            job = old_format_job('keep-me')
            del job['pod_id']
            train_sdxl.write_job(remote_job, train_runpod.remote_job(job))
            write_dataset(remote_job)
            train_sdxl.process_job(remote_models, remote_job, train_sdxl.load_config(), stub=True)
            train_runpod.write_artifact(
                remote, remote_models, remote_job, train_sdxl.load_job(remote_job)
            )

            local_models = Path(local_directory)
            local_job = local_models / '.zone-train' / 'local-job'
            local_job.mkdir(parents=True)
            artifact = (remote / 'artifact.tar.gz').read_bytes()
            with mock.patch.object(train_runpod, 'open_url', return_value=artifact):
                train_runpod.download_artifact('http://pod', local_models, local_job, job)

            self.assertTrue((local_models / 'loras' / 'jerry.safetensors').is_file())
            sidecar = json.loads(
                (local_models / 'loras' / 'jerry.safetensors.zone.json').read_text(
                    encoding='utf-8'
                )
            )
            self.assertEqual(sidecar['recipe_id'], 'sdxl-adapter')
            self.assertEqual(sidecar['trigger'], 'ohwx')
            uuid.UUID(sidecar['generation'])
            self.assertEqual(list(local_job.iterdir()), [])


class UnsafeArtifactTests(unittest.TestCase):
    def artifact(self, members: dict[str, bytes], link: str | None = None) -> bytes:
        buffer = BytesIO()
        with tarfile.open(fileobj=buffer, mode='w:gz') as archive:
            add_member(archive, 'job/jerry.safetensors', b'weight')
            for name, data in members.items():
                add_member(archive, name, data)
            if link is not None:
                info = tarfile.TarInfo(link)
                info.type = tarfile.SYMTYPE
                info.linkname = '../../../outside'
                archive.addfile(info)
        return buffer.getvalue()

    def test_an_unsafe_new_layout_member_is_refused_without_the_extraction_filter(self) -> None:
        artifacts = {
            'job parent': self.artifact({'job/../../../escape.safetensors': b'x'}),
            'models parent': self.artifact({'models/../escape.safetensors': b'x'}),
            'models symlink': self.artifact({}, link='models/loras/link.safetensors'),
        }
        for label, artifact in artifacts.items():
            with self.subTest(member=label), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                models = root / 'models'
                job_dir = models / '.zone-train' / 'local-job'
                job_dir.mkdir(parents=True)
                job = train_sdxl.normalize_job(queued_job(), job_dir)
                with (
                    mock.patch.object(
                        tarfile.TarFile,
                        'extractall',
                        without_filter(tarfile.TarFile.extractall),
                    ),
                    mock.patch.object(train_runpod, 'open_url', return_value=artifact),
                    self.assertRaisesRegex(RuntimeError, 'unsafe'),
                ):
                    train_runpod.download_artifact('http://pod', models, job_dir, job)
                self.assertEqual(sorted(path.name for path in root.iterdir()), ['models'])
                self.assertEqual(list(job_dir.iterdir()), [])
                self.assertFalse((models / 'loras').exists())


class PublishedFilesTests(unittest.TestCase):
    def test_language_packs_llm_adapter_dir(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            dest = models / 'llm' / 'jerry'
            dest.mkdir(parents=True)
            (dest / 'Modelfile').write_text('FROM /workspace/models/llm/jerry\n', encoding='utf-8')
            (dest / 'adapter_config.json').write_text('{}\n', encoding='utf-8')
            (dest / 'adapter_model.safetensors').write_bytes(b'w')
            files = train_runpod.published_files(
                models,
                {
                    'subject': 'language',
                    'filename': 'jerry',
                    'name': 'jerry',
                    'method': 'lora',
                },
            )
            names = {path.name for path in files}
            self.assertIn('Modelfile', names)
            self.assertIn('adapter_config.json', names)
            self.assertIn('adapter_model.safetensors', names)


class LanguageImportTests(unittest.TestCase):
    def test_process_job_imports_published_language_adapter(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            models = Path(directory)
            job_dir = stage_job(
                models,
                extra={
                    'subject': 'language',
                    'checkpoint': 'qwen2.5:7b',
                    'pod_id': 'keep-me',
                    'base': '',
                },
            )
            client = mock.Mock()
            client.get_pod.return_value = {'id': 'keep-me', 'status': 'RUNNING'}
            with (
                mock.patch.object(
                    train_runpod, 'wait_ready', return_value={'status': 'running'}
                ),
                mock.patch.object(train_runpod, 'poll_until_done'),
                mock.patch.object(train_runpod, 'download_artifact'),
                mock.patch('train_llm.import_published') as imported,
            ):
                train_runpod.process_job(
                    models,
                    job_dir,
                    train_sdxl.load_config(),
                    client=client,
                    sleep=lambda _interval: None,
                    proxy_base='http://127.0.0.1:9',
                )
            imported.assert_called_once_with(models, mock.ANY)
            client.delete_pod.assert_called_once_with('keep-me')


if __name__ == '__main__':
    unittest.main()
