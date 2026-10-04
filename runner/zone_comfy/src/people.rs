//! Person generate extras: OpenPose ControlNet, IP-Adapter Plus Face, embeddings, face refine.

use crate::dataset::POSES;
use crate::inventory::{self, WeightSidecar};
use crate::person;
use crate::recipe::sanitize_upload_name;
use crate::subject::{self, Subject};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use zone_vision::crop::{self, Region, Rendered, Target};
use zone_vision::decode::{self, Layout, Orientation, Raster};

pub(crate) const CONTROLNET_FILE: &str = "thibaud_xl_openpose.safetensors";
pub(crate) const IPADAPTER_FILE: &str = "ip-adapter-plus-face_sdxl_vit-h.safetensors";
pub(crate) const CLIP_VISION_FILE: &str = "CLIP-ViT-H-14-laion2B-s32B-b79K.safetensors";
pub(crate) const CONTROLNET_STRENGTH: f64 = 0.7;
pub(crate) const IPADAPTER_STRENGTH: f64 = 0.4;
pub(crate) const REFINE_DENOISE: f64 = 0.3;

const POSE_NODE: &str = "20";
const CONTROLNET_LOADER: &str = "21";
const CONTROLNET_APPLY: &str = "22";
const FACE_IMAGE_NODE: &str = "23";
const CLIP_VISION_NODE: &str = "24";
const IPADAPTER_NODE: &str = "26";

const POSE_FILES: &[&str] = &[
    "standing.png",
    "sitting.png",
    "lying.png",
    "crouching.png",
    "leaning.png",
    "walking.png",
    "running.png",
    "jumping.png",
    "climbing.png",
    "dancing.png",
    "flying.png",
    "riding.png",
    "reaching.png",
    "holding.png",
    "handstand.png",
    "front.png",
    "back_view.png",
    "side.png",
    "three_quarter.png",
    "above.png",
    "below.png",
    "close_up.png",
    "wide.png",
    "full_body.png",
    "medium_shot.png",
];

const _: () = assert!(POSES.len() == POSE_FILES.len());

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Extras {
    pub controlnet: bool,
    pub ipadapter: bool,
}

pub(crate) struct HeadCrop {
    pub png: Vec<u8>,
    pub region: Region,
    pub full_frame: bool,
}

pub(crate) fn extras(
    models_dir: &Path,
    recipe_id: &str,
    selected: &str,
    has_source: bool,
    allow: bool,
) -> Extras {
    if !allow || !sdxl_people_graph(recipe_id) {
        return Extras::default();
    }
    let sidecar = inventory::sidecar_for_weight(models_dir, selected);
    if !trigger_bound(sidecar.as_ref()) {
        return Extras::default();
    }
    let controlnet =
        !has_source && inventory::model_file_present(models_dir, "controlnet", CONTROLNET_FILE);
    let ipadapter = inventory::model_file_present(models_dir, "ipadapter", IPADAPTER_FILE)
        && inventory::model_file_present(models_dir, "clip_vision", CLIP_VISION_FILE)
        && inventory::face_path(models_dir, selected).is_some();
    Extras {
        controlnet,
        ipadapter,
    }
}

pub(crate) fn should_refine(recipe_id: &str, sidecar: Option<&WeightSidecar>) -> bool {
    matches!(recipe_id, "sdxl-people" | "sdxl-adapter")
        || (recipe_id == "sdxl" && trigger_bound(sidecar))
}

pub(crate) fn prefix_embedding(prompt: &str, sidecar: Option<&WeightSidecar>) -> String {
    let Some(token) = sidecar
        .and_then(|sidecar| sidecar.embedding.as_deref())
        .and_then(embedding_token)
    else {
        return prompt.to_string();
    };
    if prompt.split_whitespace().any(|word| word == token) {
        return prompt.to_string();
    }
    format!("{token} {prompt}")
}

