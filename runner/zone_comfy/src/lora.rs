//! Packaged LoRA training jobs. Default path posts ZoneTrainLoRA to ComfyUI.

use crate::caption::{Captioner, Draft};
use crate::config::Config;
use crate::inventory::WeightSidecar;
use crate::recipe::{RecipeCatalog, sanitize_weight_filename};
use crate::subject::{CENTRE, Subject};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use zone_vision::gravity::Point;
use zone_vision::{Raster, Rendered, decode};

#[derive(Debug, thiserror::Error)]
pub enum TrainError {
    #[error("training is not configured")]
    Disabled,
    #[error("invalid training request: {0}")]
    Invalid(&'static str),
    #[error("training failed: {0}")]
    Failed(String),
}

#[derive(Debug, Deserialize)]
pub struct TrainRequest {
    pub name: String,
    pub base: String,
    #[serde(default)]
    pub trigger: Option<String>,
    pub images: Vec<TrainImage>,
}

#[derive(Debug, Deserialize)]
pub struct TrainImage {
    pub filename: String,
    pub caption: String,
    pub bytes_base64: String,
    #[serde(default)]
    pub before_base64: Option<String>,
    /// Images sharing a group are the same shot and are captioned together.
    /// Frames pulled from a clip arrive grouped; separate photos do not.
    #[serde(default)]
    pub group: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct TrainBase {
    pub id: String,
    pub label: String,
    pub edit: bool,
}

pub fn available_bases(catalog: &RecipeCatalog, models_dir: &Path) -> Vec<TrainBase> {
    let items = crate::inventory::scan(models_dir, catalog);
    catalog
        .image_recipes()
        .filter(|recipe| !recipe.adapter)
        .filter(|recipe| {
            items
                .iter()
                .any(|item| item.recipe_id == recipe.id && item.ready)
        })
        .map(|recipe| TrainBase {
            id: recipe.id.clone(),
            label: recipe.label.clone(),
            edit: matches!(
                recipe.prompt_mode,
                crate::recipe::PromptMode::EditInstruction
            ),
        })
        .collect()
}

pub async fn train(
    config: &Config,
    litellm_host: String,
    litellm_key: String,
    mut request: TrainRequest,
) -> Result<PathBuf, TrainError> {
    if config.train_command.is_none() && !config.enabled {
        return Err(TrainError::Disabled);
    }
    let name = sanitize_weight_filename(&format!("{}.safetensors", request.name.trim()))
        .or_else(|_| sanitize_weight_filename(request.name.trim()))
        .map_err(|_| TrainError::Invalid("invalid LoRA name"))?;
    let filename = if name.ends_with(".safetensors") {
        name
    } else {
        format!("{name}.safetensors")
    };
    if request.images.is_empty() {
        return Err(TrainError::Invalid("training needs images"));
    }
    if request
        .trigger
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Err(TrainError::Invalid(
            "trigger word is required so the LoRA can retain identity",
        ));
    }
    let catalog = RecipeCatalog::load(Some(config.workflow_path.as_path()))
        .or_else(|_| RecipeCatalog::packaged())
        .map_err(|_| TrainError::Invalid("recipe catalog is missing"))?;
    let recipe = catalog
        .get(&request.base)
        .filter(|recipe| !recipe.adapter)
        .ok_or(TrainError::Invalid("unknown training base"))?;
    let work = config
        .models_dir
        .join("training")
        .join(filename.trim_end_matches(".safetensors"));
    let targets = work.join("targets");
    let controls = work.join("control_1");
    fs::create_dir_all(&targets).map_err(|error| TrainError::Failed(error.to_string()))?;
    if recipe.prompt_mode == crate::recipe::PromptMode::EditInstruction {
        fs::create_dir_all(&controls).map_err(|error| TrainError::Failed(error.to_string()))?;
    }
    // Cropping comes before captioning so the vision model describes the image
    // that will be trained on. Captioning the upload instead would have it
    // describe a background the crop is about to remove.
    let subject = Subject::shared(config);
    let side = crate::train::packaged_config()?.resolution();
    let framed = request
        .images
        .iter()
        .map(|image| frame(&subject, image, side))
        .collect::<Result<Vec<_>, _>>()?;

