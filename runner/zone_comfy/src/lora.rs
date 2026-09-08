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
#[cfg_attr(test, derive(PartialEq))]
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
    // Sanitizing the finished filename rather than the name it came from is
    // what keeps the length limit honest: an empty name would otherwise pass as
    // the hidden ".safetensors", and a 250-character one would pass as a
    // filename too long for the disk to hold, failing inside the trainer.
    let name = request.name.trim();
    let filename = if name.ends_with(".safetensors") {
        name.to_string()
    } else {
        format!("{name}.safetensors")
    };
    let filename = sanitize_weight_filename(&filename)
        .ok()
        .filter(|filename| filename != ".safetensors")
        .ok_or(TrainError::Invalid("invalid LoRA name"))?;
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
    let groups = shots(&request.images);
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
    let described = image.caption.trim();
    match trigger.map(str::trim).filter(|value| !value.is_empty()) {
        // A trigger with nothing after it is the whole caption, not the start
        // of one: the separator would otherwise dangle on every image the
        // vision model could not describe.
        Some(trigger) if described.is_empty() => trigger.to_string(),
        Some(trigger) if !mentions(described, trigger) => format!("{trigger}, {described}"),
        _ => described.to_string(),
    }
}

/// Whether the caption already carries the trigger as a token of its own.
///
/// A plain substring test would read a short trigger out of any caption that
/// happens to contain those letters, and the image would train with no trigger
/// at all. Splitting the caption into words instead cannot match a trigger that
/// carries punctuation — `my-style` is never a word — and would prefix it onto
/// a caption that already names it. So the trigger is found as written and only
/// its edges are checked.
fn mentions(caption: &str, trigger: &str) -> bool {
    caption.match_indices(trigger).any(|(start, found)| {
        let before = caption[..start].chars().next_back();
        let after = caption[start + found.len()..].chars().next();
        before.is_none_or(|edge| !edge.is_alphanumeric())
            && after.is_none_or(|edge| !edge.is_alphanumeric())
    })
}

/// The shot each image belongs to. Frames from a clip say which shot they came
/// from; a separate photo is its own, numbered past every clip's groups so the
/// two cannot be taken for each other.
fn shots(images: &[TrainImage]) -> Vec<usize> {
    // Renumbered into a dense range rather than used as sent. The group is
    // deserialized straight from the request, so counting up from the largest
    // one overflows on usize::MAX and, saturating, would hand a photo the same
    // shot as the clip. Renumbering cannot collide whatever arrives, and cannot
    // run past the number of images.
    let mut clips: Vec<usize> = Vec::new();
    let seen: Vec<Option<usize>> = images
        .iter()
        .map(|image| {
            image.group.map(|group| {
                clips
                    .iter()
                    .position(|&known| known == group)
                    .unwrap_or_else(|| {
                        clips.push(group);
                        clips.len() - 1
                    })
            })
        })
        .collect();
    let mut next = clips.len();
    seen.into_iter()
        .map(|shot| {
            shot.unwrap_or_else(|| {
                next += 1;
                next - 1
            })
        })
        .collect()
}