pub(crate) fn pose_file(prompt: &str) -> &'static str {
    let mut chosen = POSE_FILES[0];
    for (terms, file) in POSES.iter().zip(POSE_FILES) {
        if terms
            .iter()
            .any(|term| inventory::contains_phrase(prompt, term))
        {
            chosen = file;
        }
    }
    chosen
}

pub(crate) fn pose_bytes(workflow_path: &Path, prompt: &str) -> Option<(String, Vec<u8>)> {
    let file = pose_file(prompt);
    let bytes = std::fs::read(poses_dir(workflow_path)?.join(file)).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let name = sanitize_upload_name(&format!("zone-pose-{file}")).ok()?;
    Some((name, bytes))
}

pub(crate) fn apply_graph(
    workflow: &mut Value,
    extras: &Extras,
    pose_name: Option<&str>,
    face_name: Option<&str>,
    denoise: Option<f64>,
) {
    if extras.controlnet {
        if let Some(name) = pose_name
            && let Some(image) = workflow.pointer_mut("/20/inputs/image")
        {
            *image = json!(name);
        }
        if let Some(strength) = workflow.pointer_mut("/22/inputs/strength") {
            *strength = json!(CONTROLNET_STRENGTH);
        }
    } else {
        strip_controlnet(workflow);
    }
    if extras.ipadapter
        && let Some(face) = face_name
    {
        inject_ipadapter(workflow, face);
    }
    if let Some(denoise) = denoise
        && let Some(value) = workflow.pointer_mut("/3/inputs/denoise")
    {
        *value = json!(denoise);
    }
}

pub(crate) fn head_crop(bytes: &[u8], subject: &Subject) -> Option<HeadCrop> {
    let raster = decode::decode(bytes).ok()?;
    let size = raster.oriented_size();
    let body = person::bucket(size.0, size.1);
    let head = person::head_target(body);
    let focus = subject.focus(&raster, subject::CENTRE);
    if person::is_duplicate_head(body, head) {
        return Some(HeadCrop {
            png: bytes.to_vec(),
            region: Region {
                x: 0,
                y: 0,
                width: size.0,
                height: size.1,
            },
            full_frame: true,
        });
    }
    let rendered = person::render_head(subject, &raster, focus, body).ok()??;
    Some(HeadCrop {
        png: encode_png(&rendered)?,
        region: crop::plan(size, head, person::head_focus(focus)).ok()?,
        full_frame: false,
    })
}

pub(crate) fn paste(original: &[u8], refined: &[u8], region: Region) -> Option<Vec<u8>> {
    let mut raster = decode::decode(original).ok()?;
    if raster.orientation != Orientation::Normal {
        return None;
    }
    let refined = decode::decode(refined).ok()?;
    let scaled = crop::render(
        &refined,
        Region {
            x: 0,
            y: 0,
            width: refined.width,
            height: refined.height,
        },
        Target::new(region.width, region.height),
    )
    .ok()?;
    paste_rgb(&mut raster, region, &scaled)?;
    encode_raster(&raster)
}

fn sdxl_people_graph(recipe_id: &str) -> bool {
    matches!(recipe_id, "sdxl" | "sdxl-people" | "sdxl-adapter")
}

fn trigger_bound(sidecar: Option<&WeightSidecar>) -> bool {
    sidecar
        .and_then(|sidecar| sidecar.trigger.as_deref())
        .map(str::trim)
        .is_some_and(|trigger| !trigger.is_empty())
}

fn embedding_token(name: &str) -> Option<String> {
    let stem = Path::new(name).file_stem()?.to_str()?.trim();
    if stem.is_empty()
        || !stem
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return None;
    }
    Some(format!("embedding:{stem}"))
}

fn poses_dir(workflow_path: &Path) -> Option<PathBuf> {
    Some(workflow_path.parent()?.parent()?.join("poses"))
}

