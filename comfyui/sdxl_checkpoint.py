"""Diffusers SDXL to original single-file conversion.

Maps vendored from huggingface/diffusers v0.32.2
scripts/convert_diffusers_to_original_sdxl.py
"""

from __future__ import annotations

import os
import re
from pathlib import Path
from typing import Any

import torch
from safetensors.torch import save_file

unet_conversion_map = [
    ('time_embed.0.weight', 'time_embedding.linear_1.weight'),
    ('time_embed.0.bias', 'time_embedding.linear_1.bias'),
    ('time_embed.2.weight', 'time_embedding.linear_2.weight'),
    ('time_embed.2.bias', 'time_embedding.linear_2.bias'),
    ('input_blocks.0.0.weight', 'conv_in.weight'),
    ('input_blocks.0.0.bias', 'conv_in.bias'),
    ('out.0.weight', 'conv_norm_out.weight'),
    ('out.0.bias', 'conv_norm_out.bias'),
    ('out.2.weight', 'conv_out.weight'),
    ('out.2.bias', 'conv_out.bias'),
    ('label_emb.0.0.weight', 'add_embedding.linear_1.weight'),
    ('label_emb.0.0.bias', 'add_embedding.linear_1.bias'),
    ('label_emb.0.2.weight', 'add_embedding.linear_2.weight'),
    ('label_emb.0.2.bias', 'add_embedding.linear_2.bias'),
]

unet_conversion_map_resnet = [
    ('in_layers.0', 'norm1'),
    ('in_layers.2', 'conv1'),
    ('out_layers.0', 'norm2'),
    ('out_layers.3', 'conv2'),
    ('emb_layers.1', 'time_emb_proj'),
    ('skip_connection', 'conv_shortcut'),
]

unet_conversion_map_layer = []
for i in range(3):
    for j in range(2):
        hf_down_res_prefix = f'down_blocks.{i}.resnets.{j}.'
        sd_down_res_prefix = f'input_blocks.{3 * i + j + 1}.0.'
        unet_conversion_map_layer.append((sd_down_res_prefix, hf_down_res_prefix))
        if i > 0:
            hf_down_atn_prefix = f'down_blocks.{i}.attentions.{j}.'
            sd_down_atn_prefix = f'input_blocks.{3 * i + j + 1}.1.'
            unet_conversion_map_layer.append((sd_down_atn_prefix, hf_down_atn_prefix))

    for j in range(4):
        hf_up_res_prefix = f'up_blocks.{i}.resnets.{j}.'
        sd_up_res_prefix = f'output_blocks.{3 * i + j}.0.'
        unet_conversion_map_layer.append((sd_up_res_prefix, hf_up_res_prefix))
        if i < 2:
            hf_up_atn_prefix = f'up_blocks.{i}.attentions.{j}.'
            sd_up_atn_prefix = f'output_blocks.{3 * i + j}.1.'
            unet_conversion_map_layer.append((sd_up_atn_prefix, hf_up_atn_prefix))

    if i < 3:
        hf_downsample_prefix = f'down_blocks.{i}.downsamplers.0.conv.'
        sd_downsample_prefix = f'input_blocks.{3 * (i + 1)}.0.op.'
        unet_conversion_map_layer.append((sd_downsample_prefix, hf_downsample_prefix))
        hf_upsample_prefix = f'up_blocks.{i}.upsamplers.0.'
        sd_upsample_prefix = f'output_blocks.{3 * i + 2}.{1 if i == 0 else 2}.'
        unet_conversion_map_layer.append((sd_upsample_prefix, hf_upsample_prefix))

unet_conversion_map_layer.append(('output_blocks.2.2.conv.', 'output_blocks.2.1.conv.'))
unet_conversion_map_layer.append(('middle_block.1.', 'mid_block.attentions.0.'))
for j in range(2):
    unet_conversion_map_layer.append((f'middle_block.{2 * j}.', f'mid_block.resnets.{j}.'))

vae_conversion_map = [
    ('nin_shortcut', 'conv_shortcut'),
    ('norm_out', 'conv_norm_out'),
    ('mid.attn_1.', 'mid_block.attentions.0.'),
]

