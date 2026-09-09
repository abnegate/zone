from comfy_api.latest import ComfyExtension

from .gradient_node import ZoneProbeGradient
from .inference_hooks import install_all
from .probe_node import ZoneProbeLoss
from .train_node import ZoneLoadTrainFolder, ZoneTrainLoRA

install_all()


class ZoneLoraExtension(ComfyExtension):
    async def get_node_list(self):
        return [ZoneLoadTrainFolder, ZoneProbeGradient, ZoneProbeLoss, ZoneTrainLoRA]


async def comfy_entrypoint():
    return ZoneLoraExtension()
