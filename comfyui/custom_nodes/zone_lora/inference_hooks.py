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
    install_mps_kernel_workarounds()
    install_mps_fp8_dequant()
    install_wan_mps_cpu_unet()
    install_wan_mps_cpu_vae()


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


def should_rewrite_mps_pad(tensor, pad, mode) -> bool:
    """MPS F.pad on 5D constant tensors corrupts the temporal axis (pytorch/pytorch#194922)."""
    if mode not in (None, 'constant'):
        return False
    device = getattr(tensor, 'device', None)
    if device is None or device.type != 'mps':
        return False
    if getattr(tensor, 'ndim', 0) < 5:
        return False
    try:
        length = len(pad)
    except TypeError:
        return False
    return length <= tensor.ndim * 2 and length % 2 == 0


def constant_pad_with_cat(tensor, pad, value=0):
    if value is None:
        value = 0
    pairs = [int(item) for item in pad]
    if len(pairs) % 2 != 0:
        raise ValueError('padding length must be even')
    result = tensor
    dim = result.ndim - 1
    for index in range(0, len(pairs), 2):
        left, right = pairs[index], pairs[index + 1]
        size = result.size(dim)
        start = -left if left < 0 else 0
        end = size + right if right < 0 else size
        if start != 0 or end != size:
            slicer = [slice(None)] * result.ndim
            slicer[dim] = slice(start, end)
            result = result[tuple(slicer)]
        left = max(left, 0)
        right = max(right, 0)
        if left or right:
            pieces = []
            if left:
                shape = list(result.shape)
                shape[dim] = left
                pieces.append(result.new_full(shape, value))
            pieces.append(result)
            if right:
                shape = list(result.shape)
                shape[dim] = right
                pieces.append(result.new_full(shape, value))
            result = torch.cat(pieces, dim=dim)
        dim -= 1
    return result


def wrap_mps_bmm(original):
    """Non-contiguous bmm on MPS overflows past 2^32 elements (pytorch/pytorch#197636)."""

    def bmm(input, mat2, *args, **kwargs):
        if getattr(input, 'device', None) is not None and input.device.type == 'mps':
            if hasattr(input, 'is_contiguous') and not input.is_contiguous():
                input = input.contiguous()
            if hasattr(mat2, 'is_contiguous') and not mat2.is_contiguous():
                mat2 = mat2.contiguous()
        return original(input, mat2, *args, **kwargs)

    bmm._zone_patched = True
    bmm.__wrapped__ = original
    return bmm


def wrap_mps_baddbmm(original):
    """MPS baddbmm with beta=0 still reads its input, so NaN in it propagates.

    Comfy-Org/ComfyUI#15804 traces all-NaN attention on Apple Silicon to this.
    """

    def baddbmm(input, batch1, batch2, *args, **kwargs):
        if getattr(input, 'device', None) is not None and input.device.type == 'mps':
            if kwargs.get('beta', 1) == 0:
                input = torch.zeros_like(input)
                kwargs = dict(kwargs)
                kwargs['beta'] = 1
            if hasattr(batch1, 'is_contiguous') and not batch1.is_contiguous():
                batch1 = batch1.contiguous()
            if hasattr(batch2, 'is_contiguous') and not batch2.is_contiguous():
                batch2 = batch2.contiguous()
        return original(input, batch1, batch2, *args, **kwargs)

    baddbmm._zone_patched = True
    baddbmm.__wrapped__ = original
    return baddbmm


