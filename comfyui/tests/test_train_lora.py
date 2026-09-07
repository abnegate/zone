from __future__ import annotations

import importlib.util
import os
import unittest
from contextlib import contextmanager
from pathlib import Path

MODULE_PATH = Path(__file__).parents[1] / 'train_lora.py'
SPEC = importlib.util.spec_from_file_location('zone_train_lora', MODULE_PATH)
assert SPEC and SPEC.loader
train_lora = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(train_lora)

VARIABLES = ('ZONE_COMFY_INPUT', 'COMFYUI_MODELS_DIR')


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


class ComfyInputDirTests(unittest.TestCase):
    def test_override_wins(self):
        with environment(ZONE_COMFY_INPUT='/srv/comfy/input', COMFYUI_MODELS_DIR='/srv/comfy/models'):
            self.assertEqual(train_lora.comfy_input_dir(), Path('/srv/comfy/input'))

    def test_derived_from_models_dir(self):
        with environment(COMFYUI_MODELS_DIR='/srv/comfy/models'):
            self.assertEqual(train_lora.comfy_input_dir(), Path('/srv/comfy/input'))

    def test_never_falls_back_to_the_working_directory(self):
        """Path('') is '.', so a missing override used to stage images into the cwd."""
        with environment(COMFYUI_MODELS_DIR='/srv/comfy/models'):
            self.assertNotEqual(train_lora.comfy_input_dir(), Path('.'))

    def test_requires_a_location(self):
        with environment():
            with self.assertRaises(SystemExit):
                train_lora.comfy_input_dir()


if __name__ == '__main__':
    unittest.main()
