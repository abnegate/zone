from __future__ import annotations

import json
import logging
from pathlib import Path

import folder_paths
import node_helpers
import numpy as np
import safetensors.torch
import torch
from PIL import Image
from comfy.cli_args import PerformanceFeature, args
from comfy.weight_adapter import adapter_maps
from comfy.weight_adapter import adapters as adapter_classes
from comfy.weight_adapter.bypass import BypassInjectionManager
from comfy_api.latest import io
from comfy_extras.nodes_train import (
    TrainGuider,
    TrainSampler,
    _create_loss_function,
    _create_optimizer,
    _load_existing_lora,
    _prepare_latents_and_count,
    _process_conditioning,
    _process_latents_standard_mode,
    _run_training_loop,
    _validate_and_expand_conditioning,
    find_modules_at_depth,
    patch,
    unpatch,
)
import comfy.model_management

from .inference_hooks import install_all, prepare_frozen_weights, wrap_early_frozen
from .train_config import (
    load_config,
    lora_alpha,
    trains,
)


def error_scale(sigmas, sample, sigma_floor: float):
    """Flow matching makes the x0 error exactly sigma times the velocity error.

    Measuring the x0 error therefore weights a sample by sigma squared, so the
    noisy end of the schedule — where only colour and layout are recoverable —
    dominates and the clean end that carries a subject's shape counts for almost
    nothing. Training and every probe have to divide it out the same way, or a
    probe ranks adapters by an objective the trainer never optimised.
    """
    if not sigma_floor:
        return 1.0
    shape = (-1,) + (1,) * (sample.ndim - 1)
    return sigmas.detach().float().reshape(shape).clamp(min=sigma_floor)


class ZoneTrainSampler(TrainSampler):
    def __init__(self, *args, sigma_floor=0.0, **kwargs):
        super().__init__(*args, **kwargs)
        self.sigma_floor = sigma_floor

    def error_scale(self, sigmas, sample):
        return error_scale(sigmas, sample, self.sigma_floor)

    def fwd_bwd(
        self,
        model_wrap,
        batch_sigmas,
        batch_noise,
        batch_latent,
        cond,
        indicies,
        extra_args,
        dataset_size,
        bwd=True,
    ):
        from comfy_extras.nodes_train import make_batch_extra_option_dict

        xt = model_wrap.inner_model.model_sampling.noise_scaling(
            batch_sigmas, batch_noise, batch_latent, False
        )
        x0 = model_wrap.inner_model.model_sampling.noise_scaling(
            torch.zeros_like(batch_sigmas),
            torch.zeros_like(batch_noise),
            batch_latent,
            False,
        )
        model_wrap.conds['positive'] = [cond[i] for i in indicies]
        batch_extra_args = make_batch_extra_option_dict(
            extra_args, indicies, full_size=dataset_size
        )
        with torch.inference_mode(False):
            xt = xt.detach().clone()
            batch_sigmas = batch_sigmas.detach().clone()
        with torch.autocast(xt.device.type, dtype=self.training_dtype):
            x0_pred = model_wrap(xt, batch_sigmas, **batch_extra_args)
            scale = self.error_scale(batch_sigmas, x0_pred)
            loss = self.loss_fn(x0_pred.float() / scale, x0.float() / scale)
        if bwd:
            bwd_loss = loss / self.grad_acc
            if self.grad_scaler is not None:
                self.grad_scaler.scale(bwd_loss).backward()
            else:
                bwd_loss.backward()
        return loss


def snapshot(lora_sd: dict, dtype) -> dict:
    return {
        key: value.detach().to(dtype).contiguous().cpu() for key, value in lora_sd.items()
    }


