//! Packaged LoRA training jobs. Default path posts ZoneTrainLoRA to ComfyUI.

use crate::caption::{Captioner, data_url};
use crate::config::Config;
use crate::inventory::WeightSidecar;
use crate::quality::Quality;
use crate::recipe::{RecipeCatalog, TrainingModel, sanitize_weight_filename};
use crate::train::Run;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use uuid::Uuid;

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

struct ScreenedImage {
    original: usize,
    target: Vec<u8>,
    reference: Option<Vec<u8>>,
    text: String,
    url: Option<String>,
}

struct Attempt {
    id: String,
    root: PathBuf,
    input: Option<PathBuf>,
    folder: Option<String>,
    artifact: Option<String>,
    produced: Option<PathBuf>,
}

impl Attempt {
    fn create(models: &Path) -> Result<Self, TrainError> {
        require_directory(models, "models directory")?;
        let id = Uuid::new_v4().to_string();
        let training = ensure_child_directory(models, "training")?;
        let root = training.join(&id);
        fs::create_dir(&root).map_err(failed)?;
        let parent = models.parent();
        let attempt = Self {
            artifact: None,
            input: parent.map(|root| root.join("input")),
            folder: None,
            produced: parent.map(|root| root.join("output").join("loras")),
            id,
            root,
        };
        require_confined_directory(&training, &attempt.root)?;
        Ok(attempt)
    }

    fn register(&mut self, run: &Run) -> Result<(), TrainError> {
        run.validate()?;
        self.folder = Some(run.folder.clone());
        self.artifact = Some(run.artifact.clone());
        Ok(())
    }

    fn staged_name(&self) -> String {
        format!("{}.safetensors", self.id)
    }

    fn output(&self) -> PathBuf {
        self.root.join(self.staged_name())
    }

    fn clean_runtime(&self) {
        if let (Some(input), Some(folder)) = (&self.input, &self.folder)
            && safe_directory(input)
        {
            remove_entry(&input.join(folder));
        }
        let (Some(produced), Some(artifact)) = (&self.produced, &self.artifact) else {
            return;
        };
        if !produced.parent().is_some_and(safe_directory) || !safe_directory(produced) {
            return;
        }
        let Ok(entries) = fs::read_dir(produced) else {
            return;
        };
        let final_name = format!("{artifact}.safetensors");
        let checkpoint = format!("{artifact}-step");
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name == final_name || name.starts_with(&checkpoint) && name.ends_with(".safetensors")
            {
                remove_entry(&entry.path());
            }
        }
    }
}

impl Drop for Attempt {
    fn drop(&mut self) {
        self.clean_runtime();
        remove_entry(&self.root);
    }
}

pub fn available_bases(catalog: &RecipeCatalog, models_dir: &Path) -> Vec<TrainBase> {
    let items = crate::inventory::scan(models_dir, catalog);
    catalog
        .image_recipes()
        .filter(|recipe| !recipe.adapter)
        .filter(|recipe| recipe.training_model().is_ok())
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
    request: TrainRequest,
) -> Result<TrainOutcome, TrainError> {
    train_with_screening(
        config,
        litellm_host,
        litellm_key,
        request,
        crate::screening::screen,
    )
    .await
}

