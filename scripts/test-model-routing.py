"""Check custom model routing with Helm and a running LiteLLM container.

Run with python3 scripts/test-model-routing.py [container-name].
Only resolves routes; does not send inference requests or change the container.
"""

import json
import pathlib
import subprocess
import sys


ROOT = pathlib.Path(__file__).resolve().parents[1]
CONFIGURATION = {
    'OLLAMA_MODEL_FAST': 'llama3.1:8b',
    'OLLAMA_MODEL_REASON': 'deepseek-r1:14b',
    'OLLAMA_MODEL_EMBED': 'nomic-embed-text',
    'OLLAMA_MODEL_VISION': 'llava:7b',
    'OLLAMA_BASE_URL': 'http://configured-ollama:11434',
}
CHECK = '''
import json
import sys
import unittest

import yaml
from litellm import Router

configurations = json.load(sys.stdin)


class RoutingTest(unittest.TestCase):
    def test_routes(self) -> None:
        for name, content in configurations.items():
            configuration = yaml.safe_load(content)
            if name == 'helm':
                configuration = yaml.safe_load(configuration['data']['config.yaml'])
            router = Router(model_list=configuration['model_list'])
            base = ('http://ollama:11434' if name == 'helm'
                    else 'http://configured-ollama:11434')
            for model in [
                'qwen3.8:27b',
                'organization/custom-model:v2.1',
                'hf.co/owner/repository:Q4_K_M',
            ]:
                with self.subTest(configuration=name, model=model):
                    parameters = router.get_available_deployment(
                        model=model
                    )['litellm_params']
                    self.assertEqual(parameters['model'], 'ollama_chat/' + model)
                    self.assertEqual(parameters['api_base'], base)
                    self.assertEqual(parameters['timeout'], 180)
            for deployment in configuration['model_list']:
                model = deployment['model_name']
                if model == '*':
                    continue
                with self.subTest(configuration=name, alias=model):
                    parameters = router.get_available_deployment(
                        model=model
                    )['litellm_params']
                    expected = deployment['litellm_params']
                    self.assertEqual(parameters['model'], expected['model'])
                    self.assertEqual(parameters['api_base'], base)
                    self.assertEqual(parameters['timeout'], expected['timeout'])
            with self.subTest(configuration=name, adapter='embedding'):
                parameters = router.get_available_deployment(
                    model='nomic-embed-text'
                )['litellm_params']
                self.assertEqual(parameters['model'], 'ollama/nomic-embed-text')
                self.assertEqual(parameters['timeout'], 30)


unittest.main()
'''


def main() -> None:
    template = (ROOT / 'litellm/config.yaml.template').read_text()
    for name, value in CONFIGURATION.items():
        template = template.replace('{{' + name + '}}', value)
    helm = subprocess.run(
        [
            'helm', 'template', 'zone', str(ROOT / 'helm/zone-ai'),
            '--show-only', 'templates/litellm/configmap.yaml',
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    container = sys.argv[1] if len(sys.argv) > 1 else 'litellm'
    result = subprocess.run(
        ['docker', 'exec', '-i', container, 'python', '-c', CHECK],
        input=json.dumps({'docker': template, 'helm': helm}),
        text=True,
    )
    sys.exit(result.returncode)


if __name__ == '__main__':
    main()