def install_mps_kernel_workarounds() -> None:
    if not torch.backends.mps.is_available():
        return
    if getattr(torch.nn.functional.pad, '_zone_patched', False):
        return
    original_pad = torch.nn.functional.pad

    def pad(input, pad, mode='constant', value=None):
        if should_rewrite_mps_pad(input, pad, mode):
            return constant_pad_with_cat(input, pad, 0 if value is None else value)
        return original_pad(input, pad, mode=mode, value=value)

    pad._zone_patched = True
    pad.__wrapped__ = original_pad
    torch.nn.functional.pad = pad
    torch.bmm = wrap_mps_bmm(torch.bmm)
    torch.Tensor.bmm = wrap_mps_bmm(torch.Tensor.bmm)
    torch.baddbmm = wrap_mps_baddbmm(torch.baddbmm)
    torch.Tensor.baddbmm = wrap_mps_baddbmm(torch.Tensor.baddbmm)
    logging.info('Zone LoRA: MPS 5D pad, bmm, and baddbmm workarounds installed')


_FLOAT8_DTYPES = (torch.float8_e4m3fn, torch.float8_e5m2)


def wrap_mps_fp8_dequant(original):
    """MPS can store float8 but cannot cast it (TypeError on x.to(fp16))."""

    def dequantize_per_tensor_fp8(x, scale, output_type=torch.bfloat16, *args, **kwargs):
        device = getattr(x, 'device', None)
        if device is not None and device.type == 'mps':
            x = x.to('cpu')
            if getattr(scale, 'device', None) is not None and scale.device.type == 'mps':
                scale = scale.to('cpu')
            result = original(x, scale, output_type, *args, **kwargs)
            return result.to(device)
        return original(x, scale, output_type, *args, **kwargs)

    dequantize_per_tensor_fp8._zone_patched = True
    dequantize_per_tensor_fp8.__wrapped__ = original
    return dequantize_per_tensor_fp8


def install_mps_fp8_dequant() -> None:
    try:
        import comfy_kitchen.backends.eager as eager
        import comfy_kitchen.backends.eager.quantization as quantization
    except ImportError:
        return
    if getattr(eager.dequantize_per_tensor_fp8, '_zone_patched', False):
        return
    wrapped = wrap_mps_fp8_dequant(eager.dequantize_per_tensor_fp8)
    eager.dequantize_per_tensor_fp8 = wrapped
    quantization.dequantize_per_tensor_fp8 = wrapped
    logging.info('Zone LoRA: FP8 dequant on MPS runs on CPU')


def densify_tensor(tensor, dtype=torch.float16):
    if tensor is None:
        return None
    dequantize = getattr(tensor, 'dequantize', None)
    if callable(dequantize):
        qdata = getattr(tensor, '_qdata', tensor)
        device = getattr(qdata, 'device', None)
        if device is not None and device.type == 'mps':
            tensor = tensor.to('cpu')
            dequantize = tensor.dequantize
        return dequantize().detach().to(dtype=dtype)
    qdata = getattr(tensor, '_qdata', None)
    if qdata is not None:
        device = qdata.device
        if device.type == 'mps':
            qdata = qdata.to('cpu')
        dense = qdata.detach().to(dtype=dtype)
        scale = getattr(getattr(tensor, '_params', None), 'scale', None)
        if scale is not None:
            dense = dense * scale.to(device=dense.device, dtype=dtype)
        return dense
    tensor_dtype = getattr(tensor, 'dtype', None)
    if tensor_dtype in _FLOAT8_DTYPES:
        device = tensor.device
        if device.type == 'mps':
            tensor = tensor.to('cpu')
        return tensor.detach().to(dtype=dtype)
    return tensor


def materialize_quantized_to_float16(model, dtype=torch.float16) -> int:
    """Metal has no float8 kernels, so quantized weights must become dense fp16."""
    converted = 0
    root = getattr(model, 'model', model)
    for module in root.modules():
        for name, param in list(module._parameters.items()):
            dense = densify_tensor(param, dtype)
            if dense is param or dense is None:
                continue
            module._parameters[name] = torch.nn.Parameter(
                dense, requires_grad=bool(getattr(param, 'requires_grad', False))
            )
            converted += 1
        for name, buf in list(module._buffers.items()):
            dense = densify_tensor(buf, dtype)
            if dense is buf or dense is None:
                continue
            module._buffers[name] = dense
            converted += 1
    return converted


