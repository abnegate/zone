"""MPS kernels corrupt Wan 2.2 video frames (Comfy-Org/ComfyUI#15793, #16644)."""

from __future__ import annotations

import os
import sys
import unittest
from pathlib import Path
from unittest import mock

import torch

COMFYUI = Path(__file__).parents[1]
_INSTALL = os.environ.get('COMFYUI_INSTALL_DIR', '').strip()
if not _INSTALL:
    raise RuntimeError('COMFYUI_INSTALL_DIR must point to the pinned ComfyUI checkout')
INSTALL = Path(_INSTALL)
if not INSTALL.is_dir():
    raise RuntimeError(f'COMFYUI_INSTALL_DIR is not a directory: {INSTALL}')
# The checkout's custom_nodes must shadow the copy --apply-nodes put in the runtime.
for path in (str(INSTALL), str(COMFYUI)):
    while path in sys.path:
        sys.path.remove(path)
sys.path.insert(0, str(INSTALL))
sys.path.insert(0, str(COMFYUI))

from comfy.cli_args import args  # noqa: E402

# ComfyUI picks CUDA while importing comfy.model_management unless --cpu is set first.
args.cpu = True

from custom_nodes.zone_lora.inference_hooks import (  # noqa: E402
    constant_pad_with_cat,
    densify_tensor,
    install_mps_fp8_dequant,
    install_mps_kernel_workarounds,
    install_wan_mps_cpu_unet,
    install_wan_mps_cpu_vae,
    is_wan_14b_path,
    materialize_quantized_to_float16,
    should_rewrite_mps_pad,
    wan_cpu_model_options,
    wan_cpu_vae_device,
    wrap_mps_baddbmm,
    wrap_mps_bmm,
    wrap_mps_fp8_dequant,
)


def mps_available(available: bool):
    return mock.patch('torch.backends.mps.is_available', return_value=available)