    let trigger = request.trigger.clone().unwrap_or_default();
    let groups: Vec<usize> = shots(&request.images).collect();
    let mut drafts: Vec<Draft> = framed
        .iter()
        .zip(request.images.iter())
        .zip(groups)
        .map(|((framed, image), group)| Draft::new(CROP, &framed.encoded(), &image.caption, group))
        .collect();
    Captioner::new(config, litellm_host, litellm_key)
        .fill(&mut drafts, trigger.trim())
        .await;
    for (image, draft) in request.images.iter_mut().zip(drafts) {
        if image.caption.trim().is_empty() {
            image.caption = draft.caption;
        }
    }

    for (index, (image, framed)) in request.images.iter().zip(&framed).enumerate() {
        let stem = format!("{index:04}");
        let write = |path: PathBuf, bytes: &[u8]| {
            fs::write(path, bytes).map_err(|error| TrainError::Failed(error.to_string()))
        };
        write(targets.join(format!("{stem}.png")), &framed.target)?;
        write(
            targets.join(format!("{stem}.txt")),
            caption(image, request.trigger.as_deref()).as_bytes(),
        )?;
        if let Some(control) = &framed.control {
            write(controls.join(format!("{stem}.png")), control)?;
        }
    }
    let output = config.models_dir.join("loras").join(&filename);
    fs::create_dir_all(output.parent().unwrap_or(Path::new(".")))
        .map_err(|error| TrainError::Failed(error.to_string()))?;
    if let Some(command) = config.train_command.as_deref() {
        let status = Command::new("sh")
            .arg("-c")
            .arg(command)
            .env("ZONE_TRAIN_NAME", &filename)
            .env("ZONE_TRAIN_BASE", &recipe.id)
            .env("ZONE_TRAIN_DIR", &work)
            .env("ZONE_TRAIN_OUTPUT", &output)
            .env(
                "ZONE_TRAIN_TRIGGER",
                request.trigger.clone().unwrap_or_default(),
            )
            .env("COMFYUI_BASE_URL", &config.base_url)
            .env(
                "ZONE_TRAIN_CHECKPOINT",
                recipe
                    .defaults
                    .get("checkpoint")
                    .cloned()
                    .unwrap_or_else(|| config.checkpoint.clone()),
            )
            .env("ZONE_TRAIN_TIMEOUT", config.train_timeout_secs.to_string())
            .env(
                "ZONE_COMFY_INPUT",
                config
                    .models_dir
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join("input")
                    .display()
                    .to_string(),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .status()
            .await
            .map_err(|error| TrainError::Failed(error.to_string()))?;
        if !status.success() {
            return Err(TrainError::Failed(format!(
                "trainer exited {}",
                status.code().unwrap_or(1)
            )));
        }
    } else {
        crate::train::run(
            config,
            recipe,
            &work,
            &output,
            &filename,
            request.images.len(),
        )
        .await?;
    }
    if !output.is_file() {
        return Err(TrainError::Failed(
            "trainer did not write a LoRA file".into(),
        ));
    }
    if let Some(adapter) = catalog
        .adapter_recipe_for_base(recipe.hf_bases.first().unwrap_or(&recipe.id))
        .or_else(|| catalog.adapter_recipe_for_filename(&filename))
    {
        let _ = crate::inventory::write_sidecar(
            &output,
            &WeightSidecar {
                recipe_id: adapter.id.clone(),
                hf_base: recipe.hf_bases.first().cloned(),
            },
        );
    }
    Ok(output)
}

fn caption(image: &TrainImage, trigger: Option<&str>) -> String {
    match trigger.map(str::trim).filter(|value| !value.is_empty()) {
        Some(trigger) if !image.caption.contains(trigger) => {
            format!("{trigger}, {}", image.caption.trim())
        }
        _ => image.caption.trim().to_string(),
    }
}

/// The shot each image belongs to. Frames from a clip say which shot they came
/// from; a separate photo is its own, numbered past every clip's groups so the
/// two cannot be taken for each other.
fn shots(images: &[TrainImage]) -> impl Iterator<Item = usize> + '_ {
    let clips = images
        .iter()
        .filter_map(|image| image.group)
        .max()
        .map_or(0, |last| last + 1);
    images
        .iter()
        .enumerate()
        .map(move |(index, image)| image.group.unwrap_or(clips + index))
}

/// The filename a crop is captioned under. Only its extension is read, to pick
/// the MIME type of the data URL a vision model is handed.
const CROP: &str = "crop.png";

/// One upload as the dataset will hold it: a square PNG framed on its subject,
/// and the control image that has to keep answering it.
#[derive(Debug)]
struct Framed {
    target: Vec<u8>,
    control: Option<Vec<u8>>,
}

impl Framed {
    fn encoded(&self) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(&self.target)
    }
}

