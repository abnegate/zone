"""Runtime hooks so packaged ComfyUI can train and apply LoRAs without editing Comfy core."""

from __future__ import annotations

import logging
from typing import Any

import torch

from comfy.weight_adapter.bypass import BypassForwardHook

from .train_config import is_output_module, is_transformer_block


def install_all() -> None:
    install_inference_safe_bypass()
    install_keep_loaded_on_clone_swap()
    install_bypass_lora_loader()
    install_out_of_place_residuals()
    install_contiguous_tiled_scale()


def install_contiguous_tiled_scale() -> None:
    """Upscaling a whole image works on MPS; upscaling it in tiles does not.

    `ImageUpscaleWithModel` hands `tiled_scale` the permuted view that
    `image.movedim(-1, -3)` returns, whose strides are (H*W*C, 1, W*C, C). A
    slice of that spans two non-contiguous subspaces, and the MPS convolution
    reshapes its input with `view`, which refuses such a tensor:

        view size is not compatible with input tensor's size and stride

    So every ESRGAN pass over an image larger than one tile fails on Apple
    Silicon, which is every image the image and video upscale lanes produce.
    Slicing a contiguous tensor is fine, so the copy happens once here rather
    than per tile, and only when the caller's tensor is not contiguous already.
    """
    import comfy.utils

    if getattr(comfy.utils.tiled_scale_multidim, '_zone_patched', False):
        return
    original = comfy.utils.tiled_scale_multidim

    def tiled_scale_multidim(samples, function, *args, **kwargs):
        if hasattr(samples, 'is_contiguous') and not samples.is_contiguous():
            samples = samples.contiguous()
        return original(samples, function, *args, **kwargs)

    tiled_scale_multidim._zone_patched = True
    tiled_scale_multidim.__wrapped__ = original
    comfy.utils.tiled_scale_multidim = tiled_scale_multidim
    logging.info('Zone LoRA: tiled upscales get a contiguous tensor')


class OutOfPlaceResidual(torch.Tensor):
    """Flux applies block residuals in place, which corrupts tensors autograd saved."""

    @classmethod
    def __torch_function__(cls, func, types, args=(), kwargs=None):
        kwargs = kwargs or {}
        if func is torch.Tensor.add_:
            return torch.add(unwrap(args[0]), unwrap(args[1]), **kwargs)
        return func(
            *(unwrap(arg) for arg in args),
            **{name: unwrap(value) for name, value in kwargs.items()},
        )


def unwrap(value: Any) -> Any:
    if isinstance(value, OutOfPlaceResidual):
        return value.as_subclass(torch.Tensor)
    return value


def install_out_of_place_residuals() -> None:
    import comfy.ldm.flux.layers as layers

    if getattr(layers.apply_mod, '_zone_patched', False):
        return
    original = layers.apply_mod

    def apply_mod(tensor, m_mult, m_add=None, modulation_dims=None):
        output = original(tensor, m_mult, m_add, modulation_dims)
        if torch.is_grad_enabled() and output.requires_grad:
            return output.as_subclass(OutOfPlaceResidual)
        return output

    apply_mod._zone_patched = True
    apply_mod.__wrapped__ = original
    layers.apply_mod = apply_mod
    logging.info('Zone LoRA: flux residual adds stay out-of-place while training')


