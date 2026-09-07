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

    def bypass_forward(self, x, *args, **kwargs):
        adapter_bypass = getattr(self.adapter, 'bypass_forward', None)
        if adapter_bypass is not None:
            adapter_type = type(self.adapter)
            from comfy.weight_adapter.base import WeightAdapterBase, WeightAdapterTrainBase

            is_default = (
                adapter_type.bypass_forward is WeightAdapterBase.bypass_forward
                or adapter_type.bypass_forward is WeightAdapterTrainBase.bypass_forward
            )
            if not is_default:
                return adapter_bypass(self.original_forward, x, *args, **kwargs)
        with torch.no_grad():
            base_out = self.original_forward(x, *args, **kwargs)
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
    converted = 0
    for name, module in model.named_modules():
        for attr in ('weight', 'bias'):
            tensor = getattr(module, attr, None)
            if tensor is None or not torch.is_inference(tensor):
                continue
            if tensor.ndim >= 2 and is_transformer_block(name) and not is_output_module(name):
                continue
            if tensor.ndim >= 2 and not is_output_module(name) and not is_transformer_block(name):
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
