"""CLIP Plus-Face IP-Adapter for SDXL. MPS-friendly; no insightface."""

from __future__ import annotations

import math
import os

import folder_paths
import torch
import torch.nn as nn
from comfy.ldm.modules.attention import optimized_attention
import comfy.model_management as model_management
import comfy.utils

PLUS_FACE = 'ip-adapter-plus-face_sdxl_vit-h.safetensors'


def register_ipadapter_folder() -> None:
    models = folder_paths.models_dir
    path = os.path.join(models, 'ipadapter')
    os.makedirs(path, exist_ok=True)
    current, extensions = folder_paths.folder_names_and_paths.get(
        'ipadapter', ([path], folder_paths.supported_pt_extensions)
    )
    if path not in current:
        current = list(current) + [path]
    folder_paths.folder_names_and_paths['ipadapter'] = (
        current,
        extensions,
    )


register_ipadapter_folder()


def _feed_forward(dim: int, mult: int = 4) -> nn.Sequential:
    inner = int(dim * mult)
    return nn.Sequential(
        nn.LayerNorm(dim),
        nn.Linear(dim, inner, bias=False),
        nn.GELU(),
        nn.Linear(inner, dim, bias=False),
    )


def _reshape(x: torch.Tensor, heads: int) -> torch.Tensor:
    batch, length, _ = x.shape
    x = x.view(batch, length, heads, -1)
    return x.transpose(1, 2)


class PerceiverAttention(nn.Module):
    def __init__(self, dim: int, dim_head: int = 64, heads: int = 8):
        super().__init__()
        inner = dim_head * heads
        self.dim_head = dim_head
        self.heads = heads
        self.norm1 = nn.LayerNorm(dim)
        self.norm2 = nn.LayerNorm(dim)
        self.to_q = nn.Linear(dim, inner, bias=False)
        self.to_kv = nn.Linear(dim, inner * 2, bias=False)
        self.to_out = nn.Linear(inner, dim, bias=False)

    def forward(self, x: torch.Tensor, latents: torch.Tensor) -> torch.Tensor:
        x = self.norm1(x)
        latents = self.norm2(latents)
        batch, length, _ = latents.shape
        query = _reshape(self.to_q(latents), self.heads)
        key, value = self.to_kv(torch.cat((x, latents), dim=-2)).chunk(2, dim=-1)
        key = _reshape(key, self.heads)
        value = _reshape(value, self.heads)
        scale = 1 / math.sqrt(math.sqrt(self.dim_head))
        weight = (query * scale) @ (key * scale).transpose(-2, -1)
        weight = torch.softmax(weight.float(), dim=-1).type(weight.dtype)
        out = weight @ value
        out = out.permute(0, 2, 1, 3).reshape(batch, length, -1)
        return self.to_out(out)