/// Writes one file of the dataset, naming the error mapping the three writes
/// would otherwise repeat.
fn write(path: PathBuf, bytes: &[u8]) -> Result<(), TrainError> {
    fs::write(&path, bytes)
        .map_err(|error| TrainError::Failed(format!("{}: {error}", path.display())))
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

    /// A models root with the output directory the writer needs.
    fn root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("zone-train-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("loras")).unwrap();
        root
    }

    fn request(name: &str, base: &str, trigger: Option<&str>) -> TrainRequest {
        TrainRequest {
            name: name.into(),
            base: base.into(),
            trigger: trigger.map(str::to_string),
            images: vec![upload("a portrait", None)],
        }
    }

    async fn rejected(config: &Config, request: TrainRequest) -> TrainError {
        train(config, String::new(), String::new(), request)
            .await
            .expect_err("this request should not have trained")
    }

    #[tokio::test]
    async fn training_needs_either_a_command_or_a_reachable_comfyui() {
        let error = rejected(
            &Config::default(),
            request("my-style", "flux-schnell", Some("ohwx")),
        )
        .await;
        assert!(matches!(error, TrainError::Disabled), "{error}");
    }

    /// The LoRA name reaches `create_dir_all` and `fs::write`, so it is the one
    /// field of a training request that could write outside the models
    /// directory. Nothing here may be accepted, and nothing may be created.
    #[tokio::test]
    async fn a_name_that_is_not_a_filename_is_refused() {
        let root = root();
        let outside = root
            .join("..")
            .join(format!("escaped-{}", uuid::Uuid::new_v4()));
        let config = Config {
            models_dir: root.clone(),
            train_command: Some("true".into()),
            ..Default::default()
        };
        for name in [
            "",
            "   ",
            ".",
            "..",
            "../escape",
            "../../escape",
            "a/../../escape",
            "/etc/passwd",
            "/absolute",
            "back\\slash",
            "nested/name",
            &outside.display().to_string(),
            &"a".repeat(300),
        ] {
            let error = rejected(&config, request(name, "flux-schnell", Some("ohwx"))).await;
            assert!(
                matches!(error, TrainError::Invalid(_)),
                "{name:?} was not refused: {error}"
            );
        }

        let training = root.join("training");
        assert!(
            !training.exists() || fs::read_dir(&training).unwrap().count() == 0,
            "a refused name must not create a dataset directory"
        );
        assert!(
            !outside.exists(),
            "a refused name reached outside the models directory"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn training_needs_images() {
        let root = root();
        let config = Config {
            models_dir: root.clone(),
            train_command: Some("true".into()),
            ..Default::default()
        };
        let mut empty = request("my-style", "flux-schnell", Some("ohwx"));
        empty.images.clear();
        let error = rejected(&config, empty).await;
        assert!(matches!(error, TrainError::Invalid(_)), "{error}");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn a_blank_trigger_counts_as_no_trigger() {
        let root = root();
        let config = Config {
            models_dir: root.clone(),
            train_command: Some("true".into()),
            ..Default::default()
        };
        let error = rejected(&config, request("my-style", "flux-schnell", Some("   "))).await;
        assert!(matches!(error, TrainError::Invalid(_)), "{error}");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn an_unknown_base_is_refused_before_anything_is_written() {
        let root = root();
        let config = Config {
            models_dir: root.clone(),
            train_command: Some("true".into()),
            ..Default::default()
        };
        let error = rejected(&config, request("my-style", "no-such-base", Some("ohwx"))).await;
        assert!(matches!(error, TrainError::Invalid(_)), "{error}");
        assert!(
            !root.join("training/my-style").exists(),
            "a refused request should leave no dataset behind"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn a_trainer_that_exits_badly_is_reported() {
        let root = root();
        let config = Config {
            models_dir: root.clone(),
            train_command: Some("exit 3".into()),
            ..Default::default()
        };
        let error = rejected(&config, request("my-style", "flux-schnell", Some("ohwx"))).await;
        let TrainError::Failed(message) = &error else {
            panic!("expected a failure, got {error}");
        };
        assert!(
            message.contains('3'),
            "the exit code is the diagnosis: {message}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn a_trainer_that_writes_nothing_is_not_a_success() {
        let root = root();
        let config = Config {
            models_dir: root.clone(),
            train_command: Some("true".into()),
            ..Default::default()
        };
        let error = rejected(&config, request("my-style", "flux-schnell", Some("ohwx"))).await;
        assert!(
            matches!(&error, TrainError::Failed(message) if message.contains("did not write")),
            "{error}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn an_edit_base_gets_a_control_directory_beside_its_targets() {
        let root = root();
        let config = Config {
            models_dir: root.clone(),
            train_command: Some("printf lora > \"$ZONE_TRAIN_OUTPUT\"".into()),
            ..Default::default()
        };
        let catalog = RecipeCatalog::packaged().unwrap();
        let edit = catalog
            .image_recipes()
            .find(|recipe| {
                !recipe.adapter && recipe.prompt_mode == crate::recipe::PromptMode::EditInstruction
            })
            .expect("the packaged catalog no longer ships an edit base")
            .id
            .clone();
        let mut pair = request("my-edit", &edit, Some("ohwx"));
        pair.images[0].before_base64 = Some(encoded_at(8, 8));
        train(&config, String::new(), String::new(), pair)
            .await
            .unwrap();
        assert!(
            root.join("training/my-edit/control_1/0000.png").is_file(),
            "an edit base trains on before and after together"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn a_blank_caption_is_filled_in_before_the_dataset_is_written() {
        let root = root();
        let config = Config {
            models_dir: root.clone(),
            train_command: Some("printf lora > \"$ZONE_TRAIN_OUTPUT\"".into()),
            ..Default::default()
        };
        let mut blank = request("my-style", "flux-schnell", Some("ohwx"));
        blank.images[0].caption = String::new();
        train(&config, String::new(), String::new(), blank)
            .await
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("training/my-style/targets/0000.txt")).unwrap(),
            "ohwx",
            "with no caption model, the trigger alone still has to reach the dataset"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn a_name_too_long_to_write_is_refused_rather_than_failing_in_the_trainer() {
        let root = root();
        let config = Config {
            models_dir: root.clone(),
            train_command: Some("printf lora > \"$ZONE_TRAIN_OUTPUT\"".into()),
            ..Default::default()
        };
        // Under the limit itself, over it once ".safetensors" is on the end.
        let long = "a".repeat(250);
        let error = rejected(&config, request(&long, "flux-schnell", Some("ohwx"))).await;
        assert!(matches!(error, TrainError::Invalid(_)), "{error}");

        let named = train(
            &config,
            String::new(),
            String::new(),
            request("already.safetensors", "flux-schnell", Some("ohwx")),
        )
        .await
        .unwrap();
        assert_eq!(
            named.file_name().unwrap(),
            "already.safetensors",
            "a name that already carries the extension keeps exactly one"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_empty_upload_is_not_an_image() {
        let Err(error) = decode("") else {
            panic!("an empty upload is not an image");
        };
        assert!(matches!(error, TrainError::Invalid(_)), "{error}");
    }

    #[test]
    fn only_installed_bases_are_offered_for_training() {
        let root = std::env::temp_dir().join(format!("zone-bases-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("checkpoints")).unwrap();
        let catalog = RecipeCatalog::packaged().unwrap();

        assert!(
            available_bases(&catalog, &root).is_empty(),
            "nothing is trainable until its weights are on disk"
        );

        let recipe = catalog
            .get("flux-schnell")
            .expect("the packaged catalog ships flux-schnell");
        let checkpoint = recipe
            .defaults
            .get("checkpoint")
            .expect("flux-schnell declares a checkpoint");
        fs::write(root.join("checkpoints").join(checkpoint), vec![0u8; 1024]).unwrap();

        let bases = available_bases(&catalog, &root);
        assert!(
            bases
                .iter()
                .any(|base| base.id == "flux-schnell" && !base.edit),
            "the installed base should be offered: {bases:?}"
        );
        assert!(
            !bases.iter().any(|base| base.id.ends_with("-adapter")),
            "a LoRA slot is not something to train onto: {bases:?}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn without_a_train_command_the_job_runs_as_a_comfyui_graph() {
        use wiremock::matchers::{method, path as path_matcher};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path_matcher("/upload/image"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path_matcher("/prompt"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"prompt_id": "p-1"})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_matcher("/history/p-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"p-1": {"status": {"completed": true, "status_str": "success"}}}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_matcher("/view"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![9u8; 20_000]))
            .mount(&server)
            .await;

        let root = root();
        let config = Config {
            models_dir: root.clone(),
            enabled: true,
            base_url: server.uri(),
            train_command: None,
            poll_interval_ms: 50,
            ..Default::default()
        };
        let output = train(
            &config,
            String::new(),
            String::new(),
            request("graph-style", "flux-schnell", Some("ohwx")),
        )
        .await
        .unwrap();
        assert_eq!(fs::read(&output).unwrap(), vec![9u8; 20_000]);
        assert!(
            root.join("training/graph-style/targets/0000.png").is_file(),
            "the dataset has to reach disk before the graph is queued"
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
    fn a_group_at_the_top_of_its_range_does_not_wrap_a_photo_onto_a_clip() {
        // The group is deserialized straight from the request, so this is a
        // value a caller can actually send.
        let images = vec![
            upload("", Some(usize::MAX)),
            upload("", Some(usize::MAX)),
            upload("", None),
        ];
        let assigned = shots(&images);
        assert_eq!(assigned[0], assigned[1], "two frames of one shot share it");
        assert_ne!(
            assigned[2], assigned[0],
            "a photo must not be captioned as part of the clip's shot"
        );
    }

    #[test]
    fn a_trigger_is_matched_as_a_word_rather_than_a_substring() {
        // A short trigger occurs inside longer words, and a substring test
        // would read that as the trigger already being present, leaving the
        // image to train with no trigger at all.
        assert_eq!(
            caption(&upload("zrk pattern knitwear", None), Some("zrkx")),
            "zrkx, zrk pattern knitwear"
        );
        assert_eq!(
            caption(&upload("zrkxyz, a portrait", None), Some("zrkxyz")),
            "zrkxyz, a portrait",
            "a caption that already names the trigger keeps exactly one"
        );
        assert_eq!(
            caption(&upload("zrkxyzed hair", None), Some("zrkxyz")),
            "zrkxyz, zrkxyzed hair",
            "a longer word that merely starts with the trigger is not the trigger"
        );
        // A trigger is not always one word. Splitting the caption into words
        // could never match this one, and would prefix it a second time.
        assert_eq!(
            caption(&upload("my-style, a portrait", None), Some("my-style")),
            "my-style, a portrait",
            "a hyphenated trigger the caption already names is not repeated"
        );
        assert_eq!(
            caption(&upload("a portrait", None), Some("my-style")),
            "my-style, a portrait"
        );
    }

    #[test]
    fn a_failed_write_names_the_artifact_it_was_for() {
        let missing = std::env::temp_dir()
            .join(format!("zone-absent-{}", uuid::Uuid::new_v4()))
            .join("targets")
            .join("0000.png");
        let Err(TrainError::Failed(message)) = write(missing.clone(), b"x") else {
            panic!("writing into a directory that does not exist should fail");
        };
        assert!(
            message.contains("0000.png"),
            "an operator has to be told which artifact failed: {message}"
        );
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
        let assigned = shots(&images);
        assert_eq!(
            assigned,
            vec![0, 1, 0, 2, 3],
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
