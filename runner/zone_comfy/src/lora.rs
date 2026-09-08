//! Packaged LoRA training jobs. Default path posts ZoneTrainLoRA to ComfyUI.

use crate::caption::{Captioner, data_url};
use crate::config::Config;
use crate::inventory::WeightSidecar;
use crate::quality::Quality;
use crate::recipe::{RecipeCatalog, sanitize_weight_filename};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

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
}

/// A finished run: the adapter on disk and, when ComfyUI could be asked, how
/// far it beats the base it was trained from.
#[derive(Debug, Serialize)]
pub struct TrainOutcome {
    pub path: PathBuf,
    pub quality: Option<Quality>,
    pub dataset: Vec<crate::dataset::Finding>,
    pub screening: Screening,
}

#[derive(Debug, Serialize)]
pub struct Screening {
    pub kept: usize,
    pub dropped: Vec<Dropped>,
}

#[derive(Debug, Serialize)]
pub struct Dropped {
    pub filename: String,
    pub reason: crate::screening::Rejection,
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
) -> Result<TrainOutcome, TrainError> {
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
    let trigger = request.trigger.clone().unwrap_or_default();
    let mut drafts: Vec<(String, String)> = request
        .images
        .iter()
        .map(|image| {
            (
                data_url(&image.filename, &image.bytes_base64),
                image.caption.clone(),
            )
        })
        .collect();
    let described = Captioner::new(config, litellm_host, litellm_key)
        .fill(&mut drafts, trigger.trim())
        .await;
    let findings = crate::dataset::inspect(&described, request.images.len());
    for (image, (_, drafted)) in request.images.iter_mut().zip(drafts) {
        if image.caption.trim().is_empty() {
            image.caption = drafted;
        }
    }
    let decoded = request
        .images
        .iter()
        .map(|image| decode_base64(&image.bytes_base64))
        .collect::<Result<Vec<Vec<u8>>, TrainError>>()?;
    let verdict = crate::screening::screen(&decoded, crate::train::packaged_config()?.resolution());
    let dropped = verdict
        .drop
        .iter()
        .map(|(index, rejection)| Dropped {
            filename: request.images[*index].filename.clone(),
            reason: *rejection,
        })
        .collect::<Vec<Dropped>>();
    for (stem, index) in verdict.keep.iter().enumerate() {
        let image = &request.images[*index];
        let stem = format!("{stem:04}");
        fs::write(targets.join(format!("{stem}.png")), &decoded[*index])
            .map_err(|error| TrainError::Failed(error.to_string()))?;
        fs::write(
            targets.join(format!("{stem}.txt")),
            caption(image, request.trigger.as_deref()),
        )
        .map_err(|error| TrainError::Failed(error.to_string()))?;
        if let Some(before) = &image.before_base64 {
            write_decoded(&controls.join(format!("{stem}.png")), before)?;
        }
    }
    let output = config.models_dir.join("loras").join(&filename);
    fs::create_dir_all(output.parent().unwrap_or(Path::new(".")))
        .map_err(|error| TrainError::Failed(error.to_string()))?;
    let checkpoint = recipe
        .defaults
        .get("checkpoint")
        .cloned()
        .unwrap_or_else(|| config.checkpoint.clone());
    let stem = filename.trim_end_matches(".safetensors").to_string();
    let folder = if let Some(command) = config.train_command.as_deref() {
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
            .env("ZONE_TRAIN_CHECKPOINT", &checkpoint)
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
        format!("zone-train-{stem}")
    } else {
        crate::train::run(
            config,
            recipe,
            &work,
            &output,
            &filename,
            request.images.len(),
        )
        .await?
    };
    if !output.is_file() {
        return Err(TrainError::Failed(
            "trainer did not write a LoRA file".into(),
        ));
    }
    let captions: HashMap<String, String> = request
        .images
        .iter()
        .enumerate()
        .map(|(index, image)| {
            (
                format!("{index:04}.png"),
                caption(image, request.trigger.as_deref()),
            )
        })
        .collect();
    let probe = Config {
        checkpoint,
        ..config.clone()
    };
    let quality = crate::quality::select(&probe, &folder, &output, &captions).await;
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
    Ok(TrainOutcome {
        path: output,
        quality,
        dataset: findings,
        screening: Screening {
            kept: verdict.keep.len(),
            dropped,
        },
    })
}

fn caption(image: &TrainImage, trigger: Option<&str>) -> String {
    match trigger.map(str::trim).filter(|value| !value.is_empty()) {
        Some(trigger) if !image.caption.contains(trigger) => {
            format!("{trigger}, {}", image.caption.trim())
        }
        _ => image.caption.trim().to_string(),
    }
}

fn decode_base64(base64: &str) -> Result<Vec<u8>, TrainError> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64.trim())
        .map_err(|_| TrainError::Invalid("image is not valid base64"))?;
    if bytes.is_empty() {
        return Err(TrainError::Invalid("image is empty"));
    }
    Ok(bytes)
}

fn write_decoded(path: &Path, base64: &str) -> Result<(), TrainError> {
    fs::write(path, decode_base64(base64)?).map_err(|error| TrainError::Failed(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[tokio::test]
    async fn train_writes_adapter_with_configured_command() {
        let root = std::env::temp_dir().join(format!("zone-train-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("loras")).unwrap();
        let config = Config {
            models_dir: root.clone(),
            train_command: Some("printf lora > \"$ZONE_TRAIN_OUTPUT\"".into()),
            ..Default::default()
        };
        use base64::Engine;
        let tiny = base64::engine::general_purpose::STANDARD.encode([137_u8, 80, 78, 71]);
        let outcome = train(
            &config,
            String::new(),
            String::new(),
            TrainRequest {
                name: "my-style".into(),
                base: "flux-schnell".into(),
                trigger: Some("ohwx".into()),
                images: vec![TrainImage {
                    filename: "a.png".into(),
                    caption: "a portrait".into(),
                    bytes_base64: tiny,
                    before_base64: None,
                }],
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome.path.file_name().unwrap(), "my-style.safetensors");
        assert_eq!(fs::read(&outcome.path).unwrap(), b"lora");
        assert!(
            outcome.quality.is_none(),
            "a run with no reachable probe reports no score instead of failing"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn caption_prefixes_trigger_for_identity() {
        let image = TrainImage {
            filename: "a.png".into(),
            caption: "a portrait".into(),
            bytes_base64: String::new(),
            before_base64: None,
        };
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
        use base64::Engine;
        let tiny = base64::engine::general_purpose::STANDARD.encode([137_u8, 80, 78, 71]);
        let error = train(
            &config,
            String::new(),
            String::new(),
            TrainRequest {
                name: "my-style".into(),
                base: "flux-schnell".into(),
                trigger: None,
                images: vec![TrainImage {
                    filename: "a.png".into(),
                    caption: "a portrait".into(),
                    bytes_base64: tiny,
                    before_base64: None,
                }],
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(error, TrainError::Invalid(_)));
        let _ = fs::remove_dir_all(root);
    }
}
