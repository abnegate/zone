from __future__ import annotations

import importlib.util
import os
import sys
import types
import unittest
from contextlib import contextmanager
from pathlib import Path

NODE_DIR = Path(__file__).parents[1] / 'custom_nodes' / 'zone_lora'
COMFY_DIR = Path(
    os.environ.get(
        'COMFYUI_INSTALL_DIR',
        Path.home() / 'Library' / 'Application Support' / 'Zone' / 'ComfyUI',
    )
)

HIDDEN = 64
HEADS = 4
MLP_RATIO = 4.0
RANK = 4


def load_module(name: str):
    if 'zone_lora_under_test' not in sys.modules:
        package = types.ModuleType('zone_lora_under_test')
        package.__path__ = [str(NODE_DIR)]
        sys.modules['zone_lora_under_test'] = package
    spec = importlib.util.spec_from_file_location(
        f'zone_lora_under_test.{name}', NODE_DIR / f'{name}.py'
    )
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def load_hooks():
    return load_module('inference_hooks')


def comfy_available() -> bool:
    if not (COMFY_DIR / 'comfy' / 'ldm' / 'flux' / 'layers.py').is_file():
        return False
    if str(COMFY_DIR) not in sys.path:
        sys.path.insert(0, str(COMFY_DIR))
    sys.argv = ['main.py', '--cpu']
    try:
        import comfy.ldm.flux.layers  # noqa: F401
        import comfy.ops  # noqa: F401
        import torch  # noqa: F401
    except Exception:
        return False
    return True


AVAILABLE = comfy_available()


