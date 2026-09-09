from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).parents[1] / 'custom_nodes' / 'zone_lora' / 'train_config.py'
SPEC = importlib.util.spec_from_file_location('zone_lora_train_config', MODULE_PATH)
assert SPEC and SPEC.loader
train_config = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(train_config)


class TrainConfigTests(unittest.TestCase):
    def test_identity_defaults_use_full_blocks_and_rank_alpha(self):
        config = train_config.load_config()
        self.assertTrue(config['alpha_equals_rank'])
        self.assertEqual(train_config.lora_alpha(8, config), 8.0)
        self.assertGreaterEqual(int(config['rank']), 32)
        self.assertGreaterEqual(
            int(config['min_steps']),
            150,
            'the shortest run measured to improve the subject was 150 steps',
        )
        self.assertEqual(int(config['resolution']), 512)
        self.assertEqual(config['lora_dtype'], 'bf16')

    def test_transformer_targets_cover_flux_and_qwen(self):
        self.assertTrue(
            train_config.is_transformer_block('diffusion_model.double_blocks.0.img_attn.qkv')
        )
        self.assertTrue(
            train_config.is_transformer_block('diffusion_model.single_blocks.37.linear1')
        )
        self.assertTrue(
            train_config.is_transformer_block('diffusion_model.transformer_blocks.12.attn.to_q')
        )
        self.assertFalse(train_config.is_transformer_block('diffusion_model.img_in'))
        self.assertTrue(train_config.is_output_module('diffusion_model.final_layer.linear'))

    def test_checkpoints_land_often_enough_to_judge_a_run_early(self):
        """The interval is derived now, so asserting the configured gap proves nothing."""
        config = train_config.load_config()
        shortest = int(config['min_steps'])
        interval = train_config.checkpoint_interval(shortest, config)
        self.assertGreater(interval, 0, 'a long run must be testable before it ends')
        self.assertLessEqual(
            interval,
            shortest // 2,
            'at least two checkpoints before the shortest run finishes',
        )

    def test_a_long_run_writes_a_bounded_number_of_intermediates(self):
        config = train_config.load_config()
        longest = int(config['max_steps'])
        interval = train_config.checkpoint_interval(longest, config)
        self.assertLessEqual(
            longest // interval,
            12,
            'an intermediate is hundreds of megabytes, so the count has to stay bounded',
        )

    def test_modulation_layers_are_left_alone_by_default(self):
        config = train_config.load_config()
        self.assertFalse(config['train_modulation'])
        for name in (
            'diffusion_model.double_blocks.0.img_mod.lin',
            'diffusion_model.double_blocks.0.txt_mod.lin',
            'diffusion_model.single_blocks.7.modulation.lin',
        ):
            self.assertTrue(train_config.is_modulation(name), name)
            self.assertFalse(train_config.trains(name, config), name)
        for name in (
            'diffusion_model.double_blocks.0.img_attn.qkv',
            'diffusion_model.double_blocks.0.img_mlp.0',
            'diffusion_model.single_blocks.7.linear1',
        ):
            self.assertTrue(train_config.trains(name, config), name)

    def test_modulation_can_be_opted_into(self):
        config = dict(train_config.load_config(), train_modulation=True)
        self.assertTrue(
            train_config.trains('diffusion_model.double_blocks.0.img_mod.lin', config)
        )

    def test_non_transformer_modules_never_train(self):
        config = train_config.load_config()
        for name in ('diffusion_model.img_in', 'diffusion_model.final_layer.linear'):
            self.assertFalse(train_config.trains(name, config), name)

    @unittest.skipUnless(sys.platform == 'darwin', 'MPS watermarks are only applied on macOS')
    def test_mps_low_watermark_never_exceeds_high(self):
        path = MODULE_PATH.with_name('prestartup_script.py')
        spec = importlib.util.spec_from_file_location('zone_lora_prestartup', path)
        assert spec and spec.loader
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        env = {'PYTORCH_MPS_HIGH_WATERMARK_RATIO': '0.7'}
        module.apply_mps_watermarks(env)
        self.assertLessEqual(
            float(env['PYTORCH_MPS_LOW_WATERMARK_RATIO']),
            float(env['PYTORCH_MPS_HIGH_WATERMARK_RATIO']),
        )


if __name__ == '__main__':
    unittest.main()


class CheckpointCadenceTests(unittest.TestCase):
    def interval(self, steps: int, **settings) -> int:
        values = {'checkpoint_every': 50, 'checkpoints_per_run': 8}
        values.update(settings)
        return train_config.checkpoint_interval(steps, values)

    def test_a_long_run_writes_no_more_intermediates_than_a_short_one(self):
        """An intermediate is 220 MB, so a fixed gap bills the disk per step."""
        for steps in (400, 1900, 5700, 6000):
            self.assertLessEqual(
                steps // self.interval(steps), 8, f'{steps} steps writes too many intermediates'
            )

    def test_a_short_run_keeps_the_configured_gap(self):
        self.assertEqual(self.interval(300), 50)

    def test_checkpointing_can_be_turned_off(self):
        self.assertEqual(self.interval(6000, checkpoint_every=0), 0)
