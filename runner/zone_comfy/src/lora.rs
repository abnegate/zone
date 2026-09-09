//! Packaged LoRA training jobs. Default path posts ZoneTrainLoRA to ComfyUI.

use crate::caption::{Captioner, Draft};
use crate::config::Config;
use crate::inventory::{PUBLICATION_DIRECTORY, WeightDocument, WeightSidecar, publication_marker};
use crate::quality::Quality;
use crate::recipe::{RecipeCatalog, TrainingModel, sanitize_weight_filename};
use crate::subject::{CENTRE, Subject};
use crate::train::Run;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, LazyLock, Mutex, Weak};
use tokio::process::Command;
use uuid::Uuid;
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
#[cfg_attr(test, derive(PartialEq))]
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
    /// Base64 of the crop, kept only for identity runs, which caption it.
    encoded: Option<String>,
    group: usize,
}

struct Attempt {
    id: String,
    root: PathBuf,
    input: Option<PathBuf>,
    folder: Option<String>,
    artifact: Option<String>,
    produced: Option<PathBuf>,
}

static PUBLICATIONS: LazyLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

impl Attempt {
    fn create(models: &Path) -> Result<Self, TrainError> {
        require_directory(models, "models directory")?;
        let id = Uuid::new_v4().to_string();
        let training = ensure_child_directory(models, "training")?;
        sync_directory(models)?;
        let root = training.join(&id);
        fs::create_dir(&root).map_err(failed)?;
        sync_directory(&training)?;
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
            edit: matches!(recipe.training_model(), Ok(TrainingModel::QwenEdit { .. })),
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
    let side = crate::train::packaged_config()?.resolution();
    let verdict = screening(&decoded, side);
    validate_verdict(&verdict, request.images.len())?;
    let dropped = verdict
        .drop
        .iter()
        .map(|(index, rejection)| Dropped {
            filename: request.images[*index].filename.clone(),
            reason: *rejection,
        })
        .collect::<Vec<Dropped>>();
    // Cropping comes before captioning so the vision model describes the image
    // that will be trained on. Captioning the upload instead would have it
    // describe a background the crop is about to remove.
    let subject = Subject::shared(config);
    let groups = shots(&request.images);
    let mut survivors = request
        .images
        .iter()
        .zip(&groups)
        .enumerate()
        .filter(|(index, _)| verdict.keep.binary_search(index).is_ok())
        .map(|(original, (image, group))| {
            let framed = frame(&subject, image, side)?;
            Ok(ScreenedImage {
                original,
                encoded: (!edit).then(|| framed.encoded()),
                target: framed.target,
                reference: framed.control,
                text: image.caption.clone(),
                group: *group,
            })
        })
        .collect::<Result<Vec<ScreenedImage>, TrainError>>()?;
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
                let encoded = image.encoded.as_deref().ok_or_else(|| {
                    TrainError::Failed("identity survivor is missing its caption input".to_string())
                })?;
                Ok::<Draft, TrainError>(Draft::new(CROP, encoded, &image.text, image.group))
            })
            .collect::<Result<Vec<Draft>, TrainError>>()?;
        let described = Captioner::new(config, litellm_host, litellm_key)
            .fill(&mut drafts, trigger)
            .await;
        for (image, draft) in survivors.iter_mut().zip(drafts) {
            image.text = draft.caption;
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
    let run = if let Some(command) = config.train_command.as_deref() {
        let run = Run::new();
        attempt.register(&run)?;
        let mut process = Command::new("sh");
        process
            .arg("-c")
            .arg(command)
            .env("ZONE_TRAIN_NAME", &filename)
            .env("ZONE_TRAIN_FINAL_NAME", &filename)
            .env("ZONE_TRAIN_ATTEMPT", &attempt.id)
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
    let adapter = recipe
        .training_adapter()
        .map_err(|_| TrainError::Invalid("training adapter mapping is missing"))?;
    let staged_sidecar = sidecar_path(&staged);
    let bytes = serde_json::to_vec_pretty(&WeightDocument {
        sidecar: WeightSidecar {
            recipe_id: adapter.recipe_id.clone(),
            hf_base: Some(adapter.hf_base.clone()),
        },
        generation: Some(attempt.id.clone()),
    })
    .map_err(|error| TrainError::Failed(error.to_string()))?;
    write_new(&attempt.root, &staged_sidecar, &bytes)?;
    validate_output(&loras, &output)?;
    validate_output(&loras, &output_sidecar)?;
    atomic_promote(
        &attempt.root,
        &staged,
        &staged_sidecar,
        &loras,
        &output,
        &output_sidecar,
        &attempt.id,
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
    match (!trigger.is_empty(), contains_phrase(caption, trigger)) {
        (true, false) => {
            format!("{trigger}, {}", caption.trim())
        }
        _ => caption.trim().to_string(),
    }
}

/// Trigger matching is Unicode-lowercase and requires a boundary around the
/// complete phrase. It never treats a trigger as a substring of another token.
fn contains_phrase(text: &str, phrase: &str) -> bool {
    let text = text.to_lowercase();
    let phrase = phrase.to_lowercase();
    text.match_indices(&phrase).any(|(start, matched)| {
        let before = text[..start].chars().next_back();
        let after = text[start + matched.len()..].chars().next();
        !before.is_some_and(is_trigger_character) && !after.is_some_and(is_trigger_character)
    })
}

fn is_trigger_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
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
    let named = |error: std::io::Error| TrainError::Failed(format!("{}: {error}", path.display()));
    let root = fs::canonicalize(root).map_err(&named)?;
    let parent = path
        .parent()
        .ok_or(TrainError::Invalid("training path has no parent"))?;
    let parent = fs::canonicalize(parent).map_err(&named)?;
    if !parent.starts_with(&root) {
        return Err(TrainError::Invalid("training path escapes its attempt"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(&named)?;
    file.write_all(bytes).map_err(&named)
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

#[derive(Debug, Deserialize, Serialize)]
struct Publication {
    schema_version: u32,
    filename: String,
    generation: String,
    previous_weight: bool,
    previous_sidecar: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PublicationPhase {
    Weights,
    Sidecar,
}

/// A durable marker hides the final name while its two files are replaced.
/// The process-local lock serializes tasks, and the file lock extends that
/// guarantee to cooperating Zone processes that share the models directory.
fn atomic_promote(
    attempt: &Path,
    staged: &Path,
    staged_sidecar: &Path,
    parent: &Path,
    output: &Path,
    output_sidecar: &Path,
    generation: &str,
) -> Result<(), TrainError> {
    promote_with(
        attempt,
        staged,
        staged_sidecar,
        parent,
        output,
        output_sidecar,
        generation,
        |_| Ok(()),
    )
}

#[allow(clippy::too_many_arguments)]
fn promote_with<F>(
    attempt: &Path,
    staged: &Path,
    staged_sidecar: &Path,
    parent: &Path,
    output: &Path,
    output_sidecar: &Path,
    generation: &str,
    mut observe: F,
) -> Result<(), TrainError>
where
    F: FnMut(PublicationPhase) -> Result<(), TrainError>,
{
    Uuid::parse_str(generation)
        .map_err(|_| TrainError::Invalid("training attempt id is not valid"))?;
    require_regular_file(attempt, staged)?;
    require_regular_file(attempt, staged_sidecar)?;
    if read_generation(staged_sidecar).as_deref() != Some(generation) {
        return Err(TrainError::Invalid(
            "staged adapter sidecar does not match its training attempt",
        ));
    }
    validate_output(parent, output)?;
    validate_output(parent, output_sidecar)?;
    sync_file(staged)?;
    sync_file(staged_sidecar)?;

    let key = fs::canonicalize(parent).map_err(failed)?.join(
        output
            .file_name()
            .ok_or(TrainError::Invalid("LoRA output has no filename"))?,
    );
    let local = publication_lock(&key);
    let _local = local
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let publications = ensure_child_directory(parent, PUBLICATION_DIRECTORY)?;
    sync_directory(parent)?;
    let filename = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(TrainError::Invalid("LoRA output has no filename"))?;
    let _file = lock_publication(&publications, filename)?;
    let marker = publication_marker(parent, filename)
        .ok_or(TrainError::Invalid("invalid LoRA publication name"))?;
    recover_publication(parent, output, output_sidecar, &marker)?;
    validate_output(parent, output)?;
    validate_output(parent, output_sidecar)?;

    let previous_weight = snapshot(output, &attempt.join("previous.safetensors"))?;
    let previous_sidecar = snapshot(
        output_sidecar,
        &attempt.join("previous.safetensors.zone.json"),
    )?;
    sync_directory(attempt)?;
    let publication = Publication {
        schema_version: 1,
        filename: filename.to_string(),
        generation: generation.to_string(),
        previous_weight,
        previous_sidecar,
    };
    let encoded =
        serde_json::to_vec(&publication).map_err(|error| TrainError::Failed(error.to_string()))?;
    write_new(&publications, &marker, &encoded)?;
    sync_file(&marker)?;
    sync_directory(&publications)?;

    let promoted = (|| {
        fs::rename(staged, output).map_err(failed)?;
        sync_directory(parent)?;
        observe(PublicationPhase::Weights)?;
        fs::rename(staged_sidecar, output_sidecar).map_err(failed)?;
        sync_directory(parent)?;
        observe(PublicationPhase::Sidecar)?;
        Ok::<(), TrainError>(())
    })();
    if let Err(error) = promoted {
        return rollback_publication(attempt, parent, output, output_sidecar, &marker, error);
    }
    if let Err(error) = fs::remove_file(&marker) {
        return rollback_publication(
            attempt,
            parent,
            output,
            output_sidecar,
            &marker,
            failed(error),
        );
    }
    // Both renames were synced before the marker was removed. If this final
    // directory sync fails, a restart may still see the marker and complete
    // the already-consistent generation through `recover_publication`.
    let _ = sync_directory(&publications);
    Ok(())
}

fn publication_lock(key: &Path) -> Arc<Mutex<()>> {
    let mut publications = PUBLICATIONS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    publications.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = publications.get(key).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    publications.insert(key.to_path_buf(), Arc::downgrade(&lock));
    lock
}

fn lock_publication(directory: &Path, filename: &str) -> Result<fs::File, TrainError> {
    let filename = sanitize_weight_filename(filename)
        .map_err(|_| TrainError::Invalid("invalid LoRA publication name"))?;
    let path = directory.join(format!("{filename}.lock"));
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(&path).map_err(failed)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(TrainError::Invalid(
                    "LoRA publication lock is not a regular file",
                ));
            }
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .map_err(failed)?
        }
        Err(error) => return Err(failed(error)),
    };
    file.lock().map_err(failed)?;
    Ok(file)
}

fn snapshot(source: &Path, backup: &Path) -> Result<bool, TrainError> {
    match fs::symlink_metadata(source) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
            TrainError::Invalid("LoRA publication target is not a regular file"),
        ),
        Ok(_) => {
            fs::hard_link(source, backup).map_err(failed)?;
            sync_file(backup)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(failed(error)),
    }
}

fn recover_publication(
    parent: &Path,
    output: &Path,
    output_sidecar: &Path,
    marker: &Path,
) -> Result<(), TrainError> {
    match fs::symlink_metadata(marker) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(TrainError::Invalid(
                "LoRA publication marker is not a regular file",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(failed(error)),
    }
    let contents = match fs::read(marker) {
        Ok(contents) => contents,
        Err(error) => return Err(failed(error)),
    };
    let publication: Publication = serde_json::from_slice(&contents)
        .map_err(|_| TrainError::Invalid("LoRA publication marker is invalid"))?;
    if publication.schema_version != 1
        || output.file_name().and_then(|name| name.to_str()) != Some(publication.filename.as_str())
        || Uuid::parse_str(&publication.generation).is_err()
    {
        return Err(TrainError::Invalid("LoRA publication marker is invalid"));
    }
    let complete = require_regular_output(parent, output).is_ok()
        && require_regular_output(parent, output_sidecar).is_ok()
        && read_generation(output_sidecar).as_deref() == Some(publication.generation.as_str());
    if complete {
        fs::remove_file(marker).map_err(failed)?;
        let publications = marker
            .parent()
            .ok_or(TrainError::Invalid("LoRA publication marker has no parent"))?;
        let _ = sync_directory(publications);
        return Ok(());
    }
    let models = parent
        .parent()
        .ok_or(TrainError::Invalid("LoRA directory has no parent"))?;
    let training = models.join("training");
    require_directory(&training, "training directory")?;
    let attempt = training.join(&publication.generation);
    require_confined_directory(&training, &attempt)?;
    restore_snapshot(
        &attempt,
        &attempt.join("previous.safetensors"),
        output,
        publication.previous_weight,
    )?;
    restore_snapshot(
        &attempt,
        &attempt.join("previous.safetensors.zone.json"),
        output_sidecar,
        publication.previous_sidecar,
    )?;
    sync_directory(parent)?;
    fs::remove_file(marker).map_err(failed)?;
    let publications = marker
        .parent()
        .ok_or(TrainError::Invalid("LoRA publication marker has no parent"))?;
    let _ = sync_directory(publications);
    Ok(())
}

fn rollback_publication(
    attempt: &Path,
    parent: &Path,
    output: &Path,
    output_sidecar: &Path,
    marker: &Path,
    error: TrainError,
) -> Result<(), TrainError> {
    let publication = fs::read(marker).map_err(failed).and_then(|contents| {
        serde_json::from_slice::<Publication>(&contents)
            .map_err(|parse| TrainError::Failed(parse.to_string()))
    });
    let restored = publication.and_then(|publication| {
        restore_snapshot(
            attempt,
            &attempt.join("previous.safetensors"),
            output,
            publication.previous_weight,
        )?;
        restore_snapshot(
            attempt,
            &attempt.join("previous.safetensors.zone.json"),
            output_sidecar,
            publication.previous_sidecar,
        )?;
        sync_directory(parent)?;
        fs::remove_file(marker).map_err(failed)?;
        let publications = marker
            .parent()
            .ok_or(TrainError::Invalid("LoRA publication marker has no parent"))?;
        let _ = sync_directory(publications);
        Ok(())
    });
    match restored {
        Ok(()) => Err(error),
        Err(restore) => Err(TrainError::Failed(format!(
            "{error}; adapter rollback failed: {restore}"
        ))),
    }
}

fn restore_snapshot(
    attempt: &Path,
    backup: &Path,
    output: &Path,
    existed: bool,
) -> Result<(), TrainError> {
    if existed {
        require_regular_file(attempt, backup)?;
        let backup_name = backup
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(TrainError::Invalid("LoRA backup has no filename"))?;
        let restore = attempt.join(format!("restore-{backup_name}"));
        match fs::symlink_metadata(&restore) {
            Ok(metadata) if metadata.file_type().is_symlink() || metadata.is_file() => {
                fs::remove_file(&restore).map_err(failed)?;
            }
            Ok(_) => {
                return Err(TrainError::Invalid(
                    "LoRA restore path is not a regular file",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(failed(error)),
        }
        fs::hard_link(backup, &restore).map_err(failed)?;
        sync_file(&restore)?;
        sync_directory(attempt)?;
        fs::rename(restore, output).map_err(failed)
    } else {
        match fs::remove_file(output) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(failed(error)),
        }
    }
}

fn require_regular_output(parent: &Path, path: &Path) -> Result<(), TrainError> {
    validate_output(parent, path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
            TrainError::Invalid("LoRA publication target is not a regular file"),
        ),
        Ok(_) => Ok(()),
        Err(error) => Err(failed(error)),
    }
}

fn read_generation(path: &Path) -> Option<String> {
    let contents = fs::read(path).ok()?;
    serde_json::from_slice::<WeightDocument>(&contents)
        .ok()?
        .generation
}

fn sync_file(path: &Path) -> Result<(), TrainError> {
    fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(failed)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), TrainError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(failed)
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), TrainError> {
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

    /// A real image whose colour follows its name, so a crop can still be
    /// traced back to the upload it was made from.
    fn image(target: &str, caption: &str, reference: Option<&str>) -> TrainImage {
        TrainImage {
            filename: format!("{target}.png"),
            caption: caption.to_string(),
            bytes_base64: encoded(colour(target)),
            before_base64: reference.map(|value| encoded(colour(value))),
            group: None,
        }
    }

    /// ComfyUI echoes back where it put the file, and the uploader checks that
    /// what came back is what it sent.
    struct Stage;

    impl wiremock::Respond for Stage {
        fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
            let body = String::from_utf8_lossy(&request.body);
            let field = |name: &str| {
                body.split(&format!("name=\"{name}\""))
                    .nth(1)
                    .and_then(|rest| rest.split("\r\n\r\n").nth(1))
                    .and_then(|rest| rest.split("\r\n").next())
                    .unwrap_or_default()
                    .to_string()
            };
            let filename = body
                .split("filename=\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .unwrap_or_default()
                .to_string();
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": filename,
                "subfolder": field("subfolder"),
                "type": field("type"),
            }))
        }
    }

    /// ComfyUI names the artifact it is serving, and the download checks that
    /// the file it gets back is the one it asked for.
    struct ServeArtifact(Vec<u8>);

    impl wiremock::Respond for ServeArtifact {
        fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
            let filename = request
                .url
                .query_pairs()
                .find(|(key, _)| key == "filename")
                .map(|(_, value)| value.into_owned())
                .unwrap_or_default();
            ResponseTemplate::new(200)
                .insert_header(
                    "content-disposition",
                    format!("filename=\"{filename}\"").as_str(),
                )
                .insert_header("content-type", "application/octet-stream")
                .set_body_bytes(self.0.clone())
        }
    }

    fn colour(name: &str) -> [u8; 3] {
        let hash = name.bytes().fold(17u32, |hash, byte| {
            hash.wrapping_mul(31).wrapping_add(u32::from(byte))
        });
        [hash as u8, (hash >> 8) as u8, (hash >> 16) as u8]
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

    fn staged_adapter(
        config: &Config,
        generation: &str,
        contents: &[u8],
    ) -> (PathBuf, PathBuf, PathBuf) {
        let attempt = config.models_dir.join("training").join(generation);
        fs::create_dir_all(&attempt).unwrap();
        let staged = attempt.join(format!("{generation}.safetensors"));
        let sidecar = sidecar_path(&staged);
        fs::write(&staged, contents).unwrap();
        fs::write(
            &sidecar,
            serde_json::to_vec(&WeightDocument {
                sidecar: WeightSidecar {
                    recipe_id: "flux-schnell-adapter".into(),
                    hf_base: Some("black-forest-labs/FLUX.1-schnell".into()),
                },
                generation: Some(generation.to_string()),
            })
            .unwrap(),
        )
        .unwrap();
        (attempt, staged, sidecar)
    }

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

    #[tokio::test]
    async fn external_trainer_keeps_the_documented_name_inside_an_isolated_attempt() {
        let command = r#"
            test "$ZONE_TRAIN_NAME" = "legacy-style.safetensors" || exit 11
            test "$ZONE_TRAIN_FINAL_NAME" = "$ZONE_TRAIN_NAME" || exit 12
            test "$ZONE_TRAIN_ATTEMPT" = "$(basename "$ZONE_TRAIN_DIR")" || exit 13
            test "$(basename "$ZONE_TRAIN_OUTPUT")" = "$ZONE_TRAIN_ATTEMPT.safetensors" || exit 14
            case "$ZONE_TRAIN_OUTPUT" in "$ZONE_TRAIN_DIR"/*) ;; *) exit 15 ;; esac
            printf legacy > "$ZONE_TRAIN_OUTPUT"
        "#;
        let (_root, config) = harness(command);

        let outcome = train_with_screening(
            &config,
            String::new(),
            String::new(),
            identity("legacy-style"),
            keep_all,
        )
        .await
        .expect("compatible external training command");

        assert_eq!(fs::read(outcome.path).unwrap(), b"legacy");
        assert!(training_entries(&config).is_empty());
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
        let training = root.join("training");
        assert!(
            !training.exists() || fs::read_dir(&training).unwrap().count() == 0,
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
        let edit_base = catalog
            .image_recipes()
            .find(|recipe| {
                !recipe.adapter && recipe.prompt_mode == crate::recipe::PromptMode::EditInstruction
            })
            .expect("the packaged catalog no longer ships an edit base")
            .id
            .clone();
        let mut pair = request("my-edit", &edit_base, Some("ohwx"));
        pair.images[0].before_base64 = Some(encoded_at(8, 8));
        train(&config, String::new(), String::new(), pair)
            .await
            .expect("an edit base trains on before and after together");
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
            .expect("with no caption model, the trigger alone still has to reach the dataset");
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
            named.path.file_name().unwrap(),
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
        let server = MockServer::start().await;
        let prompt = uuid::Uuid::new_v4();
        Mock::given(method("POST"))
            .and(path("/upload/image"))
            .respond_with(Stage)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"prompt_id": prompt, "number": 1})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/history/{prompt}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                prompt.to_string(): {"status": {"completed": true, "status_str": "success"}}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/view"))
            .respond_with(ServeArtifact(vec![9u8; 20_000]))
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
        let outcome = train(
            &config,
            String::new(),
            String::new(),
            request("graph-style", "flux-schnell", Some("ohwx")),
        )
        .await
        .unwrap();
        assert_eq!(fs::read(&outcome.path).unwrap(), vec![9u8; 20_000]);
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|request| request.url.path() == "/upload/image"),
            "the dataset has to reach the graph before it is queued"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn caption_prefixes_trigger_for_identity() {
        assert_eq!(identity_caption("a portrait", "ohwx"), "ohwx, a portrait");
        assert_eq!(
            identity_caption("ohwx, a portrait", "ohwx"),
            "ohwx, a portrait"
        );
        assert_eq!(
            identity_caption("a fox located by a tree", "cat"),
            "cat, a fox located by a tree"
        );
        assert_eq!(
            identity_caption("A CAT by a tree", "cat"),
            "A CAT by a tree"
        );
        assert_eq!(
            identity_caption("a BLUE CAT by a tree", "blue cat"),
            "a BLUE CAT by a tree"
        );
        assert_eq!(
            identity_caption("a blue catapult", "blue cat"),
            "blue cat, a blue catapult"
        );
        assert_eq!(identity_caption("un CAFÉ", "café"), "un CAFÉ");
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
            test "$ZONE_TRAIN_NAME" = "edit-style.safetensors" || exit 31
            test "$ZONE_TRAIN_FINAL_NAME" = "edit-style.safetensors" || exit 12
            test "$ZONE_TRAIN_ATTEMPT" = "$(basename "$ZONE_TRAIN_DIR")" || exit 14
            test "$(basename "$ZONE_TRAIN_OUTPUT")" = "$ZONE_TRAIN_ATTEMPT.safetensors" || exit 13
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
            test "$(cat "$ZONE_TRAIN_DIR/targets/0000.txt")" = "change zero" || exit 17
            test "$(cat "$ZONE_TRAIN_DIR/targets/0001.txt")" = "change two" || exit 20
            test ! -e "$ZONE_TRAIN_DIR/targets/0002.png" || exit 21
            test ! -e "$ZONE_TRAIN_DIR/control_1/0002.png" || exit 22
            cp "$ZONE_TRAIN_DIR/targets/0000.png" "KEPT/target-0.png" || exit 15
            cp "$ZONE_TRAIN_DIR/control_1/0000.png" "KEPT/control-0.png" || exit 16
            cp "$ZONE_TRAIN_DIR/targets/0001.png" "KEPT/target-1.png" || exit 18
            cp "$ZONE_TRAIN_DIR/control_1/0001.png" "KEPT/control-1.png" || exit 19
            printf trained > "$ZONE_TRAIN_OUTPUT"
        "#;
        // The attempt is cleaned up when the run ends, so the images have to be
        // copied out before they can be compared against the crops they should be.
        let kept = tempfile::tempdir().expect("kept dataset");
        let command = command.replace("KEPT", &kept.path().display().to_string());
        let (_root, mut config) = harness(&command);
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

        let side = crate::train::packaged_config().unwrap().resolution();
        let subject = Subject::shared(&config);
        for (index, name) in [(0, "zero"), (1, "two")] {
            let source = image(
                &format!("target-{name}"),
                "",
                Some(&format!("reference-{name}")),
            );
            let framed = frame(&subject, &source, side).expect("the fixture frames");
            assert_eq!(
                fs::read(kept.path().join(format!("target-{index}.png"))).unwrap(),
                framed.target,
                "targets/{index:04} must hold the {name} pair's target"
            );
            assert_eq!(
                fs::read(kept.path().join(format!("control-{index}.png"))).unwrap(),
                framed.control.expect("the fixture carries a reference"),
                "control_1/{index:04} must hold the {name} pair's reference"
            );
        }
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
        paired.images[0].before_base64 = Some(encoded(colour("reference")));
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

    #[test]
    fn concurrent_same_name_publications_cannot_interleave_weight_and_sidecar() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (_root, config) = harness("unused");
        let loras = config.models_dir.join("loras");
        fs::create_dir(&loras).unwrap();
        let output = loras.join("shared.safetensors");
        let output_sidecar = sidecar_path(&output);
        let first_generation = Uuid::new_v4().to_string();
        let second_generation = Uuid::new_v4().to_string();
        let (first_attempt, first_weight, first_sidecar) =
            staged_adapter(&config, &first_generation, b"first");
        let (second_attempt, second_weight, second_sidecar) =
            staged_adapter(&config, &second_generation, b"second");
        let (weights_tx, weights_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first_output = output.clone();
        let first_output_sidecar = output_sidecar.clone();
        let first_loras = loras.clone();
        let first = std::thread::spawn(move || {
            promote_with(
                &first_attempt,
                &first_weight,
                &first_sidecar,
                &first_loras,
                &first_output,
                &first_output_sidecar,
                &first_generation,
                |phase| {
                    if phase == PublicationPhase::Weights {
                        weights_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                    }
                    Ok(())
                },
            )
        });
        weights_rx.recv().unwrap();

        let (done_tx, done_rx) = mpsc::channel();
        let second_output = output.clone();
        let second_output_sidecar = output_sidecar.clone();
        let second_loras = loras.clone();
        let expected_generation = second_generation.clone();
        let second = std::thread::spawn(move || {
            let result = atomic_promote(
                &second_attempt,
                &second_weight,
                &second_sidecar,
                &second_loras,
                &second_output,
                &second_output_sidecar,
                &second_generation,
            );
            done_tx.send(()).unwrap();
            result
        });
        assert!(
            done_rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "the second publication must wait at the same final name"
        );
        assert!(
            crate::inventory::scan(&config.models_dir, &RecipeCatalog::packaged().unwrap())
                .is_empty(),
            "readers must not observe the first weight before its sidecar"
        );

        release_tx.send(()).unwrap();
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
        assert_eq!(fs::read(output).unwrap(), b"second");
        assert_eq!(
            read_generation(&output_sidecar).as_deref(),
            Some(expected_generation.as_str())
        );
    }

    #[test]
    fn interrupted_publication_is_hidden_and_recovers_the_previous_generation() {
        let (_root, config) = harness("unused");
        let loras = config.models_dir.join("loras");
        fs::create_dir(&loras).unwrap();
        let output = loras.join("stable.safetensors");
        let output_sidecar = sidecar_path(&output);
        let previous_generation = Uuid::new_v4().to_string();
        fs::write(&output, b"previous").unwrap();
        fs::write(
            &output_sidecar,
            serde_json::to_vec(&WeightDocument {
                sidecar: WeightSidecar {
                    recipe_id: "flux-schnell-adapter".into(),
                    hf_base: Some("black-forest-labs/FLUX.1-schnell".into()),
                },
                generation: Some(previous_generation.clone()),
            })
            .unwrap(),
        )
        .unwrap();
        let generation = Uuid::new_v4().to_string();
        let (attempt, staged, staged_sidecar) = staged_adapter(&config, &generation, b"new");
        let interrupted = std::panic::catch_unwind(|| {
            promote_with(
                &attempt,
                &staged,
                &staged_sidecar,
                &loras,
                &output,
                &output_sidecar,
                &generation,
                |phase| {
                    if phase == PublicationPhase::Weights {
                        panic!("simulated process interruption");
                    }
                    Ok(())
                },
            )
            .unwrap();
        });
        assert!(interrupted.is_err());
        let marker = publication_marker(&loras, "stable.safetensors").unwrap();
        assert!(marker.is_file());
        assert!(
            crate::inventory::scan(&config.models_dir, &RecipeCatalog::packaged().unwrap())
                .is_empty(),
            "an interrupted generation must fail closed"
        );

        restore_snapshot(
            &attempt,
            &attempt.join("previous.safetensors"),
            &output,
            true,
        )
        .unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"previous");
        recover_publication(&loras, &output, &output_sidecar, &marker).unwrap();
        assert_eq!(fs::read(output).unwrap(), b"previous");
        assert_eq!(
            read_generation(&output_sidecar).as_deref(),
            Some(previous_generation.as_str())
        );
        assert!(!marker.exists());
    }

    #[test]
    fn publication_error_restores_both_files_before_returning() {
        let (_root, config) = harness("unused");
        let loras = config.models_dir.join("loras");
        fs::create_dir(&loras).unwrap();
        let output = loras.join("stable.safetensors");
        let output_sidecar = sidecar_path(&output);
        let previous_generation = Uuid::new_v4().to_string();
        fs::write(&output, b"previous").unwrap();
        fs::write(
            &output_sidecar,
            serde_json::to_vec(&WeightDocument {
                sidecar: WeightSidecar {
                    recipe_id: "flux-schnell-adapter".into(),
                    hf_base: Some("black-forest-labs/FLUX.1-schnell".into()),
                },
                generation: Some(previous_generation.clone()),
            })
            .unwrap(),
        )
        .unwrap();
        let generation = Uuid::new_v4().to_string();
        let (attempt, staged, staged_sidecar) = staged_adapter(&config, &generation, b"new");

        let error = promote_with(
            &attempt,
            &staged,
            &staged_sidecar,
            &loras,
            &output,
            &output_sidecar,
            &generation,
            |phase| match phase {
                PublicationPhase::Weights => {
                    Err(TrainError::Failed("injected publication error".into()))
                }
                PublicationPhase::Sidecar => Ok(()),
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("injected publication error"));
        assert_eq!(fs::read(output).unwrap(), b"previous");
        assert_eq!(
            read_generation(&output_sidecar).as_deref(),
            Some(previous_generation.as_str())
        );
        assert!(
            !publication_marker(&loras, "stable.safetensors")
                .unwrap()
                .exists()
        );
    }

    #[test]
    fn mismatched_staged_sidecar_never_reaches_the_final_name() {
        let (_root, config) = harness("unused");
        let loras = config.models_dir.join("loras");
        fs::create_dir(&loras).unwrap();
        let generation = Uuid::new_v4().to_string();
        let wrong_generation = Uuid::new_v4().to_string();
        let (attempt, staged, sidecar) = staged_adapter(&config, &generation, b"new");
        fs::write(
            &sidecar,
            serde_json::to_vec(&WeightDocument {
                sidecar: WeightSidecar {
                    recipe_id: "flux-schnell-adapter".into(),
                    hf_base: Some("black-forest-labs/FLUX.1-schnell".into()),
                },
                generation: Some(wrong_generation),
            })
            .unwrap(),
        )
        .unwrap();
        let output = loras.join("stable.safetensors");
        let output_sidecar = sidecar_path(&output);

        let error = atomic_promote(
            &attempt,
            &staged,
            &sidecar,
            &loras,
            &output,
            &output_sidecar,
            &generation,
        )
        .unwrap_err();

        assert!(matches!(error, TrainError::Invalid(_)));
        assert!(!output.exists());
        assert!(!output_sidecar.exists());
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

    #[tokio::test]
    async fn invalid_overlay_catalog_never_falls_back_to_packaged_training() {
        let (root, mut config) = harness("printf trained > \"$ZONE_TRAIN_OUTPUT\"");
        let workflows = root.path().join("workflows");
        let recipes = root.path().join("recipes");
        fs::create_dir(&workflows).unwrap();
        fs::create_dir(&recipes).unwrap();
        let mut catalog: Value =
            serde_json::from_str(include_str!("../../../comfyui/recipes/catalog.json")).unwrap();
        let flux = catalog["recipes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|recipe| recipe["id"] == "flux-schnell")
            .unwrap();
        flux["training"].as_object_mut().unwrap().remove("adapter");
        fs::write(
            recipes.join("catalog.json"),
            serde_json::to_vec(&catalog).unwrap(),
        )
        .unwrap();
        config.workflow_path = workflows.join("flux1-schnell-fp8-api.json");

        let error = train_with_screening(
            &config,
            String::new(),
            String::new(),
            identity("must-not-train"),
            keep_all,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, TrainError::Invalid(_)));
        assert!(training_entries(&config).is_empty());
        assert!(
            !config
                .models_dir
                .join("loras/must-not-train.safetensors")
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
            identity_caption("zrk pattern knitwear", "zrkx"),
            "zrkx, zrk pattern knitwear"
        );
        assert_eq!(
            identity_caption("zrkxyz, a portrait", "zrkxyz"),
            "zrkxyz, a portrait",
            "a caption that already names the trigger keeps exactly one"
        );
        assert_eq!(
            identity_caption("zrkxyzed hair", "zrkxyz"),
            "zrkxyz, zrkxyzed hair",
            "a longer word that merely starts with the trigger is not the trigger"
        );
        // A trigger is not always one word. Splitting the caption into words
        // could never match this one, and would prefix it a second time.
        assert_eq!(
            identity_caption("my-style, a portrait", "my-style"),
            "my-style, a portrait",
            "a hyphenated trigger the caption already names is not repeated"
        );
        assert_eq!(
            identity_caption("a portrait", "my-style"),
            "my-style, a portrait"
        );
    }

    #[test]
    fn a_failed_write_names_the_artifact_it_was_for() {
        let attempt = tempfile::tempdir().unwrap();
        let missing = attempt.path().join("targets").join("0000.png");
        let Err(TrainError::Failed(message)) = write_new(attempt.path(), &missing, b"x") else {
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