async fn train_with_screening(
    config: &Config,
    litellm_host: String,
    litellm_key: String,
    request: TrainRequest,
    screening: fn(&[Vec<u8>], u32) -> crate::screening::Verdict,
) -> Result<TrainOutcome, TrainError> {
    if config.train_command.is_none() && !config.enabled {
        return Err(TrainError::Disabled);
    }
    let filename = final_filename(&request.name)?;
    if request.images.is_empty() {
        return Err(TrainError::Invalid("training needs images"));
    }
    let catalog = RecipeCatalog::load(Some(config.workflow_path.as_path()))
        .or_else(|_| RecipeCatalog::packaged())
        .map_err(|_| TrainError::Invalid("recipe catalog is missing"))?;
    let recipe = catalog
        .get(&request.base)
        .filter(|recipe| !recipe.adapter)
        .ok_or(TrainError::Invalid("unknown training base"))?;
    let model = recipe
        .training_model()
        .map_err(|_| TrainError::Invalid("training base is not supported"))?;
    let edit = matches!(&model, TrainingModel::QwenEdit { .. });
    let trigger = request.trigger.as_deref().unwrap_or_default().trim();
    if !edit && trigger.is_empty() {
        return Err(TrainError::Invalid(
            "trigger word is required so the LoRA can retain identity",
        ));
    }
    validate_pairing(&request.images, &model)?;
    let decoded = request
        .images
        .iter()
        .map(|image| decode_base64(&image.bytes_base64))
        .collect::<Result<Vec<Vec<u8>>, TrainError>>()?;
    let references = request
        .images
        .iter()
        .map(|image| {
            image
                .before_base64
                .as_deref()
                .map(decode_base64)
                .transpose()
        })
        .collect::<Result<Vec<Option<Vec<u8>>>, TrainError>>()?;
    let verdict = screening(&decoded, crate::train::packaged_config()?.resolution());
    validate_verdict(&verdict, request.images.len())?;
    let dropped = verdict
        .drop
        .iter()
        .map(|(index, rejection)| Dropped {
            filename: request.images[*index].filename.clone(),
            reason: *rejection,
        })
        .collect::<Vec<Dropped>>();
    let mut survivors = request
        .images
        .into_iter()
        .zip(decoded)
        .zip(references)
        .enumerate()
        .filter(|(index, _)| verdict.keep.binary_search(index).is_ok())
        .map(|(original, ((image, target), reference))| ScreenedImage {
            original,
            url: (!edit).then(|| data_url(&image.filename, &image.bytes_base64)),
            target,
            reference,
            text: image.caption,
        })
        .collect::<Vec<ScreenedImage>>();
    if survivors.len() != verdict.keep.len() {
        return Err(TrainError::Failed(
            "screening returned an invalid survivor index".to_string(),
        ));
    }
    let described = if edit {
        for image in &mut survivors {
            image.text = image.text.trim().to_string();
        }
        Vec::new()
    } else {
        let mut drafts = survivors
            .iter()
            .map(|image| {
                Ok::<(String, String), TrainError>((
                    image.url.clone().ok_or_else(|| {
                        TrainError::Failed(
                            "identity survivor is missing its caption input".to_string(),
                        )
                    })?,
                    image.text.clone(),
                ))
            })
            .collect::<Result<Vec<(String, String)>, TrainError>>()?;
        let described = Captioner::new(config, litellm_host, litellm_key)
            .fill(&mut drafts, trigger)
            .await;
        for (image, (_, caption)) in survivors.iter_mut().zip(drafts) {
            image.text = caption;
        }
        described
    };
    let findings = crate::dataset::inspect(&described, survivors.len());
    let mut attempt = Attempt::create(&config.models_dir)?;
    let loras = ensure_child_directory(&config.models_dir, "loras")?;
    let output = loras.join(&filename);
    let output_sidecar = sidecar_path(&output);
    validate_output(&loras, &output)?;
    validate_output(&loras, &output_sidecar)?;
    let targets = ensure_child_directory(&attempt.root, "targets")?;
    let controls = edit
        .then(|| ensure_child_directory(&attempt.root, "control_1"))
        .transpose()?;
    let mut captions = HashMap::with_capacity(survivors.len());
    for (index, image) in survivors.iter().enumerate() {
        debug_assert_eq!(image.original, verdict.keep[index]);
        let stem = format!("{index:04}");
        write_new(
            &attempt.root,
            &targets.join(format!("{stem}.png")),
            &image.target,
        )?;
        let text = if edit {
            image.text.clone()
        } else {
            identity_caption(&image.text, trigger)
        };
        write_new(
            &attempt.root,
            &targets.join(format!("{stem}.txt")),
            text.as_bytes(),
        )?;
        captions.insert(format!("{stem}.png"), text);
        if let (Some(controls), Some(reference)) = (&controls, &image.reference) {
            write_new(
                &attempt.root,
                &controls.join(format!("{stem}.png")),
                reference,
            )?;
        }
    }
    let staged = attempt.output();
    let staged_name = attempt.staged_name();
    let run = if let Some(command) = config.train_command.as_deref() {
        let run = Run::new();
        attempt.register(&run)?;
        let mut process = Command::new("sh");
        process
            .arg("-c")
            .arg(command)
            .env("ZONE_TRAIN_NAME", &staged_name)
            .env("ZONE_TRAIN_FINAL_NAME", &filename)
            .env("ZONE_TRAIN_BASE", &recipe.id)
            .env("ZONE_TRAIN_DIR", &attempt.root)
            .env("ZONE_TRAIN_OUTPUT", &staged)
            .env("ZONE_TRAIN_TRIGGER", trigger)
            .env("ZONE_TRAIN_IMAGE_COUNT", survivors.len().to_string())
            .env(
                "ZONE_TRAIN_ARCHITECTURE",
                crate::train::architecture(&model),
            )
            .env("ZONE_TRAIN_FOLDER", &run.folder)
            .env("ZONE_TRAIN_ARTIFACT", &run.artifact)
            .env("ZONE_TRAIN_DEFER_CLEANUP", "1")
            .env("COMFYUI_BASE_URL", &config.base_url)
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
            .stderr(Stdio::piped());
        match &model {
            TrainingModel::Flux { checkpoint } => {
                process.env("ZONE_TRAIN_CHECKPOINT", checkpoint);
            }
            TrainingModel::QwenEdit { unet, clip, vae } => {
                process
                    .env("ZONE_TRAIN_UNET", unet)
                    .env("ZONE_TRAIN_CLIP", clip)
                    .env("ZONE_TRAIN_VAE", vae);
            }
        }
        let status = match process.status().await {
            Ok(status) => status,
            Err(error) => {
                crate::train::cleanup(config, &run).await;
                return Err(failed(error));
            }
        };
        if !status.success() {
            crate::train::cleanup(config, &run).await;
            return Err(TrainError::Failed(format!(
                "trainer exited {}",
                status.code().unwrap_or(1)
            )));
        }
        run
    } else {
        crate::train::run(config, &model, &attempt.root, &staged, survivors.len()).await?
    };
    attempt.register(&run)?;
    if require_regular_file(&attempt.root, &staged).is_err() {
        crate::train::cleanup(config, &run).await;
        return Err(TrainError::Failed(
            "trainer did not write a regular LoRA file".to_string(),
        ));
    }
    let quality = crate::quality::select(config, &model, &run, &staged, &captions).await;
    require_regular_file(&attempt.root, &staged).map_err(|_| {
        TrainError::Failed("quality selection did not leave a regular LoRA file".to_string())
    })?;
    let staged_sidecar = catalog
        .adapter_recipe_for_base(recipe.hf_bases.first().unwrap_or(&recipe.id))
        .or_else(|| catalog.adapter_recipe_for_filename(&filename))
        .map(|adapter| {
            let path = sidecar_path(&staged);
            let bytes = serde_json::to_vec_pretty(&WeightSidecar {
                recipe_id: adapter.id.clone(),
                hf_base: recipe.hf_bases.first().cloned(),
            })
            .map_err(|error| TrainError::Failed(error.to_string()))?;
            write_new(&attempt.root, &path, &bytes)?;
            Ok::<PathBuf, TrainError>(path)
        })
        .transpose()?;
    validate_output(&loras, &output)?;
    validate_output(&loras, &output_sidecar)?;
    atomic_promote(
        &attempt.root,
        &staged,
        staged_sidecar.as_deref(),
        &loras,
        &output,
        &output_sidecar,
    )?;
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

fn validate_pairing(images: &[TrainImage], model: &TrainingModel) -> Result<(), TrainError> {
    if matches!(model, TrainingModel::QwenEdit { .. }) {
        if images.iter().any(|image| image.before_base64.is_none()) {
            return Err(TrainError::Invalid(
                "edit training needs one reference image for every target",
            ));
        }
        if images.iter().any(|image| image.caption.trim().is_empty()) {
            return Err(TrainError::Invalid(
                "edit training needs a nonempty instruction for every image pair",
            ));
        }
    } else if images.iter().any(|image| image.before_base64.is_some()) {
        return Err(TrainError::Invalid(
            "reference images are only supported by edit training bases",
        ));
    }
    Ok(())
}

fn validate_verdict(verdict: &crate::screening::Verdict, count: usize) -> Result<(), TrainError> {
    let mut seen = vec![false; count];
    for index in verdict
        .keep
        .iter()
        .copied()
        .chain(verdict.drop.iter().map(|(index, _)| *index))
    {
        let Some(slot) = seen.get_mut(index) else {
            return Err(TrainError::Failed(
                "screening returned an invalid image index".to_string(),
            ));
        };
        if std::mem::replace(slot, true) {
            return Err(TrainError::Failed(
                "screening returned an image more than once".to_string(),
            ));
        }
    }
    if seen.iter().any(|seen| !seen)
        || verdict
            .keep
            .windows(2)
            .any(|indices| indices[0] >= indices[1])
    {
        return Err(TrainError::Failed(
            "screening did not partition the training set".to_string(),
        ));
    }
    Ok(())
}

fn identity_caption(caption: &str, trigger: &str) -> String {
    match (!trigger.is_empty(), caption.contains(trigger)) {
        (true, false) => {
            format!("{trigger}, {}", caption.trim())
        }
        _ => caption.trim().to_string(),
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

fn final_filename(name: &str) -> Result<String, TrainError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(TrainError::Invalid("invalid LoRA name"));
    }
    let filename = if name.ends_with(".safetensors") {
        name.to_string()
    } else {
        format!("{name}.safetensors")
    };
    let filename = sanitize_weight_filename(&filename)
        .map_err(|_| TrainError::Invalid("invalid LoRA name"))?;
    let path = Path::new(&filename);
    if path.file_name().and_then(|value| value.to_str()) != Some(filename.as_str())
        || path.components().count() != 1
        || filename.trim_end_matches(".safetensors").is_empty()
        || filename.chars().any(char::is_control)
    {
        return Err(TrainError::Invalid("invalid LoRA name"));
    }
    Ok(filename)
}

fn failed(error: std::io::Error) -> TrainError {
    TrainError::Failed(error.to_string())
}

fn require_directory(path: &Path, label: &'static str) -> Result<(), TrainError> {
    let metadata = fs::symlink_metadata(path).map_err(failed)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(TrainError::Invalid(label));
    }
    Ok(())
}

