#!/usr/bin/env python3
"""Check retired application storage with Helm 4 and PyYAML."""

import json
import subprocess
import unittest
from http.server import BaseHTTPRequestHandler
from http.server import ThreadingHTTPServer
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Thread
from urllib.parse import urlsplit

import yaml


class StorageTests(unittest.TestCase):
    chart = Path(__file__).resolve().parents[1] / 'helm' / 'zone-apps'
    claim = {
        'apiVersion': 'v1',
        'kind': 'PersistentVolumeClaim',
        'metadata': {
            'name': 'openwebui-pvc',
            'namespace': 'zone',
            'labels': {'owner': 'original'},
            'annotations': {'example.com/backup': 'daily'},
            'finalizers': ['kubernetes.io/pvc-protection'],
        },
        'spec': {
            'accessModes': ['ReadWriteOnce'],
            'storageClassName': 'retained-storage',
            'volumeName': 'existing-volume',
            'volumeMode': 'Filesystem',
            'resources': {'requests': {'storage': '17Gi'}},
        },
    }

    def render(self, *arguments: str) -> list[dict]:
        result = subprocess.run(
            ['helm', 'template', 'zone-apps', str(self.chart), '--namespace', 'zone',
             *arguments],
            capture_output=True, text=True, timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        documents = list(filter(None, yaml.safe_load_all(result.stdout)))
        for document in documents:
            if document['metadata']['name'].startswith('openwebui'):
                self.assertEqual(document['kind'], 'PersistentVolumeClaim')
        return documents

    def retained(self, documents: list[dict]) -> dict:
        claims = [
            document for document in documents
            if document['metadata']['name'] == 'openwebui-pvc'
        ]
        self.assertEqual(len(claims), 1)
        claim = claims[0]
        self.assertEqual(claim['metadata']['annotations']['helm.sh/resource-policy'], 'keep')
        self.assertEqual(claim['metadata']['labels'], self.claim['metadata']['labels'])
        self.assertEqual(claim['metadata']['annotations']['example.com/backup'], 'daily')
        self.assertEqual(claim['metadata']['finalizers'], self.claim['metadata']['finalizers'])
        self.assertEqual(claim['spec'], self.claim['spec'])
        return claim

    def test_fresh_install_has_no_retired_application(self) -> None:
        documents = self.render()
        self.assertFalse(any(
            document['metadata']['name'].startswith('openwebui')
            for document in documents
        ))

    def test_legacy_values_do_not_restore_workloads(self) -> None:
        documents = self.render('--is-upgrade', '--set', 'openwebui.enabled=true')
        self.assertFalse(any(
            document['metadata']['name'].startswith('openwebui')
            for document in documents
        ))

    def test_offline_retention_requires_existing_spec(self) -> None:
        result = subprocess.run(
            ['helm', 'template', 'zone-apps', str(self.chart), '--is-upgrade',
             '--set', 'retainedStorage.openwebui.enabled=true'],
            capture_output=True, text=True, timeout=30,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('must contain the existing claim spec', result.stderr)

    def test_offline_retention_preserves_existing_claim(self) -> None:
        with TemporaryDirectory() as directory:
            values = Path(directory) / 'values.json'
            values.write_text(json.dumps({'retainedStorage': {'openwebui': {
                'enabled': True,
                'metadata': self.claim['metadata'],
                'spec': self.claim['spec'],
            }}}))
            self.retained(self.render('--is-upgrade', '--values', str(values)))

    def test_upgrade_lookup_preserves_existing_claim(self) -> None:
        claim = self.claim
        paths = []

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                path = urlsplit(self.path).path
                paths.append(path)
                responses = {
                    '/version': {'major': '1', 'minor': '35', 'gitVersion': 'v1.35.0'},
                    '/api': {'kind': 'APIVersions', 'apiVersion': 'v1', 'versions': ['v1']},
                    '/apis': {'kind': 'APIGroupList', 'apiVersion': 'v1', 'groups': []},
                    '/api/v1': {
                        'kind': 'APIResourceList', 'apiVersion': 'v1', 'groupVersion': 'v1',
                        'resources': [{'name': 'persistentvolumeclaims', 'singularName': '',
                                       'namespaced': True, 'kind': 'PersistentVolumeClaim',
                                       'verbs': ['get', 'list']}],
                    },
                    '/api/v1/namespaces/zone/persistentvolumeclaims/openwebui-pvc': claim,
                }
                groups = {
                    'apps/v1': [('deployments', 'Deployment')],
                    'autoscaling/v2': [('horizontalpodautoscalers', 'HorizontalPodAutoscaler')],
                    'networking.k8s.io/v1': [('ingresses', 'Ingress')],
                    'policy/v1': [('poddisruptionbudgets', 'PodDisruptionBudget')],
                }
                for version, resources in groups.items():
                    group = version.split('/')[0]
                    descriptor = {'groupVersion': version, 'version': version.split('/')[1]}
                    responses['/apis']['groups'].append({
                        'name': group, 'versions': [descriptor], 'preferredVersion': descriptor,
                    })
                    responses[f'/apis/{version}'] = {
                        'kind': 'APIResourceList', 'apiVersion': 'v1', 'groupVersion': version,
                        'resources': [{'name': name, 'kind': kind, 'namespaced': True,
                                       'singularName': '', 'verbs': ['get', 'list']}
                                      for name, kind in resources],
                    }
                for name, kind in [('configmaps', 'ConfigMap'), ('services', 'Service'),
                                   ('serviceaccounts', 'ServiceAccount')]:
                    responses['/api/v1']['resources'].append({
                        'name': name, 'kind': kind, 'namespaced': True,
                        'singularName': '', 'verbs': ['get', 'list'],
                    })
                response = responses.get(path)
                self.send_response(200 if response else 404)
                self.send_header('Content-Type', 'application/json')
                self.end_headers()
                self.wfile.write(json.dumps(response or {'kind': 'Status', 'code': 404}).encode())

            def log_message(self, format: str, *arguments: object) -> None:
                pass

        with ThreadingHTTPServer(('127.0.0.1', 0), Handler) as server:
            thread = Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                with TemporaryDirectory() as directory:
                    configuration = Path(directory) / 'config.json'
                    configuration.write_text(json.dumps({
                        'apiVersion': 'v1', 'kind': 'Config',
                        'clusters': [{'name': 'test', 'cluster': {
                            'server': f'http://127.0.0.1:{server.server_port}'}}],
                        'contexts': [{'name': 'test', 'context': {'cluster': 'test'}}],
                        'current-context': 'test',
                    }))
                    self.retained(self.render(
                        '--is-upgrade', '--dry-run=server', '--disable-openapi-validation',
                        '--kubeconfig', str(configuration),
                        '--set', 'retainedStorage.openwebui.spec.resources.requests.storage=2Gi',
                    ))
                    self.assertIn('/api/v1/namespaces/zone/persistentvolumeclaims/openwebui-pvc', paths)
            finally:
                server.shutdown()
                thread.join()


if __name__ == '__main__':
    unittest.main()