class Resampler(nn.Module):
    def __init__(
        self,
        dim: int = 1024,
        depth: int = 4,
        dim_head: int = 64,
        heads: int = 16,
        num_queries: int = 8,
        embedding_dim: int = 768,
        output_dim: int = 1024,
        ff_mult: int = 4,
    ):
        super().__init__()
        self.latents = nn.Parameter(torch.randn(1, num_queries, dim) / dim**0.5)
        self.proj_in = nn.Linear(embedding_dim, dim)
        self.proj_out = nn.Linear(dim, output_dim)
        self.norm_out = nn.LayerNorm(output_dim)
        self.layers = nn.ModuleList(
            nn.ModuleList(
                [
                    PerceiverAttention(dim=dim, dim_head=dim_head, heads=heads),
                    _feed_forward(dim, ff_mult),
                ]
            )
            for _ in range(depth)
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        latents = self.latents.repeat(x.size(0), 1, 1)
        x = self.proj_in(x)
        for attn, feed in self.layers:
            latents = attn(x, latents) + latents
            latents = feed(latents) + latents
        return self.norm_out(self.proj_out(latents))


class ToKV(nn.Module):
    def __init__(self, state: dict):
        super().__init__()
        self.to_kvs = nn.ModuleDict()
        for key, value in state.items():
            name = key.replace('.weight', '').replace('.', '_')
            layer = nn.Linear(value.shape[1], value.shape[0], bias=False)
            layer.weight.data = value
            self.to_kvs[name] = layer


class PlusFace(nn.Module):
    def __init__(self, ipadapter: dict, clip_dim: int):
        super().__init__()
        output_dim = ipadapter['ip_adapter']['1.to_k_ip.weight'].shape[1]
        tokens = 16
        self.image_proj_model = Resampler(
            dim=1280,
            depth=4,
            dim_head=64,
            heads=20,
            num_queries=tokens,
            embedding_dim=clip_dim,
            output_dim=output_dim,
            ff_mult=4,
        )
        self.image_proj_model.load_state_dict(ipadapter['image_proj'])
        self.ip_layers = ToKV(ipadapter['ip_adapter'])

    def get_multigpu_clone(self, _device):
        return self

    @torch.inference_mode()
    def embeds(self, cond: torch.Tensor, uncond: torch.Tensor):
        device = model_management.get_torch_device()
        return (
            self.image_proj_model(cond.to(device)),
            self.image_proj_model(uncond.to(device)),
        )


class Attn2Replace:
    def __init__(self, callback, **kwargs):
        self.callback = [callback]
        self.kwargs = [kwargs]

    def add(self, callback, **kwargs):
        self.callback.append(callback)
        self.kwargs.append(kwargs)

    def __call__(self, q, k, v, extra_options):
        dtype = q.dtype
        out = optimized_attention(q, k, v, extra_options['n_heads'])
        sigma = (
            extra_options['sigmas'].detach().cpu()[0].item()
            if 'sigmas' in extra_options
            else 999999999.9
        )
        for i, callback in enumerate(self.callback):
            start = self.kwargs[i]['sigma_start']
            end = self.kwargs[i]['sigma_end']
            if sigma <= start and sigma >= end:
                out = out + callback(out, q, k, v, extra_options, **self.kwargs[i])
        return out.to(dtype=dtype)


def ipadapter_attention(
    _out,
    q,
    k,
    v,
    extra_options,
    module_key='',
    ipadapter=None,
    weight=1.0,
    cond=None,
    uncond=None,
    sigma_start=0.0,
    sigma_end=1.0,
    **_kwargs,
):
    ipadapter = ipadapter.get_multigpu_clone(q.device)
    cond_or_uncond = extra_options['cond_or_uncond']
    batch_prompt = q.shape[0] // len(cond_or_uncond)
    k_key = module_key + '_to_k_ip'
    v_key = module_key + '_to_v_ip'
    k_cond = ipadapter.ip_layers.to_kvs[k_key](cond).repeat(batch_prompt, 1, 1)
    k_uncond = ipadapter.ip_layers.to_kvs[k_key](uncond).repeat(batch_prompt, 1, 1)
    v_cond = ipadapter.ip_layers.to_kvs[v_key](cond).repeat(batch_prompt, 1, 1)
    v_uncond = ipadapter.ip_layers.to_kvs[v_key](uncond).repeat(batch_prompt, 1, 1)
    ip_k = torch.cat([(k_cond, k_uncond)[i] for i in cond_or_uncond], dim=0)
    ip_v = torch.cat([(v_cond, v_uncond)[i] for i in cond_or_uncond], dim=0)
    out_ip = optimized_attention(q, ip_k, ip_v, extra_options['n_heads'])
    return (out_ip * weight).to(dtype=q.dtype)


def set_model_patch_replace(model, patch_kwargs, key):
    options = model.model_options['transformer_options'].copy()
    patches = options.get('patches_replace', {}).copy()
    attn2 = patches.get('attn2', {}).copy()
    if key not in attn2:
        attn2[key] = Attn2Replace(ipadapter_attention, **patch_kwargs)
    else:
        attn2[key].add(ipadapter_attention, **patch_kwargs)
    patches['attn2'] = attn2
    options['patches_replace'] = patches
    model.model_options['transformer_options'] = options


def load_plus_face(path: str) -> dict:
    model = comfy.utils.load_torch_file(path, safe_load=True)
    loaded = {'image_proj': {}, 'ip_adapter': {}}
    for key, value in model.items():
        if key.startswith('image_proj.'):
            loaded['image_proj'][key.removeprefix('image_proj.')] = value
        elif key.startswith('ip_adapter.'):
            loaded['ip_adapter'][key.removeprefix('ip_adapter.')] = value
        elif key.startswith('adapter_modules.'):
            loaded['ip_adapter'][key.removeprefix('adapter_modules.')] = value
    if not loaded['ip_adapter']:
        raise RuntimeError(f'invalid IP-Adapter weights: {path}')
    return loaded


def patch_sdxl(model, ipa, cond, uncond, weight: float):
    sigma_start = model.get_model_object('model_sampling').percent_to_sigma(0.0)
    sigma_end = model.get_model_object('model_sampling').percent_to_sigma(1.0)
    kwargs = {
        'ipadapter': ipa,
        'weight': weight,
        'cond': cond,
        'uncond': uncond,
        'sigma_start': sigma_start,
        'sigma_end': sigma_end,
    }
    number = 0
    for block_id in [4, 5, 7, 8]:
        indices = range(2) if block_id in [4, 5] else range(10)
        for index in indices:
            kwargs['module_key'] = str(number * 2 + 1)
            set_model_patch_replace(model, kwargs, ('input', block_id, index))
            number += 1
    for block_id in range(6):
        indices = range(2) if block_id in [3, 4, 5] else range(10)
        for index in indices:
            kwargs['module_key'] = str(number * 2 + 1)
            set_model_patch_replace(model, kwargs, ('output', block_id, index))
            number += 1
    for index in range(10):
        kwargs['module_key'] = str(number * 2 + 1)
        set_model_patch_replace(model, kwargs, ('middle', 1, index))
        number += 1


class ZoneIPAdapterFace:
    @classmethod
    def INPUT_TYPES(cls):
        names = folder_paths.get_filename_list('ipadapter')
        if not names:
            names = [PLUS_FACE]
        return {
            'required': {
                'model': ('MODEL',),
                'clip_vision': ('CLIP_VISION',),
                'image': ('IMAGE',),
                'ipadapter_name': (names,),
                'strength': (
                    'FLOAT',
                    {'default': 0.4, 'min': 0.0, 'max': 2.0, 'step': 0.01},
                ),
            }
        }

    RETURN_TYPES = ('MODEL',)
    FUNCTION = 'apply'
    CATEGORY = 'zone'

    def apply(self, model, clip_vision, image, ipadapter_name, strength):
        if strength == 0:
            return (model,)
        path = folder_paths.get_full_path_or_raise('ipadapter', ipadapter_name)
        loaded = load_plus_face(path)
        if 'latents' not in loaded['image_proj']:
            raise RuntimeError(
                'ZoneIPAdapterFace expects IP-Adapter Plus Face weights'
            )
        encoded = clip_vision.encode_image(image, crop=True)
        clip_embed = encoded.penultimate_hidden_states
        if clip_embed is None:
            clip_embed = encoded.image_embeds.unsqueeze(1)
        clip_uncond = torch.zeros_like(clip_embed)
        device = model_management.get_torch_device()
        dtype = model_management.unet_dtype()
        if dtype not in (torch.float32, torch.float16, torch.bfloat16):
            dtype = torch.float16 if model_management.should_use_fp16() else torch.float32
        ipa = PlusFace(loaded, clip_embed.shape[-1]).to(device, dtype=dtype)
        cond, uncond = ipa.embeds(clip_embed, clip_uncond)
        cond = cond.to(device, dtype=dtype)
        uncond = uncond.to(device, dtype=dtype)
        work = model.clone()
        patch_sdxl(work, ipa, cond, uncond, float(strength))
        return (work,)
