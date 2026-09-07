from comfy_api.latest import ComfyExtension

from .inference_hooks import install_all
from .train_node import ZoneLoadTrainFolder, ZoneTrainLoRA

install_all()


class ZoneLoraExtension(ComfyExtension):
    async def get_node_list(self):
        return [ZoneLoadTrainFolder, ZoneTrainLoRA]


async def comfy_entrypoint():
    return ZoneLoraExtension()