def install_inference_safe_bypass() -> None:
    if getattr(BypassForwardHook._bypass_forward, '_zone_patched', False):
        return

    def base_forward(hook):
        """An ejected hook can stay installed when two of them wrapped one module.

        Ejecting the inner one first restores the module to the outer one's bypass,
        whose own original_forward is by then None. Falling back to the module's
        class forward is what the module would do with no hook at all.
        """
        if hook.original_forward is not None:
            return hook.original_forward
        logging.warning(
            'Zone LoRA: ejected bypass hook still installed on %s, calling it directly',
            type(hook.module).__name__,
        )
        return type(hook.module).forward.__get__(hook.module)

    def bypass_forward(self, x, *args, **kwargs):
        original_forward = base_forward(self)
        adapter_bypass = getattr(self.adapter, 'bypass_forward', None)
        if adapter_bypass is not None:
            adapter_type = type(self.adapter)
            from comfy.weight_adapter.base import WeightAdapterBase, WeightAdapterTrainBase

            is_default = (
                adapter_type.bypass_forward is WeightAdapterBase.bypass_forward
                or adapter_type.bypass_forward is WeightAdapterTrainBase.bypass_forward
            )
            if not is_default:
                return adapter_bypass(original_forward, x, *args, **kwargs)
        base_out = original_forward(x, *args, **kwargs)
        if torch.is_inference(base_out):
            base_out = base_out.clone()
        if torch.is_inference(x):
            x = x.clone()
        h_out = self.adapter.h(x, base_out)
        return self.adapter.g(base_out + h_out)

    bypass_forward._zone_patched = True
    BypassForwardHook._bypass_forward = bypass_forward
    logging.info('Zone LoRA: installed inference-safe bypass forward')


def install_keep_loaded_on_clone_swap() -> None:
    from comfy.model_patcher import ModelPatcher
    from comfy.patcher_extension import CallbacksMP

    if getattr(ModelPatcher.detach, '_zone_patched', False):
        return
    original = ModelPatcher.detach

    def detach(self, unpatch_all=True):
        if unpatch_all:
            return original(self, True)
        self.eject_model()
        for callback in self.get_all_callbacks(CallbacksMP.ON_DETACH):
            callback(self, False)
        return self.model

    detach._zone_patched = True
    ModelPatcher.detach = detach
    logging.info('Zone LoRA: clone swap keeps shared weights on device')


def install_bypass_lora_loader() -> None:
    import comfy.sd
    import folder_paths
    from nodes import LoraLoader

    if getattr(LoraLoader.load_lora, '_zone_patched', False):
        return

    def load_lora(self, model, clip, lora_name, strength_model, strength_clip):
        if strength_model == 0 and strength_clip == 0:
            return (model, clip)
        lora_path = folder_paths.get_full_path_or_raise('loras', lora_name)
        lora = None
        if self.loaded_lora is not None and self.loaded_lora[0] == lora_path:
            lora = self.loaded_lora[1]
        if lora is None:
            lora, metadata = comfy.utils.load_torch_file(
                lora_path, safe_load=True, return_metadata=True
            )
            self.loaded_lora = (lora_path, lora, metadata)
        model_lora, clip_lora = comfy.sd.load_bypass_lora_for_models(
            model, clip, lora, strength_model, strength_clip
        )
        return (model_lora, clip_lora)

    load_lora._zone_patched = True
    LoraLoader.load_lora = load_lora
    logging.info('Zone LoRA: LoraLoader applies adapters in bypass mode')


def prepare_frozen_weights(model) -> int:
    """Autograd refuses to save an inference tensor for backward.

    Leaving the adapted weights that way forces the base matmul under no_grad, which
    severs the chain the backward pass needs: an adapter only ever sees gradient that
    reached it through the base weights of every layer below it.
    """
    converted = 0
    for _, module in model.named_modules():
        for attr in ('weight', 'bias'):
            tensor = getattr(module, attr, None)
            if tensor is None or not torch.is_inference(tensor):
                continue
            setattr(
                module,
                attr,
                torch.nn.Parameter(tensor.detach().clone(), requires_grad=False),
            )
            converted += 1
    logging.info('Zone LoRA: cloned %s inference weights off autograd-unsafe storage', converted)
    return converted


def wrap_early_frozen(model) -> list:
    restores = []
    for name, module in model.named_modules():
        if not hasattr(module, 'forward') or not hasattr(module, 'weight_function'):
            continue
        if is_transformer_block(name) or is_output_module(name):
            continue
        original = module.forward

        def make_frozen(org_forward):
            def frozen_forward(*args, **kwargs):
                with torch.no_grad():
                    output = org_forward(*args, **kwargs)
                if torch.is_inference(output):
                    return output.clone()
                return output.detach()

            return frozen_forward

        module.forward = make_frozen(original)
        restores.append((module, original))
    logging.info('Zone LoRA: wrapped %s early frozen modules', len(restores))
    return restores
