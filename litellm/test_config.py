"""Regression checks for Ollama chat routing; run with python3 -m unittest discover -s litellm."""

import pathlib
import unittest


class RoutingTest(unittest.TestCase):
    def test_chat_uses_native_adapter(self) -> None:
        template = pathlib.Path(__file__).with_name('config.yaml.template').read_text()
        for model in ['FAST', 'REASON', 'VISION']:
            self.assertIn('model: ollama_chat/{{OLLAMA_MODEL_' + model + '}}', template)
        self.assertIn('model: ollama/{{OLLAMA_MODEL_EMBED}}', template)

    def test_helm_chat_uses_native_adapter(self) -> None:
        values = pathlib.Path(__file__).parents[1] / 'helm/zone-ai/values.yaml'
        self.assertIn('name: "llama3.1:8b"\n      provider: ollama_chat', values.read_text())


if __name__ == '__main__':
    unittest.main()