for i in range(4):
    for j in range(2):
        vae_conversion_map.append(
            (f'encoder.down.{i}.block.{j}.', f'encoder.down_blocks.{i}.resnets.{j}.')
        )
    if i < 3:
        vae_conversion_map.append(
            (f'down.{i}.downsample.', f'down_blocks.{i}.downsamplers.0.')
        )
        vae_conversion_map.append(
            (f'up.{3 - i}.upsample.', f'up_blocks.{i}.upsamplers.0.')
        )
    for j in range(3):
        vae_conversion_map.append(
            (f'decoder.up.{3 - i}.block.{j}.', f'decoder.up_blocks.{i}.resnets.{j}.')
        )

for i in range(2):
    vae_conversion_map.append((f'mid.block_{i + 1}.', f'mid_block.resnets.{i}.'))

vae_conversion_map_attn = [
    ('norm.', 'group_norm.'),
    ('q.', 'to_q.'),
    ('k.', 'to_k.'),
    ('v.', 'to_v.'),
    ('proj_out.', 'to_out.0.'),
]

textenc_conversion_lst = [
    ('transformer.resblocks.', 'text_model.encoder.layers.'),
    ('ln_1', 'layer_norm1'),
    ('ln_2', 'layer_norm2'),
    ('.c_fc.', '.fc1.'),
    ('.c_proj.', '.fc2.'),
    ('.attn', '.self_attn'),
    ('ln_final.', 'text_model.final_layer_norm.'),
    ('token_embedding.weight', 'text_model.embeddings.token_embedding.weight'),
    ('positional_embedding', 'text_model.embeddings.position_embedding.weight'),
]
protected = {re.escape(item[1]): item[0] for item in textenc_conversion_lst}
textenc_pattern = re.compile('|'.join(protected.keys()))
code2idx = {'q': 0, 'k': 1, 'v': 2}


def convert_unet_state_dict(unet_state_dict: dict[str, Any]) -> dict[str, Any]:
    mapping = {key: key for key in unet_state_dict}
    for sd_name, hf_name in unet_conversion_map:
        mapping[hf_name] = sd_name
    for key, value in mapping.items():
        if 'resnets' in key:
            for sd_part, hf_part in unet_conversion_map_resnet:
                value = value.replace(hf_part, sd_part)
            mapping[key] = value
    for key, value in mapping.items():
        for sd_part, hf_part in unet_conversion_map_layer:
            value = value.replace(hf_part, sd_part)
        mapping[key] = value
    return {sd_name: unet_state_dict[hf_name] for hf_name, sd_name in mapping.items()}


def reshape_weight_for_sd(weight: torch.Tensor) -> torch.Tensor:
    if weight.ndim == 1:
        return weight
    return weight.reshape(*weight.shape, 1, 1)


def convert_vae_state_dict(vae_state_dict: dict[str, Any]) -> dict[str, Any]:
    mapping = {key: key for key in vae_state_dict}
    for key, value in mapping.items():
        for sd_part, hf_part in vae_conversion_map:
            value = value.replace(hf_part, sd_part)
        mapping[key] = value
    for key, value in mapping.items():
        if 'attentions' in key:
            for sd_part, hf_part in vae_conversion_map_attn:
                value = value.replace(hf_part, sd_part)
            mapping[key] = value
    converted = {value: vae_state_dict[key] for key, value in mapping.items()}
    for key, weight in list(converted.items()):
        for name in ('q', 'k', 'v', 'proj_out'):
            if f'mid.attn_1.{name}.weight' in key:
                converted[key] = reshape_weight_for_sd(weight)
    return converted


