from __future__ import annotations

import json
import logging

import comfy.model_management
import comfy.samplers
import comfy_extras.nodes_custom_sampler
import torch
from comfy_api.latest import io
from comfy_extras.nodes_train import (
    TrainGuider,
    _prepare_latents_and_count,
    _process_conditioning,
    _process_latents_standard_mode,
    _validate_and_expand_conditioning,
    find_modules_at_depth,
    make_batch_extra_option_dict,
    patch,
    process_cond_list,
    unpatch,
)

from .inference_hooks import install_all, prepare_frozen_weights, wrap_early_frozen
from .train_config import load_config
from .train_node import error_scale, setup_identity_lora


class FixedBatchDescent(comfy.samplers.Sampler):
    """Descends on one unchanging batch, where any correct gradient has to lower the loss."""

    def __init__(
        self,
        parameters: list[torch.Tensor],
        adapters: list[torch.nn.Module],
        learning_rate: float,
        iterations: int,
        percent: float,
        seed: int,
        training_dtype: torch.dtype,
    ) -> None:
        self.parameters = parameters
        self.adapters = adapters
        self.iterations = iterations
        self.percent = percent
        self.seed = seed
        self.training_dtype = training_dtype
        self.sigma_floor = float(load_config().get('sigma_floor', 0.0))
        self.optimizer = torch.optim.AdamW(parameters, lr=learning_rate)
        self.losses: list[float] = []
        self.gradient_norms: list[float] = []
        self.adapter_norms: list[float] = []
        self.nonfinite: list[str] = []

    def sample(
        self,
        model_wrap,
        sigmas,
        extra_args,
        callback,
        noise,
        latent_image=None,
        denoise_mask=None,
        disable_pbar=False,
    ):
        model_wrap.conds = process_cond_list(model_wrap.conds)
        cond = model_wrap.conds['positive']
        sampling = model_wrap.inner_model.model_sampling
        latent = latent_image[0:1]
        sigma = torch.tensor([sampling.percent_to_sigma(self.percent)]).to(latent.device)
        generator = torch.Generator().manual_seed(self.seed)
        batch_noise = torch.randn(latent.shape, generator=generator).to(latent)
        extra = make_batch_extra_option_dict(extra_args, [0], full_size=sigmas.size(0))
        model_wrap.conds['positive'] = [cond[0]]
        with torch.inference_mode(False):
            xt = sampling.noise_scaling(sigma, batch_noise, latent, False).detach().clone()
            x0 = latent.detach().clone()
            sigma = sigma.detach().clone()
        for _ in range(self.iterations):
            with torch.autocast(xt.device.type, dtype=self.training_dtype):
                x0_pred = model_wrap(xt, sigma, **extra)
                scale = error_scale(sigma, x0_pred, self.sigma_floor)
                loss = torch.nn.functional.mse_loss(
                    x0_pred.float() / scale, x0.float() / scale
                )
            self.optimizer.zero_grad()
            loss.backward()
            broken = [
                name
                for name, value in (
                    ('loss', loss),
                    *((f'grad[{i}]', p.grad) for i, p in enumerate(self.parameters)),
                )
                if value is not None and not bool(torch.isfinite(value).all())
            ]
            if broken:
                self.nonfinite = broken[:4]
                self.losses.append(float('nan'))
                break
            total = sum(
                float(p.grad.float().pow(2).sum()) for p in self.parameters if p.grad is not None
            )
            self.losses.append(float(loss))
            self.gradient_norms.append(total**0.5)
            self.adapter_norms.append(
                sum(
                    float(w.detach().float().pow(2).sum())
                    for adapter in self.adapters
                    for w in (adapter.lora_up.weight, adapter.lora_down.weight)
                )
                ** 0.5
            )
            self.optimizer.step()
        return torch.zeros_like(latent_image)


class ZoneProbeGradient(io.ComfyNode):
    @classmethod
    def define_schema(cls):
        return io.Schema(
            node_id='ZoneProbeGradient',
            display_name='Zone Probe Gradient',
            category='zone/training',
            is_input_list=True,
            inputs=[
                io.Model.Input('model'),
                io.Latent.Input('latents'),
                io.Conditioning.Input('positive'),
                io.Float.Input('learning_rate', default=0.01, min=0.0, max=10.0, step=0.0001),
                io.Int.Input('iterations', default=20, min=1, max=500),
                io.Float.Input('percent', default=0.5, min=0.0, max=1.0, step=0.01),
                io.Int.Input('rank', default=8, min=1, max=128),
                io.Int.Input('seed', default=0, min=0, max=0xFFFFFFFF),
                io.Boolean.Input('gradient_checkpointing', default=True),
            ],
            outputs=[io.String.Output(display_name='report')],
        )

    @classmethod
    def execute(
        cls, model, latents, positive, learning_rate, iterations, percent, rank, seed,
        gradient_checkpointing,
    ):
        install_all()
        model = model[0]
        learning_rate = learning_rate[0]
        iterations = iterations[0]
        percent = percent[0]
        rank = rank[0]
        seed = seed[0]
        gradient_checkpointing = gradient_checkpointing[0]
        latents = _process_latents_standard_mode(latents)
        positive = _process_conditioning(positive)
        with torch.inference_mode(False):
            dtype = torch.float16 if model.model.get_dtype() == torch.float16 else torch.bfloat16
            latents, count, _ = _prepare_latents_and_count(latents, dtype, False)
            positive = _validate_and_expand_conditioning(positive, count, False)
            model.model.requires_grad_(False).train()
            lora_sd, trained, bypass_manager = setup_identity_lora(
                model, {}, 'LoRA', dtype, rank
            )
            parameters = [value for value in lora_sd.values() if value.requires_grad]
            prepare_frozen_weights(model.model)
            frozen_restores = wrap_early_frozen(model.model)
            injections = bypass_manager.create_injections(model.model)
            for injection in injections:
                injection.inject(model)
            if gradient_checkpointing:
                for module in find_modules_at_depth(model.model.diffusion_model, depth=2):
                    patch(module)
            descent = FixedBatchDescent(
                parameters, trained, learning_rate, iterations, percent, seed, dtype
            )
            guider = TrainGuider(model, offloading=False)
            guider.set_conds(positive)
            noise = comfy_extras.nodes_custom_sampler.Noise_RandomNoise(seed)
            try:
                comfy.model_management.in_training = True
                guider.sample(
                    noise.generate_noise({'samples': latents}),
                    latents,
                    descent,
                    torch.tensor(range(count)),
                    seed=seed,
                )
            finally:
                comfy.model_management.in_training = False
                for injection in injections:
                    injection.eject(model)
                for module in model.model.modules():
                    unpatch(module)
                for module, original in frozen_restores:
                    module.forward = original
        report = json.dumps(
            {
                'adapters': len(trained),
                'losses': [round(value, 6) for value in descent.losses],
                'gradient_norms': [round(value, 6) for value in descent.gradient_norms],
                'adapter_norms': [round(value, 6) for value in descent.adapter_norms],
                'nonfinite': descent.nonfinite,
            }
        )
        logging.info('Zone LoRA gradient probe %s', report)
        return io.NodeOutput(report)