/// Crops one upload square onto its subject.
///
/// Cropping here rather than leaving it to the loader is what keeps the subject
/// in the dataset: the loader fits whatever it is given onto a white square, so
/// an uncropped photo trains on its own letterboxing and on however much
/// background the photographer happened to include.
fn frame(subject: &Subject, image: &TrainImage, side: u32) -> Result<Framed, TrainError> {
    let raster = decode(&image.bytes_base64)?;
    let focus = subject.focus(&raster, CENTRE);
    let control = match &image.before_base64 {
        // The control has to keep answering the target pixel for pixel, so it
        // is cropped to the target's subject rather than to its own.
        Some(before) => Some(square(subject, &decode(before)?, side, focus)?),
        None => None,
    };
    Ok(Framed {
        target: square(subject, &raster, side, focus)?,
        control,
    })
}

fn square(
    subject: &Subject,
    raster: &Raster,
    side: u32,
    focus: Point,
) -> Result<Vec<u8>, TrainError> {
    png(&subject
        .render(raster, side, focus)
        .map_err(|error| TrainError::Failed(error.to_string()))?)
}

/// Decoding is also what applies a photo's EXIF rotation: a sideways image
/// otherwise trains a sideways subject.
fn decode(base64: &str) -> Result<Raster, TrainError> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64.trim())
        .map_err(|_| TrainError::Invalid("image is not valid base64"))?;
    if bytes.is_empty() {
        return Err(TrainError::Invalid("image is empty"));
    }
    decode::decode(&bytes)
        .map_err(|_| TrainError::Invalid("training images must be PNG, JPEG, or WebP"))
}