def is_wan_14b_path(unet_path) -> bool:
    path = str(unet_path).lower()
    if 'wan' not in path:
        return False
    return '14b' in path or 'high_noise' in path or 'low_noise' in path


def wan_cpu_model_options(unet_path, model_options=None) -> dict:
    """Wan sampling on Metal silently corrupts every generated frame.

    Comfy-Org/ComfyUI#15793 and #16644 reproduce it with Wan 2.2 TI2V 5B.
    """
    options = dict(model_options or {})
    path = str(unet_path).lower()
    if 'wan' not in path:
        return options
    if not torch.backends.mps.is_available():
        return options
    if is_wan_14b_path(path):
        return options
    cpu = torch.device('cpu')
    options['load_device'] = cpu
    options['offload_device'] = cpu
    return options


def install_wan_mps_cpu_unet() -> None:
    try:
        import comfy.sd
    except ImportError:
        logging.warning('Zone LoRA: comfy.sd not importable, Wan CPU load patch skipped')
        return
    if getattr(comfy.sd.load_diffusion_model, '_zone_patched', False):
        return
    original = comfy.sd.load_diffusion_model

    def load_diffusion_model(unet_path, model_options=None, disable_dynamic=False):
        options = wan_cpu_model_options(unet_path, model_options)
        if is_wan_14b_path(unet_path) and torch.backends.mps.is_available():
            cpu = torch.device('cpu')
            options['load_device'] = cpu
            options['offload_device'] = cpu
            logging.info('Zone LoRA: Wan 14B FP8 loads on CPU then materializes fp16')
            patcher = original(unet_path, model_options=options, disable_dynamic=True)
            converted = materialize_quantized_to_float16(patcher)
            patcher.load_device = torch.device('mps')
            patcher.offload_device = cpu
            patcher.size = 0
            logging.info('Zone LoRA: materialized %s 14B tensors to fp16 for Metal', converted)
            return patcher
        if options.get('load_device') == torch.device('cpu'):
            logging.info('Zone LoRA: Wan DiT loads on CPU; MPS kernels corrupt video frames')
        return original(unet_path, model_options=options, disable_dynamic=disable_dynamic)

    load_diffusion_model._zone_patched = True
    load_diffusion_model.__wrapped__ = original
    comfy.sd.load_diffusion_model = load_diffusion_model
    logging.info('Zone LoRA: Wan DiT on MPS loads on CPU')


def is_wan_vae_state_dict(sd) -> bool:
    return isinstance(sd, dict) and 'decoder.middle.0.residual.0.gamma' in sd


def wan_cpu_vae_device(sd, device=None):
    """A precaution: Comfy-Org/ComfyUI#15793 and #16644 both rule the Wan VAE out on Metal."""
    if device is not None:
        return device
    if not torch.backends.mps.is_available():
        return None
    if not is_wan_vae_state_dict(sd):
        return None
    return torch.device('cpu')


def install_wan_mps_cpu_vae() -> None:
    try:
        import comfy.sd
    except ImportError:
        logging.warning('Zone LoRA: comfy.sd not importable, Wan VAE CPU load patch skipped')
        return
    if getattr(comfy.sd.VAE.__init__, '_zone_patched', False):
        return
    original = comfy.sd.VAE.__init__

    def init(self, sd=None, device=None, config=None, dtype=None, metadata=None):
        chosen = wan_cpu_vae_device(sd, device)
        if chosen is not None:
            device = chosen
            logging.info('Zone LoRA: Wan VAE loads on CPU; MPS 3D conv corrupts video frames')
        return original(self, sd=sd, device=device, config=config, dtype=dtype, metadata=metadata)

    init._zone_patched = True
    init.__wrapped__ = original
    comfy.sd.VAE.__init__ = init
    logging.info('Zone LoRA: Wan VAE on MPS loads on CPU')


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
