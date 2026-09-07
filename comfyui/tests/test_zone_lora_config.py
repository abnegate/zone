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
    def test_steps_scale_with_images_and_clamp(self):
        config = train_config.load_config()
        self.assertGreaterEqual(train_config.train_steps(1, config), int(config['min_steps']))
        self.assertEqual(
            train_config.train_steps(8, config),
            max(int(config['min_steps']), 8 * int(config['steps_per_image'])),
        )
        self.assertLessEqual(train_config.train_steps(10_000, config), int(config['max_steps']))

    def test_identity_defaults_use_full_blocks_and_rank_alpha(self):
        config = train_config.load_config()
        self.assertTrue(config['alpha_equals_rank'])
        self.assertEqual(train_config.lora_alpha(8, config), 8.0)
        self.assertEqual(int(config['rank']), 8)
        self.assertGreaterEqual(int(config['min_steps']), 400)
        self.assertEqual(int(config['steps_per_image']), 50)
        self.assertEqual(train_config.train_steps(8, config), 400)
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
        config = train_config.load_config()
        every = int(config['checkpoint_every'])
        self.assertGreater(every, 0, 'a long run must be testable before it ends')
        self.assertLessEqual(
            every,
            int(config['min_steps']) // 2,
            'at least two checkpoints before the shortest run finishes',
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