fn strip_controlnet(workflow: &mut Value) {
    let Some(object) = workflow.as_object_mut() else {
        return;
    };
    let is_controlnet = object
        .get(CONTROLNET_LOADER)
        .and_then(|node| node.get("class_type"))
        .and_then(Value::as_str)
        == Some("ControlNetLoader");
    if !is_controlnet {
        return;
    }
    object.remove(POSE_NODE);
    object.remove(CONTROLNET_LOADER);
    object.remove(CONTROLNET_APPLY);
    let Some(inputs) = object
        .get_mut("3")
        .and_then(|node| node.get_mut("inputs"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    if link_node(inputs.get("positive")) == Some(CONTROLNET_APPLY) {
        inputs.insert("positive".into(), json!(["6", 0]));
    }
    if link_node(inputs.get("negative")) == Some(CONTROLNET_APPLY) {
        inputs.insert("negative".into(), json!(["7", 0]));
    }
}

fn inject_ipadapter(workflow: &mut Value, face: &str) {
    let Some(object) = workflow.as_object_mut() else {
        return;
    };
    if object.contains_key(IPADAPTER_NODE) {
        return;
    }
    let model = object
        .get("3")
        .and_then(|node| node.pointer("/inputs/model"))
        .cloned()
        .unwrap_or_else(|| json!(["4", 0]));
    object.insert(
        FACE_IMAGE_NODE.into(),
        json!({
            "class_type": "LoadImage",
            "inputs": { "image": face }
        }),
    );
    object.insert(
        CLIP_VISION_NODE.into(),
        json!({
            "class_type": "CLIPVisionLoader",
            "inputs": { "clip_name": CLIP_VISION_FILE }
        }),
    );
    object.insert(
        IPADAPTER_NODE.into(),
        json!({
            "class_type": "ZoneIPAdapterFace",
            "inputs": {
                "model": model,
                "clip_vision": [CLIP_VISION_NODE, 0],
                "image": [FACE_IMAGE_NODE, 0],
                "ipadapter_name": IPADAPTER_FILE,
                "strength": IPADAPTER_STRENGTH
            }
        }),
    );
    if let Some(inputs) = object
        .get_mut("3")
        .and_then(|node| node.get_mut("inputs"))
        .and_then(Value::as_object_mut)
    {
        inputs.insert("model".into(), json!([IPADAPTER_NODE, 0]));
    }
}

fn link_node(value: Option<&Value>) -> Option<&str> {
    value?.as_array()?.first()?.as_str()
}

fn paste_rgb(raster: &mut Raster, region: Region, patch: &Rendered) -> Option<()> {
    let channels = raster.layout.channels();
    if patch.width != region.width || patch.height != region.height {
        return None;
    }
    if region.x.checked_add(region.width)? > raster.width
        || region.y.checked_add(region.height)? > raster.height
    {
        return None;
    }
    if patch.pixels.len() != (patch.width as usize) * (patch.height as usize) * 3 {
        return None;
    }
    for row in 0..region.height {
        for col in 0..region.width {
            let source = ((row * patch.width + col) * 3) as usize;
            let destination =
                (((region.y + row) * raster.width + (region.x + col)) * channels as u32) as usize;
            raster.pixels[destination] = patch.pixels[source];
            raster.pixels[destination + 1] = patch.pixels[source + 1];
            raster.pixels[destination + 2] = patch.pixels[source + 2];
        }
    }
    Some(())
}

fn encode_png(rendered: &Rendered) -> Option<Vec<u8>> {
    encode_pixels(
        &rendered.pixels,
        rendered.width,
        rendered.height,
        Layout::Rgb,
    )
}

fn encode_raster(raster: &Raster) -> Option<Vec<u8>> {
    encode_pixels(&raster.pixels, raster.width, raster.height, raster.layout)
}

fn encode_pixels(pixels: &[u8], width: u32, height: u32, layout: Layout) -> Option<Vec<u8>> {
    use image::ImageEncoder;
    let mut bytes = Vec::new();
    let color = match layout {
        Layout::Rgb => image::ExtendedColorType::Rgb8,
        Layout::Rgba => image::ExtendedColorType::Rgba8,
    };
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(pixels, width, height, color)
        .ok()?;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::WeightSidecar;

    fn sdxl_graph() -> Value {
        json!({
            "3": {
                "class_type": "KSampler",
                "inputs": {
                    "model": ["4", 0],
                    "positive": ["22", 0],
                    "negative": ["22", 1],
                    "denoise": 1.0
                }
            },
            "4": { "class_type": "CheckpointLoaderSimple", "inputs": { "ckpt_name": "a.safetensors" } },
            "6": { "class_type": "CLIPTextEncode", "inputs": { "text": "prompt" } },
            "7": { "class_type": "CLIPTextEncode", "inputs": { "text": "neg" } },
            "20": { "class_type": "LoadImage", "inputs": { "image": "zone-pose-standing.png" } },
            "21": { "class_type": "ControlNetLoader", "inputs": { "control_net_name": CONTROLNET_FILE } },
            "22": {
                "class_type": "ControlNetApplyAdvanced",
                "inputs": {
                    "positive": ["6", 0],
                    "negative": ["7", 0],
                    "control_net": ["21", 0],
                    "image": ["20", 0],
                    "strength": 0.7
                }
            }
        })
    }

    fn rgb_png(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
        let pixels: Vec<u8> = (0..width * height).flat_map(|_| rgb).collect();
        encode_pixels(&pixels, width, height, Layout::Rgb).unwrap()
    }

    #[test]
    fn pose_file_defaults_to_standing_and_last_match_wins() {
        assert_eq!(pose_file("a lighthouse at dusk"), "standing.png");
        assert_eq!(pose_file("ohwx sitting in a kitchen"), "sitting.png");
        assert_eq!(pose_file("standing close up"), "close_up.png");
        assert_eq!(pose_file("lying on a sofa, full body"), "full_body.png");
    }

    #[test]
    fn embedding_prefix_uses_the_sidecar_stem() {
        let sidecar = WeightSidecar {
            embedding: Some("ohwx.safetensors".into()),
            ..Default::default()
        };
        assert_eq!(
            prefix_embedding("ohwx standing", Some(&sidecar)),
            "embedding:ohwx ohwx standing"
        );
        assert_eq!(
            prefix_embedding("embedding:ohwx already there", Some(&sidecar)),
            "embedding:ohwx already there"
        );
        assert_eq!(prefix_embedding("ohwx standing", None), "ohwx standing");
    }

    #[test]
    fn missing_extras_strip_controlnet_and_leave_the_sampler_on_clip() {
        let mut workflow = sdxl_graph();
        apply_graph(&mut workflow, &Extras::default(), None, None, None);
        assert!(workflow.get("20").is_none());
        assert!(workflow.get("21").is_none());
        assert!(workflow.get("22").is_none());
        assert_eq!(workflow["3"]["inputs"]["positive"], json!(["6", 0]));
        assert_eq!(workflow["3"]["inputs"]["negative"], json!(["7", 0]));
        assert!(workflow.get("26").is_none());
    }

    #[test]
    fn extras_keep_controlnet_and_inject_plus_face() {
        let mut workflow = sdxl_graph();
        apply_graph(
            &mut workflow,
            &Extras {
                controlnet: true,
                ipadapter: true,
            },
            Some("zone-pose-sitting.png"),
            Some("jerry.face.png"),
            None,
        );
        assert_eq!(
            workflow["20"]["inputs"]["image"],
            json!("zone-pose-sitting.png")
        );
        assert_eq!(workflow["21"]["class_type"], json!("ControlNetLoader"));
        assert_eq!(workflow["26"]["class_type"], json!("ZoneIPAdapterFace"));
        assert_eq!(
            workflow["26"]["inputs"]["image"],
            json!([FACE_IMAGE_NODE, 0])
        );
        assert_eq!(workflow["26"]["inputs"]["strength"], json!(0.4));
        assert_eq!(workflow["3"]["inputs"]["model"], json!([IPADAPTER_NODE, 0]));
        assert_eq!(workflow["3"]["inputs"]["positive"], json!(["22", 0]));
    }

    #[test]
    fn refine_denoise_overrides_the_sampler() {
        let mut workflow = sdxl_graph();
        apply_graph(
            &mut workflow,
            &Extras::default(),
            None,
            None,
            Some(REFINE_DENOISE),
        );
        assert_eq!(workflow["3"]["inputs"]["denoise"], json!(0.3));
    }

    #[test]
    fn a_square_close_up_refines_the_whole_frame() {
        let png = rgb_png(64, 64, [10, 20, 30]);
        let crop = head_crop(&png, &Subject::none()).unwrap();
        assert!(crop.full_frame);
        assert_eq!(crop.png, png);
    }

    #[test]
    fn a_portrait_emits_a_head_crop_and_pastes_it_back() {
        let png = rgb_png(64, 96, [12, 34, 56]);
        let crop = head_crop(&png, &Subject::none()).unwrap();
        assert!(!crop.full_frame);
        assert!(!crop.png.is_empty());
        let patched = rgb_png(crop.region.width, crop.region.height, [200, 10, 10]);
        let out = paste(&png, &patched, crop.region).unwrap();
        let raster = decode::decode(&out).unwrap();
        let index = ((crop.region.y * raster.width + crop.region.x) * 3) as usize;
        assert_eq!(&raster.pixels[index..index + 3], &[200, 10, 10]);
    }

    #[test]
    fn people_recipes_refine_without_a_bound_identity() {
        assert!(should_refine("sdxl-people", None));
        assert!(should_refine("sdxl-adapter", None));
        assert!(!should_refine("sdxl", None));
        assert!(!should_refine("flux-schnell", None));
        let sidecar = WeightSidecar {
            trigger: Some("ohwx".into()),
            ..Default::default()
        };
        assert!(should_refine("sdxl", Some(&sidecar)));
    }

    #[test]
    fn extras_require_a_bound_identity_and_the_weight_files() {
        let root = tempfile::tempdir().unwrap();
        let models = root.path();
        std::fs::create_dir_all(models.join("loras")).unwrap();
        std::fs::write(models.join("loras/jerry.safetensors"), b"lora").unwrap();
        crate::inventory::write_sidecar(
            &models.join("loras/jerry.safetensors"),
            &WeightSidecar {
                recipe_id: "sdxl-adapter".into(),
                hf_base: Some("John6666/lustify-sdxl-nsfw-checkpoint-ggwp-v7-sdxl".into()),
                trigger: Some("ohwx".into()),
                face: Some("jerry.face.png".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            extras(models, "sdxl-adapter", "jerry.safetensors", false, true),
            Extras::default()
        );
        std::fs::create_dir_all(models.join("controlnet")).unwrap();
        std::fs::write(models.join("controlnet").join(CONTROLNET_FILE), b"cn").unwrap();
        std::fs::create_dir_all(models.join("ipadapter")).unwrap();
        std::fs::write(models.join("ipadapter").join(IPADAPTER_FILE), b"ipa").unwrap();
        std::fs::create_dir_all(models.join("clip_vision")).unwrap();
        std::fs::write(models.join("clip_vision").join(CLIP_VISION_FILE), b"clip").unwrap();
        std::fs::write(models.join("loras/jerry.face.png"), b"face").unwrap();
        assert_eq!(
            extras(models, "sdxl-adapter", "jerry.safetensors", false, true),
            Extras {
                controlnet: true,
                ipadapter: true,
            }
        );
        assert_eq!(
            extras(models, "sdxl-adapter", "jerry.safetensors", true, true),
            Extras {
                controlnet: false,
                ipadapter: true,
            }
        );
        assert_eq!(
            extras(models, "flux-schnell", "jerry.safetensors", false, true),
            Extras::default()
        );
        assert_eq!(
            extras(models, "sdxl-adapter", "jerry.safetensors", false, false),
            Extras::default()
        );
    }
}
