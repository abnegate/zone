from __future__ import annotations

import json

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
    make_batch_extra_option_dict,
    process_cond_list,
)


class LossProbe(comfy.samplers.Sampler):
    """The training loss at a fixed noise level, so two sets of weights are comparable."""

    def __init__(self, percents: list[float], seed: int):
        self.percents = percents
        self.seed = seed
        self.losses: dict[float, float] = {}

    def noise_for(self, percent: float, index: int, latent: torch.Tensor) -> torch.Tensor:
        generator = torch.Generator().manual_seed(
            self.seed + index * 7919 + int(percent * 1_000_000)
        )
        return torch.randn(latent.shape, generator=generator).to(latent)

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
        dataset_size = sigmas.size(0)
        sampling = model_wrap.inner_model.model_sampling
        for percent in self.percents:
            sigma = sampling.percent_to_sigma(percent)
            total = 0.0
            for index in range(dataset_size):
                latent = latent_image[index : index + 1]
                batch_sigmas = torch.tensor([sigma]).to(latent.device)
                batch_noise = self.noise_for(percent, index, latent)
                xt = sampling.noise_scaling(batch_sigmas, batch_noise, latent, False)
                x0 = sampling.noise_scaling(
                    torch.zeros_like(batch_sigmas), torch.zeros_like(batch_noise), latent, False
                )
                model_wrap.conds['positive'] = [cond[index]]
                batch_extra = make_batch_extra_option_dict(
                    extra_args, [index], full_size=dataset_size
                )
                x0_pred = model_wrap(xt, batch_sigmas, **batch_extra)
                total += torch.nn.functional.mse_loss(x0_pred.float(), x0.float()).item()
            self.losses[percent] = total / dataset_size
        return torch.zeros_like(latent_image)


class ZoneProbeLoss(io.ComfyNode):
    @classmethod
    def define_schema(cls):
        return io.Schema(
            node_id='ZoneProbeLoss',
            display_name='Zone Probe Loss',
            category='zone/training',
            is_input_list=True,
            inputs=[
                io.Model.Input('model'),
                io.Latent.Input('latents'),
                io.Conditioning.Input('positive'),
                io.String.Input('percents', default='0.1,0.3,0.5,0.7,0.9'),
                io.Int.Input('seed', default=0, min=0, max=0xFFFFFFFF),
            ],
            outputs=[io.String.Output(display_name='report')],
        )

    @classmethod
    def execute(cls, model, latents, positive, percents, seed):
        model = model[0]
        seed = seed[0]
        wanted = [float(part) for part in percents[0].split(',') if part.strip()]
        latents = _process_latents_standard_mode(latents)
        positive = _process_conditioning(positive)
        dtype = torch.float16 if model.model.get_dtype() == torch.float16 else torch.bfloat16
        latents, count, _ = _prepare_latents_and_count(latents, dtype, False)
        positive = _validate_and_expand_conditioning(positive, count, False)
        probe = LossProbe(wanted, seed)
        guider = TrainGuider(model, offloading=False)
        guider.set_conds(positive)
        noise = comfy_extras.nodes_custom_sampler.Noise_RandomNoise(seed)
        guider.sample(
            noise.generate_noise({'samples': latents}),
            latents,
            probe,
            torch.tensor(range(count)),
            seed=seed,
        )
        report = json.dumps(
            {
                'images': count,
                'mean': round(sum(probe.losses.values()) / max(len(probe.losses), 1), 6),
                'by_percent': {f'{k:g}': round(v, 6) for k, v in probe.losses.items()},
            }
        )
        return io.NodeOutput(report)
