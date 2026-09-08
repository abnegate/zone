from __future__ import annotations

import importlib.util
import os
import unittest
from contextlib import contextmanager
from pathlib import Path

MODULE_PATH = Path(__file__).parents[1] / 'train_lora.py'
SERVER_PATH = Path(__file__).parents[2] / 'runner' / 'zone_comfy' / 'src' / 'train.rs'
SPEC = importlib.util.spec_from_file_location('zone_train_lora', MODULE_PATH)
assert SPEC and SPEC.loader
train_lora = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(train_lora)

VARIABLES = ('ZONE_COMFY_INPUT', 'COMFYUI_MODELS_DIR', 'ZONE_TRAIN_STEPS')
DATASETS = (8, 24, 100, 300)
HEALTHY_PASSES = (17.0, 19.0)
BUDGET_KEYS = frozenset({'passes_per_image', 'min_steps', 'max_steps'})


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


def steps_body(source: str) -> str:
    start = source.find('fn steps(')
    if start < 0:
        raise AssertionError(f'{SERVER_PATH}: fn steps not found')
    opened = source.index('{', start)
    depth = 0
    for index in range(opened, len(source)):
        if source[index] == '{':
            depth += 1
        elif source[index] == '}':
            depth -= 1
            if depth == 0:
                return source[opened : index + 1]
    raise AssertionError(f'{SERVER_PATH}: fn steps has no closing brace')


class TrainStepsTests(unittest.TestCase):
    def config(self, **overrides: object) -> dict:
        settings = train_lora.load_config()
        settings.update(overrides)
        return settings

    def passes(self, image_count: int, settings: dict) -> float:
        return train_lora.train_steps(image_count, settings) / image_count

    def test_a_larger_dataset_is_never_trained_less_per_image_than_a_smaller_one(self) -> None:
        with environment():
            settings = self.config()
            for smaller, larger in zip(DATASETS, DATASETS[1:]):
                with self.subTest(smaller=smaller, larger=larger):
                    self.assertGreaterEqual(
                        self.passes(larger, settings),
                        self.passes(smaller, settings),
                        f'{larger} images get {self.passes(larger, settings)} passes each and '
                        f'{smaller} images get {self.passes(smaller, settings)}, so uploading '
                        'more photos would train the subject less',
                    )

    def test_every_realistic_dataset_trains_inside_the_measured_band(self) -> None:
        with environment():
            settings = self.config()
            low, high = HEALTHY_PASSES
            for count in DATASETS:
                with self.subTest(images=count):
                    budget = self.passes(count, settings)
                    message = f'{count} images train {budget} passes each'
                    self.assertGreaterEqual(budget, low, message)
                    self.assertLessEqual(budget, high, message)

    def test_the_floor_keeps_a_tiny_dataset_training(self) -> None:
        with environment():
            settings = self.config()
            self.assertLessEqual(int(settings['min_steps']), int(settings['max_steps']))
            for count in (1, 2, 3):
                with self.subTest(images=count):
                    self.assertEqual(
                        train_lora.train_steps(count, settings),
                        int(settings['min_steps']),
                        f'{count} images must still train to the floor',
                    )

    def test_the_ceiling_holds_for_a_huge_dataset(self) -> None:
        with environment():
            settings = self.config()
            self.assertEqual(
                train_lora.train_steps(10_000, settings),
                int(settings['max_steps']),
                'a huge set is capped',
            )

    def test_the_server_spends_the_same_budget_on_the_same_keys(self) -> None:
        """train.rs reimplements this budget, and only a test keeps the two in step."""
        source = SERVER_PATH.read_text()
        self.assertIn('train_config.json', source, f'{SERVER_PATH} embeds a different config')
        self.assertEqual(
            BUDGET_KEYS,
            {name for name in BUDGET_KEYS if f'{name}: u32' in source},
            f'{SERVER_PATH} declares different budget fields',
        )
        body = ''.join(steps_body(source).split())
        self.assertIn('.saturating_mul(self.passes_per_image)', body, str(SERVER_PATH))
        self.assertIn('.clamp(self.min_steps,self.max_steps)', body, str(SERVER_PATH))
        self.assertLessEqual(BUDGET_KEYS, set(self.config()))

    def test_override_wins_over_the_budget(self) -> None:
        with environment(ZONE_TRAIN_STEPS='37'):
            self.assertEqual(train_lora.train_steps(8, self.config()), 37)


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
