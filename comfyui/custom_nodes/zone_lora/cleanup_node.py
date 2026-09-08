from __future__ import annotations

import re
import shutil
from pathlib import Path

import folder_paths
from comfy_api.latest import io


FOLDER = re.compile(
    r'zone-train-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}'
    r'-[89ab][0-9a-f]{3}-[0-9a-f]{12}'
)
ARTIFACT = re.compile(
    r'zone-lora-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}'
    r'-[89ab][0-9a-f]{3}-[0-9a-f]{12}'
)


class ZoneCleanupTrainingRun(io.ComfyNode):
    @classmethod
    def define_schema(cls):
        return io.Schema(
            node_id='ZoneCleanupTrainingRun',
            display_name='Zone Cleanup Training Run',
            category='zone/training',
            is_output_node=True,
            inputs=[
                io.String.Input('folder', default=''),
                io.String.Input('artifact', default=''),
            ],
            outputs=[io.String.Output(display_name='report')],
        )

    @classmethod
    def execute(cls, folder, artifact):
        if not FOLDER.fullmatch(folder) or not ARTIFACT.fullmatch(artifact):
            raise ValueError('cleanup requires server-generated Zone run UUIDs')
        removed = []

        input_root = Path(folder_paths.get_input_directory()).resolve(strict=True)
        dataset = input_root / folder
        if dataset.exists():
            if dataset.is_symlink() or not dataset.resolve(strict=True).is_relative_to(input_root):
                raise ValueError('cleanup refuses an unsafe training folder')
            shutil.rmtree(dataset)
            removed.append(folder)

        pattern = re.compile(rf'{re.escape(artifact)}(?:-step[0-9]+)?\.safetensors')
        roots = [
            (Path(folder_paths.get_output_directory()) / 'loras').resolve(strict=True),
            Path(folder_paths.get_folder_paths('loras')[0]).resolve(strict=True),
        ]
        for root in roots:
            for path in root.iterdir():
                if not pattern.fullmatch(path.name):
                    continue
                if path.is_symlink() or not path.is_file():
                    raise ValueError('cleanup refuses a non-regular training artifact')
                path.unlink()
                removed.append(path.name)
        return io.NodeOutput(','.join(removed))