fn ensure_child_directory(parent: &Path, name: &str) -> Result<PathBuf, TrainError> {
    require_directory(parent, "training path is not a directory")?;
    let path = parent.join(name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(TrainError::Invalid("training path is not a safe directory"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&path).map_err(failed)?;
        }
        Err(error) => return Err(failed(error)),
    }
    require_confined_directory(parent, &path)?;
    Ok(path)
}

fn require_confined_directory(parent: &Path, child: &Path) -> Result<(), TrainError> {
    let parent = fs::canonicalize(parent).map_err(failed)?;
    let child = fs::canonicalize(child).map_err(failed)?;
    if !child.starts_with(&parent) || child == parent {
        return Err(TrainError::Invalid(
            "training path escapes its configured root",
        ));
    }
    Ok(())
}

fn require_regular_file(root: &Path, path: &Path) -> Result<(), TrainError> {
    let metadata = fs::symlink_metadata(path).map_err(failed)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TrainError::Invalid("training output is not a regular file"));
    }
    let root = fs::canonicalize(root).map_err(failed)?;
    let path = fs::canonicalize(path).map_err(failed)?;
    if !path.starts_with(root) {
        return Err(TrainError::Invalid("training output escapes its attempt"));
    }
    Ok(())
}

fn write_new(root: &Path, path: &Path, bytes: &[u8]) -> Result<(), TrainError> {
    let root = fs::canonicalize(root).map_err(failed)?;
    let parent = path
        .parent()
        .ok_or(TrainError::Invalid("training path has no parent"))?;
    let parent = fs::canonicalize(parent).map_err(failed)?;
    if !parent.starts_with(&root) {
        return Err(TrainError::Invalid("training path escapes its attempt"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(failed)?;
    file.write_all(bytes).map_err(failed)
}

fn validate_output(parent: &Path, output: &Path) -> Result<(), TrainError> {
    require_confined_directory(
        parent
            .parent()
            .ok_or(TrainError::Invalid("LoRA directory has no parent"))?,
        parent,
    )?;
    if output.parent() != Some(parent) {
        return Err(TrainError::Invalid("LoRA output escapes its directory"));
    }
    match fs::symlink_metadata(output) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(TrainError::Invalid("LoRA output is not a regular file"))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(failed(error)),
    }
}

/// Rename is the adapter commit point. Both paths sit below the configured
/// models root, and a hard link keeps the previous adapter recoverable until
/// its staged sidecar has also reached the final name.
fn atomic_promote(
    attempt: &Path,
    staged: &Path,
    staged_sidecar: Option<&Path>,
    parent: &Path,
    output: &Path,
    output_sidecar: &Path,
) -> Result<(), TrainError> {
    require_regular_file(attempt, staged)?;
    validate_output(parent, output)?;
    if let Some(staged_sidecar) = staged_sidecar {
        require_regular_file(attempt, staged_sidecar)?;
        validate_output(parent, output_sidecar)?;
    }
    let previous = if output.exists() {
        let previous = attempt.join("previous.safetensors");
        fs::hard_link(output, &previous).map_err(failed)?;
        Some(previous)
    } else {
        None
    };
    fs::rename(staged, output).map_err(failed)?;
    let Some(staged_sidecar) = staged_sidecar else {
        return Ok(());
    };
    if let Err(error) = fs::rename(staged_sidecar, output_sidecar) {
        let restored = match previous {
            Some(previous) => fs::rename(previous, output),
            None => fs::remove_file(output),
        };
        return match restored {
            Ok(()) => Err(failed(error)),
            Err(restore) => Err(TrainError::Failed(format!(
                "sidecar promotion failed ({error}); adapter rollback failed ({restore})"
            ))),
        };
    }
    Ok(())
}

fn sidecar_path(weight: &Path) -> PathBuf {
    let filename = weight
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("weight");
    weight.with_file_name(format!("{filename}.zone.json"))
}

fn remove_entry(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

fn safe_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::screening::{Rejection, Verdict};
    use base64::Engine;
    use serde_json::{Value, json};
    use wiremock::{
        Mock, MockServer, Request, ResponseTemplate,
        matchers::{method, path, path_regex},
    };

    fn harness(command: &str) -> (tempfile::TempDir, Config) {
        let root = tempfile::tempdir().expect("temporary ComfyUI root");
        let models = root.path().join("models");
        fs::create_dir(&models).expect("models directory");
        let config = Config {
            base_url: "http://127.0.0.1:9".to_string(),
            models_dir: models,
            train_command: Some(command.to_string()),
            train_timeout_secs: 2,
            ..Default::default()
        };
        (root, config)
    }

    fn encoded(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn image(target: &str, caption: &str, reference: Option<&str>) -> TrainImage {
        TrainImage {
            filename: format!("{target}.png"),
            caption: caption.to_string(),
            bytes_base64: encoded(target.as_bytes()),
            before_base64: reference.map(|value| encoded(value.as_bytes())),
        }
    }

    fn identity(name: &str) -> TrainRequest {
        TrainRequest {
            name: name.to_string(),
            base: "flux-schnell".to_string(),
            trigger: Some("ohwx".to_string()),
            images: vec![image("target", "a portrait", None)],
        }
    }

    fn edit(images: Vec<TrainImage>) -> TrainRequest {
        TrainRequest {
            name: "edit-style".to_string(),
            base: "qwen-image-edit".to_string(),
            trigger: None,
            images,
        }
    }

    fn keep_all(images: &[Vec<u8>], _resolution: u32) -> Verdict {
        Verdict {
            keep: (0..images.len()).collect(),
            drop: Vec::new(),
        }
    }

    fn drop_middle(_images: &[Vec<u8>], _resolution: u32) -> Verdict {
        Verdict {
            keep: vec![0, 2],
            drop: vec![(1, Rejection::Duplicate)],
        }
    }

    fn training_entries(config: &Config) -> Vec<PathBuf> {
        let training = config.models_dir.join("training");
        let Ok(entries) = fs::read_dir(training) else {
            return Vec::new();
        };
        entries.flatten().map(|entry| entry.path()).collect()
    }

    #[tokio::test]
    async fn train_writes_adapter_with_configured_command() {
        let (_root, config) = harness("printf lora > \"$ZONE_TRAIN_OUTPUT\"");
        let outcome = train(&config, String::new(), String::new(), identity("my-style"))
            .await
            .unwrap();
        assert_eq!(outcome.path.file_name().unwrap(), "my-style.safetensors");
        assert_eq!(fs::read(&outcome.path).unwrap(), b"lora");
        assert!(
            outcome.quality.is_none(),
            "a run with no reachable probe reports no score instead of failing"
        );
        assert!(training_entries(&config).is_empty());
    }

    #[test]
    fn caption_prefixes_trigger_for_identity() {
        assert_eq!(identity_caption("a portrait", "ohwx"), "ohwx, a portrait");
        assert_eq!(
            identity_caption("ohwx, a portrait", "ohwx"),
            "ohwx, a portrait"
        );
    }

    #[tokio::test]
    async fn train_rejects_missing_trigger() {
        let (_root, config) = harness("printf lora > \"$ZONE_TRAIN_OUTPUT\"");
        let mut request = identity("my-style");
        request.trigger = None;
        let error = train(&config, String::new(), String::new(), request)
            .await
            .unwrap_err();
        assert!(matches!(error, TrainError::Invalid(_)));
        assert!(training_entries(&config).is_empty());
    }

    #[test]
    fn final_adapter_name_is_one_nonempty_path_component() {
        for invalid in [
            "",
            "   ",
            ".safetensors",
            "../escape",
            "folder/escape",
            r"folder\escape",
            "line\nbreak",
        ] {
            assert!(
                final_filename(invalid).is_err(),
                "{invalid:?} must not become a filesystem path"
            );
        }
        assert_eq!(
            final_filename(" studio-style ").unwrap(),
            "studio-style.safetensors"
        );
    }

    #[tokio::test]
    async fn edit_training_requires_a_decodable_reference_and_instruction_for_every_pair() {
        let (_root, config) = harness("printf lora > \"$ZONE_TRAIN_OUTPUT\"");
        let cases = [
            edit(vec![image("target", "replace the sky", None)]),
            edit(vec![TrainImage {
                before_base64: Some("not base64".to_string()),
                ..image("target", "replace the sky", Some("reference"))
            }]),
            edit(vec![image("target", "  ", Some("reference"))]),
        ];
        for request in cases {
            let error =
                train_with_screening(&config, String::new(), String::new(), request, keep_all)
                    .await
                    .unwrap_err();
            assert!(matches!(error, TrainError::Invalid(_)), "got {error:?}");
        }
        assert!(training_entries(&config).is_empty());
    }

    #[tokio::test]
    async fn edit_screening_reindexes_targets_references_and_instructions_together() {
        let command = r#"
            test "$ZONE_TRAIN_IMAGE_COUNT" = "2" || exit 11
            test "$ZONE_TRAIN_FINAL_NAME" = "edit-style.safetensors" || exit 12
            stem=${ZONE_TRAIN_NAME%.safetensors}
            test "$stem" != "$ZONE_TRAIN_NAME" || exit 13
            test "$stem" = "$(basename "$ZONE_TRAIN_DIR")" || exit 14
            test "$ZONE_TRAIN_ARCHITECTURE" = "qwen_edit" || exit 23
            test "$ZONE_TRAIN_UNET" = "qwen_image_edit_2511_fp8mixed.safetensors" || exit 24
            test "$ZONE_TRAIN_CLIP" = "qwen_2.5_vl_7b_fp8_scaled.safetensors" || exit 25
            test "$ZONE_TRAIN_VAE" = "qwen_image_vae.safetensors" || exit 26
            test "$ZONE_TRAIN_DEFER_CLEANUP" = "1" || exit 27
            folder_id=${ZONE_TRAIN_FOLDER#zone-train-}
            artifact_id=${ZONE_TRAIN_ARTIFACT#zone-lora-}
            test "$folder_id" != "$ZONE_TRAIN_FOLDER" || exit 28
            test "$artifact_id" != "$ZONE_TRAIN_ARTIFACT" || exit 29
            test "$folder_id" != "$artifact_id" || exit 30
            test "$(cat "$ZONE_TRAIN_DIR/targets/0000.png")" = "target-zero" || exit 15
            test "$(cat "$ZONE_TRAIN_DIR/control_1/0000.png")" = "reference-zero" || exit 16
            test "$(cat "$ZONE_TRAIN_DIR/targets/0000.txt")" = "change zero" || exit 17
            test "$(cat "$ZONE_TRAIN_DIR/targets/0001.png")" = "target-two" || exit 18
            test "$(cat "$ZONE_TRAIN_DIR/control_1/0001.png")" = "reference-two" || exit 19
            test "$(cat "$ZONE_TRAIN_DIR/targets/0001.txt")" = "change two" || exit 20
            test ! -e "$ZONE_TRAIN_DIR/targets/0002.png" || exit 21
            test ! -e "$ZONE_TRAIN_DIR/control_1/0002.png" || exit 22
            printf trained > "$ZONE_TRAIN_OUTPUT"
        "#;
        let (_root, mut config) = harness(command);
        config.caption_model = "vision".to_string();
        let captioner = wiremock::MockServer::start().await;
        let outcome = train_with_screening(
            &config,
            captioner.uri(),
            "key".to_string(),
            edit(vec![
                image("target-zero", " change zero ", Some("reference-zero")),
                image("target-one", "change one", Some("reference-one")),
                image("target-two", " change two ", Some("reference-two")),
            ]),
            drop_middle,
        )
        .await
        .expect("screened edit run");

        assert_eq!(outcome.screening.kept, 2);
        assert_eq!(outcome.screening.dropped.len(), 1);
        assert_eq!(outcome.screening.dropped[0].filename, "target-one.png");
        assert!(
            outcome
                .dataset
                .iter()
                .any(|finding| finding.detail.starts_with("Only 2 images")),
            "dataset advice must use the screened count: {:?}",
            outcome.dataset
        );
        assert_eq!(fs::read(&outcome.path).unwrap(), b"trained");
        assert!(
            captioner.received_requests().await.unwrap().is_empty(),
            "edit instructions must never be sent through the identity captioner"
        );
        assert!(training_entries(&config).is_empty());
    }

    #[tokio::test]
    async fn screened_qwen_pipeline_posts_one_aligned_model_and_pair_contract() {
        let server = MockServer::start().await;
        let prompt = Uuid::new_v4();
        let root = tempfile::tempdir().expect("temporary ComfyUI root");
        let models = root.path().join("models");
        let input = root.path().join("input");
        let produced = root.path().join("output/loras");
        fs::create_dir(&models).unwrap();
        fs::create_dir(&input).unwrap();
        fs::create_dir_all(&produced).unwrap();

        let other_folder = format!("zone-train-{}", Uuid::new_v4());
        let other_artifact = format!("zone-lora-{}", Uuid::new_v4());
        fs::create_dir(input.join(&other_folder)).unwrap();
        fs::write(input.join(&other_folder).join("kept"), b"other input").unwrap();
        fs::write(
            produced.join(format!("{other_artifact}.safetensors")),
            b"other output",
        )
        .unwrap();

        let produced_for_prompt = produced.clone();
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .respond_with(move |request: &Request| {
                let body: Value = request.body_json().unwrap();
                let graph = &body["prompt"];
                if graph.as_object().is_some_and(|nodes| {
                    nodes
                        .values()
                        .any(|node| node["class_type"] == "ZoneTrainLoRA")
                }) {
                    let artifact = graph["7"]["inputs"]["save_name"].as_str().unwrap();
                    fs::write(
                        produced_for_prompt.join(format!("{artifact}.safetensors")),
                        b"remote final",
                    )
                    .unwrap();
                    fs::write(
                        produced_for_prompt.join(format!("{artifact}-step11.safetensors")),
                        b"remote checkpoint",
                    )
                    .unwrap();
                }
                ResponseTemplate::new(200).set_body_json(json!({
                    "prompt_id": prompt,
                    "number": 0.0,
                    "node_errors": {}
                }))
            })
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/history/[0-9a-f-]+$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                prompt.to_string(): {
                    "status": {"completed": true, "status_str": "success"},
                    "outputs": {}
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/view"))
            .respond_with(|request: &Request| {
                let filename = request
                    .url
                    .query_pairs()
                    .find_map(|(name, value)| (name == "filename").then(|| value.into_owned()))
                    .unwrap();
                ResponseTemplate::new(200)
                    .insert_header("Content-Disposition", format!("filename=\"{filename}\""))
                    .insert_header("Content-Type", "application/octet-stream")
                    .set_body_bytes(vec![7; 10_001])
            })
            .mount(&server)
            .await;

        let config = Config {
            enabled: true,
            base_url: server.uri(),
            models_dir: models,
            poll_interval_ms: 1,
            train_command: None,
            train_timeout_secs: 2,
            ..Default::default()
        };
        let outcome = train_with_screening(
            &config,
            String::new(),
            String::new(),
            edit(vec![
                image("target-zero", " change zero ", Some("reference-zero")),
                image("target-one", "change one", Some("reference-one")),
                image("target-two", " change two ", Some("reference-two")),
            ]),
            drop_middle,
        )
        .await
        .expect("native Qwen run");

        assert_eq!(outcome.screening.kept, 2);
        assert_eq!(fs::read(&outcome.path).unwrap(), vec![7; 10_001]);
        let requests = server.received_requests().await.unwrap();
        let graphs = requests
            .iter()
            .filter(|request| request.url.path() == "/prompt")
            .map(|request| request.body_json::<Value>().unwrap()["prompt"].clone())
            .collect::<Vec<Value>>();
        let training = graphs
            .iter()
            .find(|graph| {
                graph.as_object().is_some_and(|nodes| {
                    nodes
                        .values()
                        .any(|node| node["class_type"] == "ZoneTrainLoRA")
                })
            })
            .expect("posted training graph");
        assert_eq!(training["1"]["class_type"], "UNETLoader");
        assert_eq!(training["2"]["class_type"], "CLIPLoader");
        assert_eq!(training["2"]["inputs"]["type"], "qwen_image");
        assert_eq!(training["3"]["class_type"], "VAELoader");
        assert_eq!(training["5"]["class_type"], "VAEEncode");
        assert_eq!(training["6"]["class_type"], "TextEncodeQwenImageEditPlus");
        let serialized = training.to_string();
        assert!(!serialized.contains("CheckpointLoaderSimple"));
        assert!(!serialized.contains("MakeTrainingDataset"));

        let manifest: Value =
            serde_json::from_str(training["4"]["inputs"]["manifest_json"].as_str().unwrap())
                .unwrap();
        let expected = json!({
            "schema_version": 1,
            "architecture": "qwen_edit",
            "pairs": [
                {
                    "index": 0,
                    "target": "targets/0000.png",
                    "reference": "control_1/0000.png",
                    "instruction": "change zero"
                },
                {
                    "index": 1,
                    "target": "targets/0001.png",
                    "reference": "control_1/0001.png",
                    "instruction": "change two"
                }
            ]
        });
        assert_eq!(manifest, expected);

        let folder = training["4"]["inputs"]["folder"]
            .as_str()
            .unwrap()
            .to_string();
        let artifact = training["7"]["inputs"]["save_name"]
            .as_str()
            .unwrap()
            .to_string();
        Run {
            folder: folder.clone(),
            artifact: artifact.clone(),
        }
        .validate()
        .unwrap();
        assert_ne!(
            folder.trim_start_matches("zone-train-"),
            artifact.trim_start_matches("zone-lora-")
        );

        let quality = graphs
            .iter()
            .find(|graph| {
                graph.as_object().is_some_and(|nodes| {
                    nodes
                        .values()
                        .any(|node| node["class_type"] == "ZoneProbeLoss")
                })
            })
            .expect("posted quality graph");
        let quality_manifest: Value =
            serde_json::from_str(quality["4"]["inputs"]["manifest_json"].as_str().unwrap())
                .unwrap();
        assert_eq!(quality_manifest, expected);
        assert_eq!(quality["1"]["class_type"], "UNETLoader");
        assert_eq!(quality["2"]["inputs"]["type"], "qwen_image");
        assert_eq!(quality["3"]["class_type"], "VAELoader");
        assert_eq!(quality["6"]["class_type"], "TextEncodeQwenImageEditPlus");

        assert!(!input.join(folder).exists());
        assert!(!produced.join(format!("{artifact}.safetensors")).exists());
        assert!(
            !produced
                .join(format!("{artifact}-step11.safetensors"))
                .exists()
        );
        assert_eq!(
            fs::read(input.join(other_folder).join("kept")).unwrap(),
            b"other input"
        );
        assert_eq!(
            fs::read(produced.join(format!("{other_artifact}.safetensors"))).unwrap(),
            b"other output"
        );
        assert!(training_entries(&config).is_empty());
    }

    #[tokio::test]
    async fn non_edit_training_rejects_controls_and_never_stages_a_control_folder() {
        let command = r#"
            test ! -e "$ZONE_TRAIN_DIR/control_1" || exit 31
            test "$(cat "$ZONE_TRAIN_DIR/targets/0000.txt")" = "ohwx, a portrait" || exit 32
            printf trained > "$ZONE_TRAIN_OUTPUT"
        "#;
        let (_root, config) = harness(command);
        let outcome = train_with_screening(
            &config,
            String::new(),
            String::new(),
            identity("identity"),
            keep_all,
        )
        .await
        .expect("identity run");
        assert_eq!(fs::read(outcome.path).unwrap(), b"trained");

        let mut paired = identity("paired-identity");
        paired.images[0].before_base64 = Some(encoded(b"reference"));
        let error = train_with_screening(&config, String::new(), String::new(), paired, keep_all)
            .await
            .unwrap_err();
        assert!(matches!(error, TrainError::Invalid(_)));
        assert!(training_entries(&config).is_empty());
    }

    #[tokio::test]
    async fn a_failed_same_name_attempt_preserves_the_previous_adapter_and_is_not_reused() {
        let (_root, mut config) = harness("printf first > \"$ZONE_TRAIN_OUTPUT\"");
        let first = train_with_screening(
            &config,
            String::new(),
            String::new(),
            identity("stable"),
            keep_all,
        )
        .await
        .expect("first attempt");
        let original = fs::read(&first.path).unwrap();

        config.train_command = Some("printf partial > \"$ZONE_TRAIN_OUTPUT\"; exit 41".to_string());
        let error = train_with_screening(
            &config,
            String::new(),
            String::new(),
            identity("stable"),
            keep_all,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, TrainError::Failed(_)));
        assert_eq!(fs::read(&first.path).unwrap(), original);
        assert!(training_entries(&config).is_empty());

        config.train_command = Some(
            "test ! -e \"$ZONE_TRAIN_OUTPUT\" || exit 42; printf third > \"$ZONE_TRAIN_OUTPUT\""
                .to_string(),
        );
        let third = train_with_screening(
            &config,
            String::new(),
            String::new(),
            identity("stable"),
            keep_all,
        )
        .await
        .expect("fresh third attempt");
        assert_eq!(fs::read(third.path).unwrap(), b"third");
        assert!(training_entries(&config).is_empty());
    }

    #[tokio::test]
    async fn attempt_cleanup_never_removes_another_attempt() {
        let (_root, config) = harness("printf trained > \"$ZONE_TRAIN_OUTPUT\"");
        let other = config
            .models_dir
            .join("training")
            .join(Uuid::new_v4().to_string());
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("marker"), b"owned elsewhere").unwrap();

        train_with_screening(
            &config,
            String::new(),
            String::new(),
            identity("isolated"),
            keep_all,
        )
        .await
        .expect("isolated attempt");

        assert_eq!(fs::read(other.join("marker")).unwrap(), b"owned elsewhere");
        assert_eq!(training_entries(&config), vec![other]);
    }

    #[tokio::test]
    async fn unsupported_recipe_requests_fail_closed_before_creating_an_attempt() {
        let (_root, config) = harness("printf trained > \"$ZONE_TRAIN_OUTPUT\"");
        let mut request = identity("unsupported");
        request.base = "sd15".to_string();

        let error = train_with_screening(&config, String::new(), String::new(), request, keep_all)
            .await
            .unwrap_err();

        assert!(matches!(error, TrainError::Invalid(_)));
        assert!(training_entries(&config).is_empty());
        assert!(
            !config
                .models_dir
                .join("loras/unsupported.safetensors")
                .exists()
        );
    }

    #[test]
    fn advertised_bases_are_ready_and_have_a_typed_training_architecture() {
        let (_root, config) = harness("unused");
        fs::create_dir_all(config.models_dir.join("checkpoints")).unwrap();
        fs::create_dir_all(config.models_dir.join("diffusion_models")).unwrap();
        fs::create_dir_all(config.models_dir.join("text_encoders")).unwrap();
        fs::create_dir_all(config.models_dir.join("vae")).unwrap();
        fs::write(
            config
                .models_dir
                .join("checkpoints/flux1-schnell-fp8.safetensors"),
            b"flux",
        )
        .unwrap();
        fs::write(
            config
                .models_dir
                .join("checkpoints/sd15-custom.safetensors"),
            b"sd15",
        )
        .unwrap();
        fs::write(
            config
                .models_dir
                .join("diffusion_models/qwen_image_edit_2511_fp8mixed.safetensors"),
            b"qwen",
        )
        .unwrap();
        fs::write(
            config
                .models_dir
                .join("text_encoders/qwen_2.5_vl_7b_fp8_scaled.safetensors"),
            b"clip",
        )
        .unwrap();
        fs::write(
            config.models_dir.join("vae/qwen_image_vae.safetensors"),
            b"vae",
        )
        .unwrap();
        let catalog = RecipeCatalog::packaged().unwrap();

        let bases = available_bases(&catalog, &config.models_dir);

        assert!(bases.iter().any(|base| base.id == "flux-schnell"));
        assert!(
            bases
                .iter()
                .any(|base| base.id == "qwen-image-edit" && base.edit)
        );
        assert!(
            bases.iter().all(|base| base.id != "sd15"),
            "a runnable generation graph is not automatically a trainable architecture"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_symlinked_training_root_cannot_escape_the_models_directory() {
        use std::os::unix::fs::symlink;

        let (root, config) = harness("printf escaped > \"$ZONE_TRAIN_OUTPUT\"");
        let outside = root.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, config.models_dir.join("training")).unwrap();

        let error = train_with_screening(
            &config,
            String::new(),
            String::new(),
            identity("escape"),
            keep_all,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, TrainError::Invalid(_)));
        assert!(fs::read_dir(outside).unwrap().next().is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_symlinked_final_adapter_is_rejected_without_touching_its_target() {
        use std::os::unix::fs::symlink;

        let (root, config) = harness("printf replacement > \"$ZONE_TRAIN_OUTPUT\"");
        let loras = config.models_dir.join("loras");
        fs::create_dir(&loras).unwrap();
        let outside = root.path().join("outside.safetensors");
        fs::write(&outside, b"outside").unwrap();
        symlink(&outside, loras.join("linked.safetensors")).unwrap();

        let error = train_with_screening(
            &config,
            String::new(),
            String::new(),
            identity("linked"),
            keep_all,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, TrainError::Invalid(_)));
        assert_eq!(fs::read(outside).unwrap(), b"outside");
        assert!(training_entries(&config).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn dataset_writes_do_not_follow_a_symlinked_file() {
        use std::os::unix::fs::symlink;

        let (root, config) = harness("unused");
        let attempt = Attempt::create(&config.models_dir).unwrap();
        let targets = ensure_child_directory(&attempt.root, "targets").unwrap();
        let outside = root.path().join("outside.png");
        fs::write(&outside, b"outside").unwrap();
        let staged = targets.join("0000.png");
        symlink(&outside, &staged).unwrap();

        assert!(write_new(&attempt.root, &staged, b"replacement").is_err());
        assert_eq!(fs::read(outside).unwrap(), b"outside");
    }
}