/// Encodes a rendered RGB image as PNG.
pub(crate) fn png(rendered: &Rendered) -> Result<Vec<u8>, TrainError> {
    use image::ImageEncoder;
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &rendered.pixels,
            rendered.width,
            rendered.height,
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|error| TrainError::Failed(error.to_string()))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use base64::Engine;

    /// A real one-pixel image, so the dataset writer has something to decode.
    fn encoded(colour: [u8; 3]) -> String {
        base64::engine::general_purpose::STANDARD.encode(
            png(&Rendered {
                width: 1,
                height: 1,
                pixels: colour.to_vec(),
            })
            .unwrap(),
        )
    }

    /// A flat image of the given size, encoded the way an upload arrives.
    fn encoded_at(width: u32, height: u32) -> String {
        base64::engine::general_purpose::STANDARD.encode(
            png(&Rendered {
                width,
                height,
                pixels: (0..width * height)
                    .flat_map(|index| [(index % 251) as u8, 40, 90])
                    .collect(),
            })
            .unwrap(),
        )
    }

    fn upload(caption: &str, group: Option<usize>) -> TrainImage {
        TrainImage {
            filename: "a.png".into(),
            caption: caption.into(),
            bytes_base64: encoded([12, 34, 56]),
            before_base64: None,
            group,
        }
    }

    #[tokio::test]
    async fn train_writes_adapter_with_configured_command() {
        let root = std::env::temp_dir().join(format!("zone-train-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("loras")).unwrap();
        let config = Config {
            models_dir: root.clone(),
            train_command: Some("printf lora > \"$ZONE_TRAIN_OUTPUT\"".into()),
            ..Default::default()
        };
        let output = train(
            &config,
            String::new(),
            String::new(),
            TrainRequest {
                name: "my-style".into(),
                base: "flux-schnell".into(),
                trigger: Some("ohwx".into()),
                images: vec![upload("a portrait", None)],
            },
        )
        .await
        .unwrap();
        assert_eq!(output.file_name().unwrap(), "my-style.safetensors");
        assert_eq!(fs::read(&output).unwrap(), b"lora");
        let written = root.join("training/my-style/targets/0000.png");
        assert!(written.is_file(), "the dataset image was not written");
        assert_eq!(
            fs::read_to_string(root.join("training/my-style/targets/0000.txt")).unwrap(),
            "ohwx, a portrait"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn caption_prefixes_trigger_for_identity() {
        let image = upload("a portrait", None);
        assert_eq!(caption(&image, Some("ohwx")), "ohwx, a portrait");
        let already = TrainImage {
            caption: "ohwx, a portrait".into(),
            ..image
        };
        assert_eq!(caption(&already, Some("ohwx")), "ohwx, a portrait");
    }

    #[tokio::test]
    async fn train_rejects_missing_trigger() {
        let root = std::env::temp_dir().join(format!("zone-train-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("loras")).unwrap();
        let config = Config {
            models_dir: root.clone(),
            train_command: Some("printf lora > \"$ZONE_TRAIN_OUTPUT\"".into()),
            ..Default::default()
        };
        let error = train(
            &config,
            String::new(),
            String::new(),
            TrainRequest {
                name: "my-style".into(),
                base: "flux-schnell".into(),
                trigger: None,
                images: vec![upload("a portrait", None)],
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(error, TrainError::Invalid(_)));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_photo_never_lands_in_a_clips_shot() {
        let images = vec![
            upload("", Some(0)),
            upload("", Some(1)),
            upload("", Some(0)),
            upload("", None),
            upload("", None),
        ];
        let assigned: Vec<usize> = shots(&images).collect();
        assert_eq!(
            assigned,
            vec![0, 1, 0, 5, 6],
            "frames keep their shot, photos get one each, and the two never meet"
        );
    }

    #[test]
    fn a_photo_is_framed_as_the_upright_square_the_dataset_reads_back() {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(&[10; 8 * 4 * 3], 8, 4, image::ExtendedColorType::Rgb8)
            .unwrap();
        let framed = frame(
            &Subject::none(),
            &TrainImage {
                filename: "a.jpg".into(),
                caption: String::new(),
                bytes_base64: base64::engine::general_purpose::STANDARD.encode(&jpeg),
                before_base64: None,
                group: None,
            },
            4,
        )
        .unwrap();
        assert!(
            framed.target.starts_with(b"\x89PNG\r\n\x1a\n"),
            "a .png in the dataset has to be a PNG"
        );
        assert_eq!(
            decode::decode(&framed.target).unwrap().oriented_size(),
            (4, 4),
            "the loader letterboxes anything that is not already square"
        );
        assert!(framed.control.is_none(), "there was no before image");
    }

    #[test]
    fn an_undecodable_upload_is_rejected_rather_than_framed() {
        let error = frame(
            &Subject::none(),
            &TrainImage {
                filename: "a.png".into(),
                caption: String::new(),
                bytes_base64: base64::engine::general_purpose::STANDARD.encode(b"not an image"),
                before_base64: None,
                group: None,
            },
            512,
        )
        .unwrap_err();
        assert!(matches!(error, TrainError::Invalid(_)), "{error}");
    }

    #[test]
    fn an_edit_pair_is_cropped_the_same_way_on_both_sides() {
        let wide = encoded_at(16, 8);
        let framed = frame(
            &Subject::none(),
            &TrainImage {
                filename: "a.png".into(),
                caption: String::new(),
                bytes_base64: wide.clone(),
                before_base64: Some(wide),
                group: None,
            },
            8,
        )
        .unwrap();
        assert_eq!(
            framed.control.as_deref(),
            Some(framed.target.as_slice()),
            "a control cropped to its own subject stops answering its target"
        );
    }
}
