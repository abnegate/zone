from __future__ import annotations

import os
import re
import shutil
import uuid
from pathlib import Path

import folder_paths
from comfy_api.latest import io


ARTIFACT = re.compile(
    r'zone-lora-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}'
    r'(?:-step[0-9]+)?\.safetensors'
)


class ZoneStageTrainingArtifact(io.ComfyNode):
    @classmethod
    def define_schema(cls):
        return io.Schema(
            node_id='ZoneStageTrainingArtifact',
            display_name='Zone Stage Training Artifact',
            category='zone/training',
            is_output_node=True,
            inputs=[io.String.Input('artifact', default='')],
            outputs=[io.String.Output(display_name='artifact')],
        )

    @classmethod
    def execute(cls, artifact):
        if not ARTIFACT.fullmatch(artifact):
            raise ValueError('staging requires a server-generated Zone artifact name')
        source_root = (Path(folder_paths.get_output_directory()) / 'loras').resolve(strict=True)
        destination_root = Path(folder_paths.get_folder_paths('loras')[0]).resolve(strict=True)
        source = source_root / artifact
        destination = destination_root / artifact
        if source.is_symlink() or not source.is_file():
            raise ValueError('training artifact is missing or not a regular file')
        if destination.is_symlink():
            raise ValueError('training artifact destination cannot be a symlink')
        temporary = destination_root / f'.{artifact}.{uuid.uuid4()}.tmp'
        try:
            with source.open('rb') as reader, temporary.open('xb') as writer:
                shutil.copyfileobj(reader, writer)
                writer.flush()
                os.fsync(writer.fileno())
            os.replace(temporary, destination)
        finally:
            temporary.unlink(missing_ok=True)
        return io.NodeOutput(artifact)