def write_lora(lora_sd: dict, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    safetensors.torch.save_file(lora_sd, str(destination))
    logging.info('Zone LoRA: wrote %s (%s tensors)', destination, len(lora_sd))


def reseed(adapter) -> None:
    """Let both LoRA matrices learn.

    Comfy seeds lora_up with kaiming noise and lora_down at zero. The product is
    zero either way, but the gradient is not: lora_down gets a signal amplified
    by lora_up, while lora_up gets grad @ lora_down.T, which is exactly zero on
    the first step and stays negligible after. lora_up therefore keeps its random
    values and the adapter can only ever write into that fixed random subspace.
    Swapping the two is what every other LoRA trainer does and lets rank 8 mean
    rank 8.
    """
    with torch.no_grad():
        torch.nn.init.kaiming_uniform_(adapter.lora_down.weight, a=5**0.5)
        torch.nn.init.constant_(adapter.lora_up.weight, 0.0)


def setup_identity_lora(mp, existing_weights, algorithm, lora_dtype, rank):
    settings = load_config()
    alpha = lora_alpha(rank, settings)
    adapter_cls = adapter_maps[algorithm]
    lora_sd = {}
    trained = []
    bypass_manager = BypassInjectionManager()
    for name, module in mp.model.named_modules():
        if not trains(name, settings):
            continue
        if not hasattr(module, 'weight_function') or module.weight is None:
            continue
        if getattr(module.weight, 'ndim', 0) < 2:
            continue
        existing = None
        for adapter_cls_candidate in adapter_classes:
            existing = adapter_cls_candidate.load(name, existing_weights, alpha, None)
            if existing is not None:
                break
        if existing is not None:
            train_adapter = existing.to_train().to(lora_dtype)
        else:
            train_adapter = adapter_cls.create_train(
                module.weight, rank=rank, alpha=alpha
            ).to(lora_dtype)
            reseed(train_adapter)
        train_adapter.train()
        for param_name, parameter in train_adapter.named_parameters():
            parameter.requires_grad_(param_name != 'alpha')
            lora_sd[f'{name}.{param_name}'] = parameter
        trained.append(train_adapter)
        bypass_manager.add_adapter(f'{name}.weight', trained[-1], strength=1.0)
    minimum = int(settings.get('min_adapters', 16))
    if len(trained) < minimum:
        raise ValueError(
            f'only {len(trained)} transformer LoRA adapters; identity training needs at least {minimum}'
        )
    logging.info('Zone LoRA: %s adapters alpha=%s rank=%s', len(trained), alpha, rank)
    return lora_sd, trained, bypass_manager


def square(image: Image.Image, resolution: int) -> Image.Image:
    """Crop to the centre square rather than padding to it.

    Padding a 16:9 photo to a square leaves 44% of every training image a flat
    border, and a border that appears in all of them is exactly what an identity
    adapter learns first.
    """
    side = min(image.width, image.height)
    left = (image.width - side) // 2
    top = (image.height - side) // 2
    cropped = image.crop((left, top, left + side, top + side))
    return cropped.resize((resolution, resolution), Image.LANCZOS)


class ZoneLoadTrainFolder(io.ComfyNode):
    @classmethod
    def define_schema(cls):
        return io.Schema(
            node_id='ZoneLoadTrainFolder',
            display_name='Zone Load Train Folder',
            category='zone/training',
            inputs=[
                io.String.Input('folder', default='zone-train'),
                io.String.Input('captions_json', default='{}'),
                io.Int.Input('resolution', default=512, min=64, max=2048),
            ],
            outputs=[
                io.Image.Output(display_name='images', is_output_list=True),
                io.String.Output(display_name='texts', is_output_list=True),
            ],
        )

    @classmethod
    def execute(cls, folder, captions_json, resolution):
        root = Path(folder_paths.get_input_directory()) / folder
        captions = json.loads(captions_json or '{}')
        images = []
        texts = []
        for png in sorted(root.glob('*.png')):
            image = square(Image.open(png).convert('RGB'), resolution)
            array = np.array(image).astype(np.float32) / 255.0
            images.append(torch.from_numpy(array)[None,])
            caption_path = png.with_suffix('.txt')
            texts.append(
                captions.get(png.name)
                or (caption_path.read_text().strip() if caption_path.is_file() else '')
            )
        if not images:
            raise ValueError(f'no training pngs in {root}')
        return io.NodeOutput(images, texts)


class ZoneTrainLoRA(io.ComfyNode):
    @classmethod
    def define_schema(cls):
        settings = load_config()
        return io.Schema(
            node_id='ZoneTrainLoRA',
            display_name='Zone Train LoRA',
            category='zone/training',
            is_experimental=True,
            is_input_list=True,
            is_output_node=True,
            inputs=[
                io.Model.Input('model'),
                io.Latent.Input('latents'),
                io.Conditioning.Input('positive'),
                io.Int.Input('steps', default=int(settings['min_steps']), min=1, max=100000),
                io.Float.Input(
                    'learning_rate',
                    default=float(settings['learning_rate']),
                    min=0.0000001,
                    max=1.0,
                    step=0.0000001,
                ),
                io.Int.Input('rank', default=int(settings['rank']), min=1, max=128),
                io.Int.Input('seed', default=int(settings['seed']), min=0, max=0xFFFFFFFFFFFFFFFF),
                io.Combo.Input(
                    'training_dtype',
                    options=['bf16', 'fp32', 'none'],
                    default=settings['training_dtype'],
                ),
                io.Combo.Input(
                    'lora_dtype',
                    options=['bf16', 'fp32'],
                    default=settings['lora_dtype'],
                ),
                io.Boolean.Input(
                    'gradient_checkpointing',
                    default=bool(settings['gradient_checkpointing']),
                ),
                io.Int.Input(
                    'checkpoint_depth',
                    default=int(settings['checkpoint_depth']),
                    min=1,
                    max=5,
                ),
                io.Boolean.Input('bypass_mode', default=bool(settings['bypass_mode'])),
                io.String.Input('save_name', default='zone_lora'),
            ],
            outputs=[
                io.Custom('LORA_MODEL').Output(display_name='lora'),
                io.Int.Output(display_name='steps'),
            ],
        )

    @classmethod
    def execute(
        cls,
        model,
        latents,
        positive,
        steps,
        learning_rate,
        rank,
        seed,
        training_dtype,
        lora_dtype,
        gradient_checkpointing,
        checkpoint_depth,
        bypass_mode,
        save_name,
    ):
        install_all()
        model = model[0]
        steps = steps[0]
        learning_rate = learning_rate[0]
        rank = rank[0]
        seed = seed[0]
        training_dtype = training_dtype[0]
        lora_dtype = lora_dtype[0]
        gradient_checkpointing = gradient_checkpointing[0]
        checkpoint_depth = checkpoint_depth[0]
        bypass_mode = bypass_mode[0]
        save_name = save_name[0]
        latents = _process_latents_standard_mode(latents)
        positive = _process_conditioning(positive)

        with torch.inference_mode(False):
            mp = model
            lora_dtype_t = node_helpers.string_to_torch_dtype(lora_dtype)
            use_grad_scaler = False
            if training_dtype != 'none':
                dtype = node_helpers.string_to_torch_dtype(training_dtype)
                mp.set_model_compute_dtype(dtype)
            else:
                model_dtype = mp.model.get_dtype()
                if model_dtype == torch.float16:
                    dtype = torch.float16
                    if lora_dtype_t != torch.bfloat16:
                        use_grad_scaler = True
                    if PerformanceFeature.Fp16Accumulation in args.fast:
                        logging.warning(
                            'FP16 model with fp16_accumulation can NaN during training'
                        )
                else:
                    dtype = torch.bfloat16
            latents, num_images, multi_res = _prepare_latents_and_count(
                latents, dtype, False
            )
            positive = _validate_and_expand_conditioning(positive, num_images, False)
            mp.model.requires_grad_(False).train()
            existing_weights, existing_steps = _load_existing_lora('[None]')
            if not bypass_mode:
                raise ValueError('ZoneTrainLoRA requires bypass_mode for quantized checkpoints')
            lora_sd, trained, bypass_manager = setup_identity_lora(
                mp, existing_weights, 'LoRA', lora_dtype_t, rank
            )
            trainable = [value for value in lora_sd.values() if getattr(value, 'requires_grad', False)]
            optimizer = _create_optimizer('AdamW', trainable, learning_rate)
            criterion = _create_loss_function('MSE')
            prepare_frozen_weights(mp.model)
            frozen_restores = wrap_early_frozen(mp.model)
            injections = bypass_manager.create_injections(mp.model)
            for injection in injections:
                injection.inject(mp)
            if gradient_checkpointing:
                modules_to_patch = find_modules_at_depth(
                    mp.model.diffusion_model, depth=checkpoint_depth
                )
                for module in modules_to_patch:
                    patch(module)
            logging.info('Zone LoRA: training %s steps on the loaded checkpoint', steps)
            settings = load_config()
            losses = []
            stem = Path(save_name).name.removesuffix('.safetensors')
            output_dir = Path(folder_paths.get_output_directory()) / 'loras'
            every = int(settings.get('checkpoint_every', 0))

            def loss_callback(loss):
                losses.append(loss)
                if loss != loss:
                    raise RuntimeError('training loss became NaN')
                if len(losses) == 1 or len(losses) % 10 == 0:
                    logging.info('Zone LoRA step %s/%s loss=%s', len(losses), steps, f'{loss:.4f}')
                if every and len(losses) % every == 0 and len(losses) < steps:
                    write_lora(
                        snapshot(lora_sd, lora_dtype_t),
                        output_dir / f'{stem}-step{len(losses)}.safetensors',
                    )

            train_sampler = ZoneTrainSampler(
                criterion,
                optimizer,
                loss_callback=loss_callback,
                batch_size=1,
                grad_acc=1,
                total_steps=steps,
                seed=seed,
                training_dtype=dtype,
                use_grad_scaler=use_grad_scaler,
                sigma_floor=float(settings.get('sigma_floor', 0.0)),
            )
            guider = TrainGuider(mp, offloading=False)
            guider.set_conds(positive)
            try:
                comfy.model_management.in_training = True
                _run_training_loop(
                    guider, train_sampler, latents, num_images, seed, False, False
                )
            finally:
                comfy.model_management.in_training = False
                for injection in injections:
                    injection.eject(mp)
                for module in mp.model.modules():
                    unpatch(module)
                for module, original in frozen_restores:
                    module.forward = original
            lora_sd = snapshot(lora_sd, lora_dtype_t)
            write_lora(lora_sd, output_dir / f'{stem}.safetensors')
            del trained
            return io.NodeOutput(lora_sd, steps + existing_steps)