class WanMps(unittest.TestCase):
    def test_flux_and_qwen_keep_their_load_device(self) -> None:
        with mps_available(True):
            options = wan_cpu_model_options(
                'qwen_image_edit_2511_fp8mixed.safetensors',
                {'dtype': torch.float16},
            )
        self.assertEqual(options, {'dtype': torch.float16})

    def test_only_wan_paths_are_wan_14b_experts(self) -> None:
        for path in (
            '/models/diffusion_models/wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors',
            '/models/diffusion_models/wan2.2_i2v_low_noise_14B_fp8_scaled.safetensors',
            'Wan2.1_T2V_14B.safetensors',
        ):
            self.assertTrue(is_wan_14b_path(path), path)
        for path in (
            '/models/diffusion_models/wan2.2_ti2v_5B_fp16.safetensors',
            '/models/diffusion_models/hunyuan_video_14B.safetensors',
            '/models/diffusion_models/cosmos_predict2_14B.safetensors',
            '/models/diffusion_models/high_noise_refiner.safetensors',
            '/models/diffusion_models/low_noise_refiner.safetensors',
        ):
            self.assertFalse(is_wan_14b_path(path), path)

    def test_wan_keeps_caller_options_without_mps(self) -> None:
        with mps_available(False):
            options = wan_cpu_model_options(
                'wan2.2_ti2v_5B_fp16.safetensors',
                {'dtype': torch.float16},
            )
        self.assertEqual(options, {'dtype': torch.float16})

    def test_wan_weights_are_pinned_to_cpu_on_mps(self) -> None:
        with mps_available(True):
            options = wan_cpu_model_options(
                '/models/diffusion_models/wan2.2_ti2v_5B_fp16.safetensors',
                {'dtype': torch.float16},
            )
        self.assertEqual(options['load_device'], torch.device('cpu'))
        self.assertEqual(options['offload_device'], torch.device('cpu'))
        self.assertEqual(options['dtype'], torch.float16)

    def test_a14b_weights_load_on_cpu_on_mps(self) -> None:
        with mps_available(True):
            options = wan_cpu_model_options(
                '/models/diffusion_models/wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors',
                {'dtype': torch.float16},
            )
        self.assertEqual(
            options,
            {
                'dtype': torch.float16,
                'load_device': torch.device('cpu'),
                'offload_device': torch.device('cpu'),
            },
        )

    def test_constant_pad_with_cat_matches_functional_pad(self) -> None:
        tensor = torch.arange(2 * 3 * 4 * 5 * 6, dtype=torch.float32).reshape(2, 3, 4, 5, 6)
        pad = (1, 2, 0, 1, 3, 0)
        expected = torch.nn.functional.pad(tensor, pad, mode='constant', value=0)
        got = constant_pad_with_cat(tensor, pad, 0)
        self.assertEqual(tuple(got.shape), tuple(expected.shape))
        self.assertTrue(torch.equal(got, expected))

    def test_cpu_5d_pad_is_not_rewritten(self) -> None:
        tensor = torch.zeros(1, 1, 2, 2, 2)
        self.assertFalse(should_rewrite_mps_pad(tensor, (1, 0, 0, 0, 0, 0), 'constant'))

    def test_bmm_wrapper_makes_mps_inputs_contiguous(self) -> None:
        seen: list[tuple[bool, bool]] = []

        def original(left, right, *args, **kwargs):
            seen.append((left.is_contiguous(), right.is_contiguous()))
            return left

        wrapped = wrap_mps_bmm(original)
        if not torch.backends.mps.is_available():
            left = torch.randn(2, 8, 4).transpose(1, 2)
            right = torch.randn(2, 4, 6)
            wrapped(left, right)
            self.assertEqual(seen, [(left.is_contiguous(), right.is_contiguous())])
            return
        left = torch.randn(2, 8, 4, device='mps').transpose(1, 2)
        right = torch.randn(2, 4, 6, device='mps')
        self.assertFalse(left.is_contiguous())
        wrapped(left, right)
        self.assertEqual(seen, [(True, True)])

    def test_baddbmm_wrapper_rewrites_beta_zero_on_mps(self) -> None:
        seen: list[tuple[float, float]] = []

        def original(input, batch1, batch2, *args, **kwargs):
            seen.append((float(input.sum().cpu()), float(kwargs.get('beta', 1))))
            return input

        wrapped = wrap_mps_baddbmm(original)
        if not torch.backends.mps.is_available():
            input = torch.ones(2, 3, 3)
            wrapped(input, input, input, beta=0)
            self.assertEqual(seen, [(float(input.sum()), 0.0)])
            return
        input = torch.ones(2, 3, 3, device='mps')
        batch1 = torch.ones(2, 3, 4, device='mps')
        batch2 = torch.ones(2, 4, 3, device='mps')
        wrapped(input, batch1, batch2, beta=0)
        self.assertEqual(seen, [(0.0, 1.0)])

    def unwrap_mps_kernels(self) -> dict[str, object]:
        kernels = {
            'pad': (torch.nn.functional, 'pad'),
            'bmm': (torch, 'bmm'),
            'tensor_bmm': (torch.Tensor, 'bmm'),
            'baddbmm': (torch, 'baddbmm'),
            'tensor_baddbmm': (torch.Tensor, 'baddbmm'),
        }
        originals = {}
        for name, (owner, attribute) in kernels.items():
            installed = getattr(owner, attribute)
            self.addCleanup(setattr, owner, attribute, installed)
            original = installed
            while hasattr(original, '__wrapped__'):
                original = original.__wrapped__
            setattr(owner, attribute, original)
            originals[name] = original
        return originals

    def installed_mps_kernels(self) -> dict[str, object]:
        return {
            'pad': torch.nn.functional.pad,
            'bmm': torch.bmm,
            'tensor_bmm': torch.Tensor.bmm,
            'baddbmm': torch.baddbmm,
            'tensor_baddbmm': torch.Tensor.baddbmm,
        }

    def test_mps_workarounds_leave_every_kernel_alone_without_mps(self) -> None:
        originals = self.unwrap_mps_kernels()
        with mps_available(False):
            install_mps_kernel_workarounds()
        for name, kernel in self.installed_mps_kernels().items():
            with self.subTest(kernel=name):
                self.assertIs(kernel, originals[name])

    def test_mps_workarounds_wrap_every_kernel_on_mps(self) -> None:
        originals = self.unwrap_mps_kernels()
        with mps_available(True):
            install_mps_kernel_workarounds()
        for name, kernel in self.installed_mps_kernels().items():
            with self.subTest(kernel=name):
                self.assertTrue(getattr(kernel, '_zone_patched', False))
                self.assertIs(kernel.__wrapped__, originals[name])

    def test_installing_mps_workarounds_twice_does_not_stack_wrappers(self) -> None:
        originals = self.unwrap_mps_kernels()
        with mps_available(True):
            install_mps_kernel_workarounds()
            once = self.installed_mps_kernels()
            install_mps_kernel_workarounds()
        self.assertEqual(self.installed_mps_kernels(), once)
        self.assertIs(once['pad'].__wrapped__, originals['pad'])

    @unittest.skipUnless(torch.backends.mps.is_available(), 'mps is not available')
    def test_rewritten_5d_pad_on_mps_matches_cpu(self) -> None:
        cpu = torch.arange(2 * 3 * 4 * 5 * 6, dtype=torch.float32).reshape(2, 3, 4, 5, 6)
        pad = (1, 2, 0, 0, 3, 0)
        expected = torch.nn.functional.pad(cpu, pad, mode='constant', value=0)
        mps = cpu.to('mps')
        self.assertTrue(should_rewrite_mps_pad(mps, pad, 'constant'))
        got = constant_pad_with_cat(mps, pad, 0)
        self.assertTrue(torch.equal(got.cpu(), expected))

    def test_densify_float8_on_cpu(self) -> None:
        tensor = torch.ones(2, 4).to(torch.float8_e4m3fn)
        dense = densify_tensor(tensor)
        self.assertEqual(dense.dtype, torch.float16)
        self.assertEqual(dense.device.type, 'cpu')
        self.assertEqual(tuple(dense.shape), (2, 4))

    def test_densify_uses_dequantize(self) -> None:
        class Quantized:
            device = torch.device('cpu')

            def dequantize(self):
                return torch.ones(2, 3)

        dense = densify_tensor(Quantized())
        self.assertEqual(dense.dtype, torch.float16)
        self.assertTrue(torch.equal(dense, torch.ones(2, 3, dtype=torch.float16)))

    def test_materialize_replaces_float8_parameters(self) -> None:
        module = torch.nn.Linear(4, 3, bias=False)
        module._parameters['weight'] = torch.ones(3, 4).to(torch.float8_e4m3fn)
        converted = materialize_quantized_to_float16(module)
        self.assertEqual(converted, 1)
        self.assertEqual(module.weight.dtype, torch.float16)
        self.assertIsInstance(module.weight, torch.nn.Parameter)

    def test_fp8_dequant_wrapper_passthrough_on_cpu(self) -> None:
        seen: list[str] = []

        def original(x, scale, output_type=torch.bfloat16):
            seen.append(x.device.type)
            return x.to(dtype=output_type) * scale.to(dtype=output_type)

        wrapped = wrap_mps_fp8_dequant(original)
        tensor = torch.ones(2, 2).to(torch.float8_e4m3fn)
        scale = torch.tensor(2.0)
        output = wrapped(tensor, scale, torch.float16)
        self.assertEqual(seen, ['cpu'])
        self.assertEqual(output.dtype, torch.float16)
        self.assertTrue(torch.allclose(output, torch.ones(2, 2, dtype=torch.float16) * 2))

    @unittest.skipUnless(torch.backends.mps.is_available(), 'mps is not available')
    def test_fp8_dequant_wrapper_moves_mps_to_cpu(self) -> None:
        seen: list[str] = []

        def original(x, scale, output_type=torch.bfloat16):
            seen.append(x.device.type)
            return x.to(dtype=output_type) * scale.to(dtype=output_type)

        wrapped = wrap_mps_fp8_dequant(original)
        tensor = torch.ones(2, 2).to(torch.float8_e4m3fn).to('mps')
        scale = torch.tensor(1.0, device='mps')
        output = wrapped(tensor, scale, torch.float16)
        self.assertEqual(seen, ['cpu'])
        self.assertEqual(output.device.type, 'mps')
        self.assertEqual(output.dtype, torch.float16)

    def test_installing_fp8_dequant_twice_does_not_stack_wrappers(self) -> None:
        try:
            import comfy_kitchen.backends.eager as eager
        except ImportError:
            self.skipTest('comfy_kitchen missing')
        installed = eager.dequantize_per_tensor_fp8
        self.addCleanup(setattr, eager, 'dequantize_per_tensor_fp8', installed)
        import comfy_kitchen.backends.eager.quantization as quantization

        installed_quant = quantization.dequantize_per_tensor_fp8
        self.addCleanup(setattr, quantization, 'dequantize_per_tensor_fp8', installed_quant)
        original = installed
        while hasattr(original, '__wrapped__'):
            original = original.__wrapped__
        eager.dequantize_per_tensor_fp8 = original
        quantization.dequantize_per_tensor_fp8 = original
        install_mps_fp8_dequant()
        once = eager.dequantize_per_tensor_fp8
        install_mps_fp8_dequant()
        self.assertIs(eager.dequantize_per_tensor_fp8, once)
        self.assertIs(once.__wrapped__, original)
        self.assertIs(quantization.dequantize_per_tensor_fp8, once)

    def test_a14b_load_materializes_fp16_and_sets_mps(self) -> None:
        import comfy.sd

        installed = comfy.sd.load_diffusion_model
        self.addCleanup(setattr, comfy.sd, 'load_diffusion_model', installed)

        class FakePatcher:
            def __init__(self):
                self.model = torch.nn.Linear(4, 3, bias=False)
                self.model._parameters['weight'] = torch.ones(3, 4).to(torch.float8_e4m3fn)
                self.load_device = torch.device('cpu')
                self.offload_device = torch.device('cpu')
                self.size = 1

        seen: list[tuple] = []

        def fake_load(unet_path, model_options=None, disable_dynamic=False):
            seen.append((unet_path, dict(model_options or {}), disable_dynamic))
            return FakePatcher()

        comfy.sd.load_diffusion_model = fake_load
        install_wan_mps_cpu_unet()
        with mps_available(True):
            patcher = comfy.sd.load_diffusion_model(
                '/models/diffusion_models/wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors'
            )
        self.assertEqual(len(seen), 1)
        self.assertEqual(seen[0][1]['load_device'], torch.device('cpu'))
        self.assertEqual(seen[0][1]['offload_device'], torch.device('cpu'))
        self.assertTrue(seen[0][2])
        self.assertEqual(patcher.load_device, torch.device('mps'))
        self.assertEqual(patcher.offload_device, torch.device('cpu'))
        self.assertEqual(patcher.model.weight.dtype, torch.float16)
        self.assertEqual(patcher.size, 0)

    def test_a14b_load_keeps_caller_options_without_mps(self) -> None:
        import comfy.sd

        installed = comfy.sd.load_diffusion_model
        self.addCleanup(setattr, comfy.sd, 'load_diffusion_model', installed)
        seen: list[tuple] = []
        loaded = object()

        def fake_load(unet_path, model_options=None, disable_dynamic=False):
            seen.append((dict(model_options or {}), disable_dynamic))
            return loaded

        comfy.sd.load_diffusion_model = fake_load
        install_wan_mps_cpu_unet()
        with mps_available(False):
            patcher = comfy.sd.load_diffusion_model(
                '/models/diffusion_models/wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors',
                model_options={'dtype': torch.float16},
            )
        self.assertIs(patcher, loaded)
        self.assertEqual(seen, [({'dtype': torch.float16}, False)])

    def test_installing_the_unet_hook_twice_does_not_stack_wrappers(self) -> None:
        import comfy.sd

        installed = comfy.sd.load_diffusion_model
        self.addCleanup(setattr, comfy.sd, 'load_diffusion_model', installed)
        original = installed
        while hasattr(original, '__wrapped__'):
            original = original.__wrapped__
        comfy.sd.load_diffusion_model = original
        install_wan_mps_cpu_unet()
        once = comfy.sd.load_diffusion_model
        install_wan_mps_cpu_unet()
        self.assertIs(comfy.sd.load_diffusion_model, once)
        self.assertIs(once.__wrapped__, original)

    def test_a_non_wan_14b_model_loads_untouched_on_mps(self) -> None:
        import comfy.sd

        installed = comfy.sd.load_diffusion_model
        self.addCleanup(setattr, comfy.sd, 'load_diffusion_model', installed)
        seen: list[tuple] = []
        loaded = object()

        def fake_load(unet_path, model_options=None, disable_dynamic=False):
            seen.append((dict(model_options or {}), disable_dynamic))
            return loaded

        comfy.sd.load_diffusion_model = fake_load
        install_wan_mps_cpu_unet()
        with mps_available(True):
            patcher = comfy.sd.load_diffusion_model(
                '/models/diffusion_models/hunyuan_video_14B.safetensors',
                model_options={'dtype': torch.float16},
            )
        self.assertIs(patcher, loaded)
        self.assertEqual(seen, [({'dtype': torch.float16}, False)])

    def test_flux_vae_keeps_caller_device(self) -> None:
        with mps_available(True):
            device = wan_cpu_vae_device({'decoder.conv_out.weight': torch.zeros(1)})
        self.assertIsNone(device)

    def test_wan_vae_is_pinned_to_cpu_on_mps(self) -> None:
        with mps_available(True):
            device = wan_cpu_vae_device({'decoder.middle.0.residual.0.gamma': torch.zeros(1)})
        self.assertEqual(device, torch.device('cpu'))

    def test_wan_vae_keeps_caller_device_without_mps(self) -> None:
        with mps_available(False):
            device = wan_cpu_vae_device({'decoder.middle.0.residual.0.gamma': torch.zeros(1)})
        self.assertIsNone(device)

    def test_wan_vae_keeps_an_explicit_device(self) -> None:
        self.assertEqual(
            wan_cpu_vae_device(
                {'decoder.middle.0.residual.0.gamma': torch.zeros(1)},
                torch.device('mps'),
            ),
            torch.device('mps'),
        )

    def test_installing_the_vae_hook_twice_does_not_stack_wrappers(self) -> None:
        import comfy.sd

        installed = comfy.sd.VAE.__init__
        self.addCleanup(setattr, comfy.sd.VAE, '__init__', installed)
        original = installed
        while hasattr(original, '__wrapped__'):
            original = original.__wrapped__
        comfy.sd.VAE.__init__ = original
        install_wan_mps_cpu_vae()
        once = comfy.sd.VAE.__init__
        install_wan_mps_cpu_vae()
        self.assertIs(comfy.sd.VAE.__init__, once)
        self.assertIs(once.__wrapped__, original)


if __name__ == '__main__':
    unittest.main()