@unittest.skipUnless(AVAILABLE, f'ComfyUI runtime not installed at {COMFY_DIR}')
class FluxTrainingGradientTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        import comfy.model_management
        import torch

        cls.torch = torch
        comfy.model_management.get_torch_device = lambda: torch.device('cpu')
        comfy.model_management.in_training = True
        cls.hooks = load_hooks()
        cls.hooks.install_inference_safe_bypass()
        cls.hooks.install_out_of_place_residuals()
        cls.node = load_module('train_node')

    @contextmanager
    def upstream_apply_mod(self):
        import comfy.ldm.flux.layers as layers

        patched = layers.apply_mod
        self.assertTrue(
            getattr(patched, '_zone_patched', False), 'out-of-place hook is not installed'
        )
        layers.apply_mod = patched.__wrapped__
        try:
            yield
        finally:
            layers.apply_mod = patched

    def build_blocks(self):
        import comfy.ops
        import torch
        from comfy.ldm.flux.layers import DoubleStreamBlock, SingleStreamBlock

        operations = comfy.ops.disable_weight_init
        torch.manual_seed(0)
        double = DoubleStreamBlock(
            HIDDEN, HEADS, MLP_RATIO, qkv_bias=True,
            dtype=torch.float32, device='cpu', operations=operations,
        )
        single = SingleStreamBlock(
            HIDDEN, HEADS, MLP_RATIO,
            dtype=torch.float32, device='cpu', operations=operations,
        )
        blocks = torch.nn.ModuleDict({'double_blocks': double, 'single_blocks': single})
        with torch.no_grad():
            for parameter in blocks.parameters():
                parameter.normal_(0.0, 0.02)
            for _, buffer in blocks.named_buffers():
                buffer.normal_(0.0, 0.02)
        blocks.requires_grad_(False).train()
        return blocks

    def attach_adapters(self, blocks):
        from comfy.weight_adapter import adapter_maps
        from comfy.weight_adapter.bypass import BypassInjectionManager

        manager = BypassInjectionManager()
        adapters = []
        for name, module in blocks.named_modules():
            weight = getattr(module, 'weight', None)
            if not hasattr(module, 'weight_function') or weight is None or weight.ndim < 2:
                continue
            adapter = adapter_maps['LoRA'].create_train(weight, rank=RANK, alpha=float(RANK))
            adapters.append(adapter.train().requires_grad_(True))
            manager.add_adapter(f'{name}.weight', adapters[-1], strength=1.0)
        manager.create_injections(blocks)
        for hook in manager.hooks:
            hook.inject()
        return adapters

    def train_step(self, checkpointing: bool = False):
        import torch
        from comfy.ldm.flux.layers import EmbedND

        blocks = self.build_blocks()
        adapters = self.attach_adapters(blocks)
        double = blocks['double_blocks']
        single = blocks['single_blocks']

        if checkpointing:
            for block in (double, single):
                original = block.forward

                def call(args, kwargs, original=original):
                    return original(*args, **kwargs)

                def checkpointed(*args, call=call, **kwargs):
                    return torch.utils.checkpoint.checkpoint(
                        call, args, kwargs, use_reentrant=False
                    )

                block.forward = checkpointed

        image_tokens, text_tokens = 8, 4
        image = torch.randn(1, image_tokens, HIDDEN)
        text = torch.randn(1, text_tokens, HIDDEN)
        vector = torch.randn(1, HIDDEN)
        head_dim = HIDDEN // HEADS
        positions = torch.arange(image_tokens + text_tokens).float()
        embedding = EmbedND(dim=head_dim, theta=10000, axes_dim=[head_dim])
        pe = embedding(positions.reshape(1, image_tokens + text_tokens, 1))

        image, text = double(img=image, txt=text, vec=vector, pe=pe)
        hidden = torch.cat((text, image), dim=1)
        hidden = single(hidden, vec=vector, pe=pe)
        loss = hidden.float().pow(2).mean()
        loss.backward()
        gradients = [adapter.lora_down.weight.grad for adapter in adapters]
        return float(loss.detach()), hidden.detach(), gradients

    def test_reseeding_lets_both_lora_matrices_learn(self):
        """Comfy's default seeds lora_down at zero, so lora_up never gets a
        gradient and the adapter is stuck in a fixed random subspace."""
        import torch
        from comfy.weight_adapter import adapter_maps

        linear = torch.nn.Linear(HIDDEN, HIDDEN * 3)
        x = torch.randn(1, 4, HIDDEN)

        def gradients(reseed):
            torch.manual_seed(0)
            adapter = adapter_maps['LoRA'].create_train(
                linear.weight, rank=RANK, alpha=float(RANK)
            )
            if reseed:
                self.node.reseed(adapter)
            adapter.train().requires_grad_(True)
            adapter.multiplier, adapter.is_conv, adapter.conv_dim = 1.0, False, 0
            adapter.kw_dict = {}
            base = linear(x)
            (base + adapter.h(x, base)).pow(2).mean().backward()
            return adapter.lora_up.weight.grad, adapter.lora_down.weight.grad

        up, down = gradients(reseed=False)
        self.assertEqual(float(up.abs().sum()), 0.0, 'upstream leaves lora_up untrained')
        self.assertGreater(float(down.abs().sum()), 0.0)

        up, down = gradients(reseed=True)
        self.assertGreater(float(up.abs().sum()), 0.0, 'lora_up must receive gradient')

    def test_upstream_in_place_residuals_break_training(self):
        with self.upstream_apply_mod():
            with self.assertRaises(RuntimeError) as raised:
                self.train_step()
        self.assertIn('inplace operation', str(raised.exception))

    def test_hook_restores_finite_gradients_for_every_adapter(self):
        torch = self.torch
        _, output, gradients = self.train_step()
        self.assertTrue(bool(torch.isfinite(output).all()), 'block output must stay finite')
        self.assertGreater(len(gradients), 0, 'expected at least one LoRA adapter')
        for index, gradient in enumerate(gradients):
            self.assertIsNotNone(gradient, f'adapter {index} received no gradient')
            self.assertTrue(
                bool(torch.isfinite(gradient).all()), f'adapter {index} gradient is not finite'
            )
            self.assertGreater(
                float(gradient.abs().sum()), 0.0, f'adapter {index} gradient is zero'
            )

    def test_an_ejected_hook_left_installed_still_runs_the_module(self):
        """Nested hooks eject out of order and leave one behind with no original."""
        import torch
        from comfy.weight_adapter import adapter_maps
        from comfy.weight_adapter.bypass import BypassForwardHook

        blocks = self.build_blocks()
        module = blocks['double_blocks'].img_attn.qkv
        adapter = adapter_maps['LoRA'].create_train(module.weight, rank=RANK, alpha=float(RANK))
        hook = BypassForwardHook(module, adapter, 1.0)
        hook.inject()
        expected = module.forward(torch.randn(1, 4, HIDDEN))
        hook.original_forward = None
        with self.assertLogs(level='WARNING'):
            actual = module.forward(torch.randn(1, 4, HIDDEN) * 0 + 0)
        self.assertEqual(actual.shape, expected.shape)
        self.assertTrue(bool(torch.isfinite(actual).all()))

    def test_inference_path_is_untouched(self):
        import comfy.ldm.flux.layers as layers
        import torch

        tensor = torch.randn(1, 4, HIDDEN)
        multiplier = torch.randn(1, 1, HIDDEN)
        shift = torch.randn(1, 1, HIDDEN)
        with torch.no_grad():
            hooked = layers.apply_mod(tensor, multiplier, shift, None)
            with self.upstream_apply_mod():
                expected = layers.apply_mod(tensor, multiplier, shift, None)
        self.assertIs(type(hooked), torch.Tensor, 'inference must not see the residual subclass')
        self.assertTrue(bool(torch.equal(hooked, expected)))

    def identity_lora(self):
        node = self.node
        settings = node.load_config()
        settings['min_adapters'] = 1
        original = node.load_config
        node.load_config = lambda *args, **kwargs: settings
        try:
            return node.setup_identity_lora(
                types.SimpleNamespace(model=self.build_blocks()),
                {},
                'LoRA',
                self.torch.float32,
                RANK,
            )
        finally:
            node.load_config = original

    def test_alpha_is_a_constant_not_a_parameter_to_optimise(self):
        """A trained alpha rescales every adapter each step and diverges the run."""
        lora_sd, _, _ = self.identity_lora()
        alphas = [key for key in lora_sd if key.endswith('.alpha')]
        self.assertTrue(alphas, 'adapters must record their alpha')
        for key in alphas:
            self.assertFalse(lora_sd[key].requires_grad, f'{key} would be stepped by the optimiser')

    def test_both_lora_matrices_are_trainable(self):
        lora_sd, _, _ = self.identity_lora()
        matrices = [key for key in lora_sd if key.endswith(('lora_up.weight', 'lora_down.weight'))]
        self.assertTrue(matrices)
        for key in matrices:
            self.assertTrue(lora_sd[key].requires_grad, f'{key} must train')

    def test_gradient_checkpointing_matches_plain_backward(self):
        torch = self.torch
        plain_loss, _, plain_gradients = self.train_step(checkpointing=False)
        checkpoint_loss, _, checkpoint_gradients = self.train_step(checkpointing=True)
        self.assertAlmostEqual(plain_loss, checkpoint_loss, places=6)
        self.assertEqual(len(plain_gradients), len(checkpoint_gradients))
        for index, (expected, actual) in enumerate(zip(plain_gradients, checkpoint_gradients)):
            self.assertTrue(
                bool(torch.allclose(expected, actual, rtol=1e-5, atol=1e-7)),
                f'adapter {index} gradient differs under gradient checkpointing',
            )


