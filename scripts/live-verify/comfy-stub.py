#!/usr/bin/env python3
"""A ComfyUI stand-in that speaks the real /prompt, /history, /view protocol.

It runs no models. It reads the submitted graph, decides which lane it is
(image, video, audio, image upscale, video upscale), and answers with a real
media file attributed to the node the collector expects. That exercises every
line of the server's ComfyUI path -- submit, poll, collect, fetch, store,
announce -- without the weights.
"""

import json
import os
import sys
import threading
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

FIXTURES = os.environ.get('ZONE_COMFY_FIXTURES', '')

LANES = {
    'image': ('image.png', 'images', 'temp', None),
    'video': ('video.webm', 'videos', 'temp', None),
    'audio': ('audio.flac', 'audio', 'temp', None),
    'upscale_image': ('upscaled.png', 'images', 'temp', '4'),
    'upscale_video': ('upscaled.webm', 'videos', 'temp', '5'),
}

MIMES = {
    '.png': 'image/png',
    '.webm': 'video/webm',
    '.flac': 'audio/flac',
    '.mp4': 'video/mp4',
}

prompts = {}
uploads = []
cancels = []
lock = threading.Lock()


def lane_of(graph):
    classes = {
        str(node.get('class_type')) for node in graph.values() if isinstance(node, dict)
    }
    if 'ImageUpscaleWithModel' in classes:
        return 'upscale_video' if 'LoadVideo' in classes else 'upscale_image'
    if 'PreviewAudio' in classes or 'SaveAudio' in classes:
        return 'audio'
    if 'SaveWEBM' in classes or 'SaveVideo' in classes:
        return 'video'
    return 'image'


def output_node(graph, lane):
    _, _, _, pinned = LANES[lane]
    if pinned:
        return pinned
    # Sweeping lanes accept any node; answer on the graph's own sink so the
    # shape matches what ComfyUI would return.
    sinks = [
        key
        for key, node in graph.items()
        if isinstance(node, dict)
        and str(node.get('class_type')).startswith(('Preview', 'Save'))
    ]
    return sinks[0] if sinks else next(iter(graph), '1')


class Handler(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, fmt, *args):
        sys.stderr.write('[comfy-stub] %s\n' % (fmt % args))

    def _json(self, status, payload):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _body(self):
        length = int(self.headers.get('Content-Length') or 0)
        if not length:
            return {}
        raw = self.rfile.read(length)
        try:
            return json.loads(raw)
        except ValueError:
            return {'_raw': raw}

    def do_GET(self):
        parsed = urlparse(self.path)
        if parsed.path.startswith('/history/'):
            prompt_id = parsed.path.rsplit('/', 1)[-1]
            with lock:
                entry = prompts.get(prompt_id)
            if not entry:
                return self._json(200, {})
            filename, key, kind, _ = LANES[entry['lane']]
            return self._json(
                200,
                {
                    prompt_id: {
                        'status': {'status_str': 'success', 'completed': True},
                        'outputs': {
                            entry['node']: {
                                key: [
                                    {
                                        'filename': filename,
                                        'subfolder': '',
                                        'type': kind,
                                    }
                                ]
                            }
                        },
                    }
                },
            )
        if parsed.path == '/view':
            query = parse_qs(parsed.query)
            filename = (query.get('filename') or [''])[0]
            path = os.path.join(FIXTURES, os.path.basename(filename))
            if not os.path.isfile(path):
                return self._json(404, {'error': 'no such file'})
            with open(path, 'rb') as handle:
                data = handle.read()
            mime = MIMES.get(os.path.splitext(path)[1], 'application/octet-stream')
            self.send_response(200)
            self.send_header('Content-Type', mime)
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return None
        if parsed.path == '/_stub/state':
            with lock:
                return self._json(
                    200,
                    {
                        'prompts': [
                            {'id': key, 'lane': value['lane']}
                            for key, value in prompts.items()
                        ],
                        'uploads': list(uploads),
                        'cancelled': list(cancels),
                    },
                )
        if parsed.path in ('/system_stats', '/object_info', '/queue'):
            return self._json(200, {})
        return self._json(404, {'error': 'not found'})

    def do_POST(self):
        parsed = urlparse(self.path)
        if parsed.path == '/prompt':
            body = self._body()
            graph = body.get('prompt') or {}
            lane = lane_of(graph)
            prompt_id = str(uuid.uuid4())
            with lock:
                prompts[prompt_id] = {'lane': lane, 'node': output_node(graph, lane)}
            self.log_message('queued %s as %s', prompt_id, lane)
            return self._json(200, {'prompt_id': prompt_id, 'number': len(prompts)})
        if parsed.path.startswith('/upload/'):
            length = int(self.headers.get('Content-Length') or 0)
            raw = self.rfile.read(length) if length else b''
            name = 'upload.png'
            marker = b'filename="'
            if marker in raw:
                start = raw.index(marker) + len(marker)
                name = raw[start : raw.index(b'"', start)].decode('utf-8', 'replace')
            with lock:
                uploads.append(name)
            self.log_message('accepted upload %s (%d bytes)', name, length)
            return self._json(200, {'name': name, 'subfolder': '', 'type': 'input'})
        if parsed.path == '/queue':
            body = self._body()
            with lock:
                cancels.extend(body.get('delete') or [])
            return self._json(200, {})
        if parsed.path == '/history':
            body = self._body()
            with lock:
                for prompt_id in body.get('delete') or []:
                    prompts.pop(prompt_id, None)
            return self._json(200, {})
        if parsed.path == '/_stub/reset':
            with lock:
                prompts.clear()
                uploads.clear()
                cancels.clear()
            return self._json(200, {})
        return self._json(404, {'error': 'not found'})


if __name__ == '__main__':
    if not os.path.isdir(FIXTURES):
        sys.exit('ZONE_COMFY_FIXTURES must point at a directory of media fixtures')
    port = int(os.environ.get('COMFY_STUB_PORT', '8188'))
    ThreadingHTTPServer(('127.0.0.1', port), Handler).serve_forever()
