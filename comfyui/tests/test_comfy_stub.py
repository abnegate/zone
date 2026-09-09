"""The ComfyUI stand-in must answer as the lane that was submitted.

`scripts/live-verify/comfy-stub.py` decides which media to serve by reading the
submitted graph. If that decision is wrong the live suite still passes -- it
just verifies the wrong lane, which is the failure class the suite exists to
close. The upscale lanes matter most: they pin the node their output comes from,
because `LoadVideo` reports its own input as an output and a collector that
swept every node would take the uploaded source for the result.
"""

from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path

ROOT = Path(__file__).parents[2]
STUB = ROOT / 'scripts' / 'live-verify' / 'comfy-stub.py'
WORKFLOWS = ROOT / 'comfyui' / 'workflows'


def load_stub():
    specification = importlib.util.spec_from_file_location('comfy_stub', STUB)
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


stub = load_stub()


def graph(name: str) -> dict:
    with open(WORKFLOWS / name, encoding='utf-8') as handle:
        return json.load(handle)


class LaneDetection(unittest.TestCase):
    def test_every_shipped_workflow_lands_in_the_lane_it_belongs_to(self) -> None:
        expected = {
            'flux1-schnell-fp8-api.json': 'image',
            'flux1-schnell-fp8-img2img-api.json': 'image',
            'flux1-dev-fp8-api.json': 'image',
            'sd15-api.json': 'image',
            'sdxl-api.json': 'image',
            'qwen-image-edit-2511-api.json': 'image',
            'qwen-image-edit-2511-edit-api.json': 'image',
            'wan2.2-ti2v-5b-api.json': 'video',
            'wan2.2-ti2v-5b-i2v-api.json': 'video',
            'ace-step-v1-3.5b-api.json': 'audio',
            'upscale-image-api.json': 'upscale_image',
            'upscale-video-api.json': 'upscale_video',
        }
        for name, lane in expected.items():
            with self.subTest(workflow=name):
                self.assertEqual(stub.lane_of(graph(name)), lane)

    def test_a_video_upscale_is_not_read_as_an_image_upscale(self) -> None:
        """Both carry `ImageUpscaleWithModel`; only one loads a clip."""
        image = stub.lane_of(graph('upscale-image-api.json'))
        video = stub.lane_of(graph('upscale-video-api.json'))
        self.assertEqual(image, 'upscale_image')
        self.assertEqual(video, 'upscale_video')
        self.assertNotEqual(image, video)

    def test_an_unrecognised_graph_falls_back_to_the_image_lane(self) -> None:
        self.assertEqual(stub.lane_of({'1': {'class_type': 'Whatever'}}), 'image')
        self.assertEqual(stub.lane_of({}), 'image')


class OutputAttribution(unittest.TestCase):
    def test_an_upscale_answers_on_the_node_the_collector_pins(self) -> None:
        """`zone_comfy` collects an upscale from node 4 (image) and 5 (video),
        so answering on any other node would look like an empty result."""
        for name, lane, node in [
            ('upscale-image-api.json', 'upscale_image', '4'),
            ('upscale-video-api.json', 'upscale_video', '5'),
        ]:
            with self.subTest(workflow=name):
                self.assertEqual(stub.output_node(graph(name), lane), node)

    def test_a_sweeping_lane_answers_on_the_graph_sink(self) -> None:
        for name, lane, sink in [
            ('flux1-schnell-fp8-api.json', 'image', '9'),
            ('wan2.2-ti2v-5b-api.json', 'video', '10'),
            ('ace-step-v1-3.5b-api.json', 'audio', '10'),
        ]:
            with self.subTest(workflow=name):
                self.assertEqual(stub.output_node(graph(name), lane), sink)


class ServedMedia(unittest.TestCase):
    def test_each_lane_serves_a_distinct_file_under_the_expected_key(self) -> None:
        """A lane collecting the wrong file has to be visible, so no two lanes
        may serve the same bytes, and each key is what `collect_output_files`
        looks under."""
        keys = {lane: entry[1] for lane, entry in stub.LANES.items()}
        self.assertEqual(keys['image'], 'images')
        self.assertEqual(keys['upscale_image'], 'images')
        self.assertEqual(keys['video'], 'videos')
        self.assertEqual(keys['upscale_video'], 'videos')
        self.assertEqual(keys['audio'], 'audio')

        filenames = [entry[0] for entry in stub.LANES.values()]
        self.assertEqual(len(filenames), len(set(filenames)))

    def test_generated_media_is_reported_as_temporary(self) -> None:
        """`collect_output_files` refuses a non-temporary image or audio file,
        so a stand-in that reported `output` would pass for reasons the server
        would reject in production."""
        for lane in ('image', 'upscale_image', 'audio'):
            self.assertEqual(stub.LANES[lane][2], 'temp', lane)


if __name__ == '__main__':
    unittest.main()
