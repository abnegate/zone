from comfy_api.latest import ComfyExtension

from .cleanup_node import ZoneCleanupTrainingRun
from .dataset_node import ZoneLoadTrainDataset
from .gradient_node import ZoneProbeGradient
from .inference_hooks import install_all
from .probe_node import ZoneProbeLoss
from .stage_node import ZoneStageTrainingArtifact
from .train_node import ZoneLoadTrainFolder, ZoneTrainLoRA

install_all()


class ZoneLoraExtension(ComfyExtension):
    async def get_node_list(self):
        return [
            ZoneLoadTrainDataset,
            ZoneCleanupTrainingRun,
            ZoneLoadTrainFolder,
            ZoneProbeGradient,
            ZoneProbeLoss,
            ZoneStageTrainingArtifact,
            ZoneTrainLoRA,
        ]


async def comfy_entrypoint():
    return ZoneLoraExtension()