def convert_openclip_text_enc_state_dict(text_enc_dict: dict[str, Any]) -> dict[str, Any]:
    converted: dict[str, Any] = {}
    capture_qkv_weight: dict[str, list[Any]] = {}
    capture_qkv_bias: dict[str, list[Any]] = {}
    for key, value in text_enc_dict.items():
        if key.endswith(('.self_attn.q_proj.weight', '.self_attn.k_proj.weight', '.self_attn.v_proj.weight')):
            prefix = key[: -len('.q_proj.weight')]
            code = key[-len('q_proj.weight')]
            capture_qkv_weight.setdefault(prefix, [None, None, None])
            capture_qkv_weight[prefix][code2idx[code]] = value
            continue
        if key.endswith(('.self_attn.q_proj.bias', '.self_attn.k_proj.bias', '.self_attn.v_proj.bias')):
            prefix = key[: -len('.q_proj.bias')]
            code = key[-len('q_proj.bias')]
            capture_qkv_bias.setdefault(prefix, [None, None, None])
            capture_qkv_bias[prefix][code2idx[code]] = value
            continue
        relabelled = textenc_pattern.sub(lambda match: protected[re.escape(match.group(0))], key)
        converted[relabelled] = value
    for prefix, tensors in capture_qkv_weight.items():
        if None in tensors:
            raise RuntimeError('text encoder qkv weight is incomplete')
        relabelled = textenc_pattern.sub(lambda match: protected[re.escape(match.group(0))], prefix)
        converted[relabelled + '.in_proj_weight'] = torch.cat(tensors)
    for prefix, tensors in capture_qkv_bias.items():
        if None in tensors:
            raise RuntimeError('text encoder qkv bias is incomplete')
        relabelled = textenc_pattern.sub(lambda match: protected[re.escape(match.group(0))], prefix)
        converted[relabelled + '.in_proj_bias'] = torch.cat(tensors)
    return converted


def convert_openai_text_enc_state_dict(text_enc_dict: dict[str, Any]) -> dict[str, Any]:
    return text_enc_dict


def unwrap_state(model: Any) -> dict[str, Any]:
    if hasattr(model, 'get_base_model'):
        model = model.get_base_model()
    return model.state_dict()


def convert_pipeline_state(
    unet_state: dict[str, Any],
    vae_state: dict[str, Any],
    text_encoder_state: dict[str, Any],
    text_encoder_2_state: dict[str, Any],
) -> dict[str, Any]:
    unet_state = {
        'model.diffusion_model.' + key: value
        for key, value in convert_unet_state_dict(unet_state).items()
    }
    vae_state = {
        'first_stage_model.' + key: value
        for key, value in convert_vae_state_dict(vae_state).items()
    }
    text_encoder_state = {
        'conditioner.embedders.0.transformer.' + key: value
        for key, value in convert_openai_text_enc_state_dict(text_encoder_state).items()
    }
    text_encoder_2_state = {
        'conditioner.embedders.1.model.' + key: value
        for key, value in convert_openclip_text_enc_state_dict(text_encoder_2_state).items()
    }
    projection_key = 'conditioner.embedders.1.model.text_projection.weight'
    projection = text_encoder_2_state.pop(projection_key, None)
    if projection is None:
        projection = text_encoder_2_state.pop('conditioner.embedders.1.model.text_projection')
    text_encoder_2_state['conditioner.embedders.1.model.text_projection'] = (
        projection.T.contiguous()
    )
    return {**unet_state, **vae_state, **text_encoder_state, **text_encoder_2_state}


def pipeline_to_original(unet: Any, vae: Any, text_encoder: Any, text_encoder_2: Any) -> dict[str, Any]:
    return convert_pipeline_state(
        unwrap_state(unet),
        unwrap_state(vae),
        unwrap_state(text_encoder),
        unwrap_state(text_encoder_2),
    )


def as_checkpoint_tensors(state: dict[str, Any]) -> dict[str, Any]:
    converted = {}
    for key, value in state.items():
        tensor = value.detach().cpu().contiguous()
        if tensor.is_floating_point():
            tensor = tensor.half()
        converted[key] = tensor
    return converted


def save_original_checkpoint(path: Path, state: dict[str, Any]) -> None:
    path = Path(path)
    temporary = path.with_name(path.name + '.tmp')
    save_file(as_checkpoint_tensors(state), str(temporary))
    os.replace(temporary, path)
