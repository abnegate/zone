from __future__ import annotations

import json
import os
import sys
import unittest
from contextlib import contextmanager
from pathlib import Path

COMFYUI = Path(__file__).parents[1]
if str(COMFYUI) not in sys.path:
    sys.path.insert(0, str(COMFYUI))

import probe_lora  # noqa: E402

VARIABLES = (
    'ZONE_COMFY_INPUT',
    'COMFYUI_MODELS_DIR',
    'ZONE_PROBE_FOLDER',
    'ZONE_PROBE_RANK',
    'ZONE_PROBE_RESOLUTION',
)


@contextmanager
def environment(**values: str):
    previous = {name: os.environ.get(name) for name in VARIABLES}
    for name in VARIABLES:
        os.environ.pop(name, None)
    os.environ.update(values)
    try:
        yield
    finally:
        for name in VARIABLES:
            os.environ.pop(name, None)
            if previous[name] is not None:
                os.environ[name] = previous[name]


def config() -> dict:
    return json.loads(
        (COMFYUI / 'custom_nodes' / 'zone_lora' / 'train_config.json').read_text()
    )


class GradientGraphTests(unittest.TestCase):
    def test_rank_defaults_to_the_rank_training_uses(self):
        """A probe of a rank other than the trained one describes a different adapter."""
        with environment():
            graph = probe_lora.gradient_graph('folder', {}, 512)
        self.assertEqual(graph['4']['inputs']['rank'], int(config()['rank']))

    def test_rank_override_still_wins(self):
        with environment(ZONE_PROBE_RANK='4'):
            graph = probe_lora.gradient_graph('folder', {}, 512)
        self.assertEqual(graph['4']['inputs']['rank'], 4)


class DatasetTests(unittest.TestCase):
    def test_a_missing_probe_folder_is_refused_rather_than_silently_empty(self):
        """Path('') is '.', so an unset input directory used to glob the cwd."""
        with environment(ZONE_PROBE_FOLDER='zone-probe-nothing-here', COMFYUI_MODELS_DIR='/srv/comfy/models'):
            with self.assertRaises(SystemExit) as raised:
                probe_lora.dataset()
        self.assertIn('zone-probe-nothing-here', str(raised.exception))

    def test_the_probe_folder_is_read_from_comfyui_input(self):
        with environment(ZONE_PROBE_FOLDER='sample', ZONE_COMFY_INPUT=str(COMFYUI / 'tests')):
            with self.assertRaises(SystemExit) as raised:
                probe_lora.dataset()
        self.assertIn(str(COMFYUI / 'tests' / 'sample'), str(raised.exception))


if __name__ == '__main__':
    unittest.main()