@unittest.skipUnless(AVAILABLE, f'ComfyUI runtime not installed at {COMFY_DIR}')
class LossWeightingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        import torch

        cls.torch = torch
        cls.node = load_module('train_node')
        cls.sampler_cls = cls.node.ZoneTrainSampler

    def sampler(self, sigma_floor):
        sampler = self.sampler_cls.__new__(self.sampler_cls)
        sampler.sigma_floor = sigma_floor
        return sampler

    def losses_across_sigmas(self, sigma_floor):
        """The x0 error a fixed velocity error produces is exactly sigma times it."""
        torch = self.torch
        sampler = self.sampler(sigma_floor)
        velocity_error = torch.full((1, 4, 8, 8), 0.25)
        losses = []
        for sigma in (0.2, 0.5, 1.0):
            sigmas = torch.tensor([sigma])
            error = velocity_error * sigma
            scale = sampler.error_scale(sigmas, error)
            losses.append(float(torch.nn.functional.mse_loss(error / scale, torch.zeros_like(error) / scale)))
        return losses

    def test_x0_space_loss_ignores_the_clean_end_of_the_schedule(self):
        quietest, middle, noisiest = self.losses_across_sigmas(0.0)
        self.assertAlmostEqual(middle / quietest, (0.5 / 0.2) ** 2, places=4)
        self.assertAlmostEqual(noisiest / quietest, (1.0 / 0.2) ** 2, places=4)

    def test_velocity_space_loss_weights_every_noise_level_alike(self):
        quietest, middle, noisiest = self.losses_across_sigmas(0.05)
        self.assertAlmostEqual(middle, quietest, places=6)
        self.assertAlmostEqual(noisiest, quietest, places=6)

    def test_every_probe_measures_the_objective_training_optimises(self):
        """A probe on raw x0 error ranks adapters by sigma squared, which training does not."""
        torch = self.torch
        node = self.node
        floor = float(node.load_config().get('sigma_floor', 0.0))
        self.assertGreater(floor, 0.0, 'the shared scale is only meaningful with a floor')
        sample = torch.zeros((1, 4, 8, 8))
        for sigma in (0.2, 0.6, 0.95):
            sigmas = torch.tensor([sigma])
            shared = node.error_scale(sigmas, sample, floor)
            sampler = self.sampler_with_floor(floor)
            self.assertTrue(
                bool(torch.equal(shared, sampler.error_scale(sigmas, sample))),
                f'the trainer and the probes disagree at sigma {sigma}',
            )

    def test_both_probes_take_their_scale_from_the_trainer(self):
        """Sharing the function is what stops a probe re-deriving a different objective."""
        import ast

        for name in ('probe_node', 'gradient_node'):
            source = (NODE_DIR / f'{name}.py').read_text()
            imported = {
                alias.name
                for node in ast.walk(ast.parse(source))
                if isinstance(node, ast.ImportFrom) and node.module == 'train_node'
                for alias in node.names
            }
            self.assertIn('error_scale', imported, f'{name} must scale the way training does')

    def sampler_with_floor(self, floor: float):
        sampler = self.node.ZoneTrainSampler.__new__(self.node.ZoneTrainSampler)
        sampler.sigma_floor = floor
        return sampler

    def test_floor_caps_the_amplification_near_zero_noise(self):
        torch = self.torch
        sampler = self.sampler(0.05)
        error = torch.zeros((1, 4, 8, 8))
        scale = sampler.error_scale(torch.tensor([1e-6]), error)
        self.assertAlmostEqual(float(scale.reshape(-1)[0]), 0.05, places=6)


if __name__ == '__main__':
    unittest.main()


@unittest.skipUnless(AVAILABLE, f'ComfyUI runtime not installed at {COMFY_DIR}')
class CheckpointCadenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.node = load_module('train_node')

    def interval(self, steps: int, **settings) -> int:
        base = {'checkpoint_every': 50, 'checkpoints_per_run': 8}
        base.update(settings)
        return self.node.checkpoint_interval(steps, base)

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
