//! Scan the ComfyUI models directory and join files to packaged recipes.

use crate::config::Config;
use crate::recipe::{MediaKind, Recipe, RecipeCatalog, RequiredFile, sanitize_weight_filename};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const SIDECAR_SUFFIX: &str = ".zone.json";
pub(crate) const PUBLICATION_DIRECTORY: &str = ".zone-publish";
const PUBLICATION_SUFFIX: &str = ".pending";

const SCAN_DIRECTORIES: &[(&str, &str)] = &[
    ("checkpoints", "checkpoint"),
    ("diffusion_models", "diffusion_model"),
    ("loras", "lora"),
];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WeightSidecar {
    pub recipe_id: String,
    #[serde(default)]
    pub hf_base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
}

pub const FACE_SUFFIX: &str = ".face.png";
pub const EMBEDDINGS_DIRECTORY: &str = "embeddings";

pub fn face_filename(weight: &str) -> Option<String> {
    let stem = Path::new(weight).file_stem()?.to_str()?;
    if stem.is_empty() {
        return None;
    }
    Some(format!("{stem}{FACE_SUFFIX}"))
}

/// A ready identity adapter whose trigger word can select it at generate time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub filename: String,
    pub trigger: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct WeightDocument {
    #[serde(flatten)]
    pub sidecar: WeightSidecar,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InventoryItem {
    pub filename: String,
    pub recipe_id: String,
    pub kind: String,
    pub label: String,
    pub directory: String,
    pub size: u64,
    pub modified_at: Option<String>,
    pub ready: bool,
    pub required_files: Vec<String>,
    pub adapter: bool,
    pub prompt_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
}

pub fn scan(models_dir: &Path, catalog: &RecipeCatalog) -> Vec<InventoryItem> {
    let Some(models_dir) = models_root(models_dir) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for (directory, kind) in SCAN_DIRECTORIES {
        let folder = models_dir.join(directory);
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if filename.ends_with(SIDECAR_SUFFIX) || !filename.ends_with(".safetensors") {
                continue;
            }
            if sanitize_weight_filename(filename).is_err() {
                continue;
            }
            if *kind == "lora" && publication_pending(&models_dir, filename) {
                continue;
            }
            let metadata = fs::metadata(&path).ok();
            let size = metadata.as_ref().map(|meta| meta.len()).unwrap_or(0);
            let modified_at = metadata
                .and_then(|meta| meta.modified().ok())
                .and_then(rfc3339);
            let sidecar = read_sidecar(&path);
            let trigger = sidecar.as_ref().and_then(document_trigger);
            let recipe = resolve_recipe(catalog, filename, kind, sidecar.as_ref());
            let Some(recipe) = recipe else {
                continue;
            };
            if *kind == "lora" && publication_pending(&models_dir, filename) {
                continue;
            }
            let missing = missing_required(&models_dir, &recipe.required_files);
            let mut required: Vec<String> = recipe
                .required_files
                .iter()
                .map(|file| file.filename.clone())
                .collect();
            if *kind == "lora" {
                required.insert(0, filename.to_string());
            }
            items.push(InventoryItem {
                filename: filename.to_string(),
                recipe_id: recipe.id.clone(),
                kind: kind.to_string(),
                label: inventory_label(recipe, filename),
                directory: directory.to_string(),
                size,
                modified_at,
                ready: missing.is_empty(),
                required_files: if missing.is_empty() {
                    required
                } else {
                    missing
                },
                adapter: recipe.adapter,
                prompt_mode: match recipe.prompt_mode {
                    crate::recipe::PromptMode::ClipScene => "clip_scene".to_string(),
                    crate::recipe::PromptMode::EditInstruction => "edit_instruction".to_string(),
                },
                trigger,
            });
        }
    }
    items.sort_by(|left, right| left.filename.cmp(&right.filename));
    items
}

pub fn find<'a>(items: &'a [InventoryItem], filename: &str) -> Option<&'a InventoryItem> {
    items.iter().find(|item| item.filename == filename)
}

/// Ready identity adapters on disk, in filename order.
pub fn identities(models_dir: &Path, workflow_path: &Path) -> Vec<Identity> {
    let Some(catalog) = load_catalog(workflow_path) else {
        return Vec::new();
    };
    identities_among(&scan(models_dir, &catalog))
}

/// If `haystack` names exactly one ready identity, pin that adapter as the
/// image checkpoint so generate/edit load it.
pub fn bind_identity(config: &mut Config, haystack: &str) -> Option<Identity> {
    let catalog = load_catalog(&config.workflow_path)?;
    let identity = identity_for_prompt(&config.models_dir, &catalog, haystack)?;
    config.checkpoint = identity.filename.clone();
    Some(identity)
}

pub fn identity_for_prompt(
    models_dir: &Path,
    catalog: &RecipeCatalog,
    haystack: &str,
) -> Option<Identity> {
    identity_among(&scan(models_dir, catalog), haystack)
}

fn load_catalog(workflow_path: &Path) -> Option<RecipeCatalog> {
    RecipeCatalog::load(Some(workflow_path))
        .or_else(|_| RecipeCatalog::packaged())
        .ok()
}

fn is_identity(item: &InventoryItem) -> bool {
    item.ready
        && item.recipe_id != "wan-adapter"
        && (item.adapter || item.kind == "checkpoint")
        && item
            .trigger
            .as_deref()
            .is_some_and(|trigger| !trigger.trim().is_empty())
}

fn identities_among(items: &[InventoryItem]) -> Vec<Identity> {
    items
        .iter()
        .filter(|item| is_identity(item))
        .filter_map(|item| {
            let trigger = item.trigger.as_deref()?.trim();
            (!trigger.is_empty()).then(|| Identity {
                filename: item.filename.clone(),
                trigger: trigger.to_string(),
            })
        })
        .collect()
}

fn identity_among(items: &[InventoryItem], haystack: &str) -> Option<Identity> {
    let mut matches: Vec<&InventoryItem> = items
        .iter()
        .filter(|item| {
            is_identity(item)
                && item.trigger.as_deref().is_some_and(|trigger| {
                    let trigger = trigger.trim();
                    !trigger.is_empty() && contains_phrase(haystack, trigger)
                })
        })
        .collect();
    if matches.is_empty() {
        return None;
    }
    let longest = matches
        .iter()
        .filter_map(|item| item.trigger.as_deref())
        .map(|trigger| trigger.trim().chars().count())
        .max()
        .unwrap_or(0);
    matches.retain(|item| {
        item.trigger
            .as_deref()
            .is_some_and(|trigger| trigger.trim().chars().count() == longest)
    });
    let item = (matches.len() == 1).then_some(matches[0])?;
    Some(Identity {
        filename: item.filename.clone(),
        trigger: item
            .trigger
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_string(),
    })
}

/// Trigger matching is Unicode-lowercase and requires a boundary around the
/// complete phrase. It never treats a trigger as a substring of another token.
pub(crate) fn contains_phrase(text: &str, phrase: &str) -> bool {
    let text = text.to_lowercase();
    let phrase = phrase.to_lowercase();
    if phrase.is_empty() {
        return false;
    }
    text.match_indices(&phrase).any(|(start, matched)| {
        let before = text[..start].chars().next_back();
        let after = text[start + matched.len()..].chars().next();
        !before.is_some_and(is_trigger_character) && !after.is_some_and(is_trigger_character)
    })
}

fn is_trigger_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn document_trigger(document: &WeightDocument) -> Option<String> {
    document
        .sidecar
        .trigger
        .as_deref()
        .map(str::trim)
        .filter(|trigger| !trigger.is_empty())
        .map(str::to_string)
}

pub fn write_sidecar(path: &Path, sidecar: &WeightSidecar) -> std::io::Result<()> {
    let encoded = serde_json::to_vec_pretty(sidecar)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    fs::write(sidecar_path(path), encoded)
}

pub fn relative_weight_path(kind: &str, filename: &str) -> Option<PathBuf> {
    let directory = match kind {
        "checkpoint" => "checkpoints",
        "diffusion_model" => "diffusion_models",
        "lora" => "loras",
        "vae" => "vae",
        "text_encoder" => "text_encoders",
        _ => return None,
    };
    Some(PathBuf::from(directory).join(filename))
}

fn resolve_recipe<'a>(
    catalog: &'a RecipeCatalog,
    filename: &str,
    kind: &str,
    document: Option<&WeightDocument>,
) -> Option<&'a Recipe> {
    if kind == "lora" {
        let document = document?;
        if document
            .generation
            .as_deref()
            .is_some_and(|generation| uuid::Uuid::parse_str(generation).is_err())
        {
            return None;
        }
        let sidecar = &document.sidecar;
        let recipe = catalog.get(&sidecar.recipe_id)?;
        let hf_base = sidecar.hf_base.as_deref()?;
        return (matches!(recipe.kind, MediaKind::Image | MediaKind::Video)
            && recipe.adapter
            && recipe.has_lora_slot()
            && recipe
                .hf_bases
                .iter()
                .any(|base| base.eq_ignore_ascii_case(hf_base)))
        .then_some(recipe);
    }
    if let Some(document) = document
        && let Some(recipe) = catalog.get(&document.sidecar.recipe_id)
    {
        return Some(recipe);
    }
    catalog.image_recipe_for(filename).ok()
}

pub(crate) fn publication_marker(loras: &Path, filename: &str) -> Option<PathBuf> {
    let filename = sanitize_weight_filename(filename).ok()?;
    Some(
        loras
            .join(PUBLICATION_DIRECTORY)
            .join(format!("{filename}{PUBLICATION_SUFFIX}")),
    )
}

fn publication_pending(models_dir: &Path, filename: &str) -> bool {
    publication_marker(&models_dir.join("loras"), filename)
        .is_some_and(|path| fs::symlink_metadata(path).is_ok())
}

fn missing_required(models_dir: &Path, required: &[RequiredFile]) -> Vec<String> {
    required
        .iter()
        .filter(|file| !required_path(models_dir, file).is_some_and(|path| path.is_file()))
        .map(|file| file.filename.clone())
        .collect()
}

pub(crate) fn files_present(models_dir: &Path, required: &[RequiredFile]) -> bool {
    missing_required(models_dir, required).is_empty()
}

/// The models root the operator configured, resolved to the real directory it
/// names. A setting that points at anything else has nothing to scan.
fn models_root(models_dir: &Path) -> Option<PathBuf> {
    let root = fs::canonicalize(models_dir).ok()?;
    root.is_dir().then_some(root)
}

/// One segment of a catalog-supplied path. A segment that could steer the join
/// out of the models root is not a segment.
fn path_component(value: &str) -> Option<String> {
    let usable = !value.is_empty()
        && value.len() <= 256
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains("..");
    usable.then(|| value.to_string())
}

fn required_path(models_dir: &Path, file: &RequiredFile) -> Option<PathBuf> {
    let directory = path_component(&file.directory)?;
    let filename = path_component(&file.filename)?;
    Some(models_dir.join(directory).join(filename))
}

fn inventory_label(recipe: &Recipe, filename: &str) -> String {
    if recipe.adapter {
        format!("{} · {filename}", recipe.label)
    } else {
        recipe.label.clone()
    }
}

fn sidecar_path(weight: &Path) -> PathBuf {
    let mut name = weight
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("weight")
        .to_string();
    name.push_str(SIDECAR_SUFFIX);
    weight.with_file_name(name)
}

fn read_sidecar(weight: &Path) -> Option<WeightDocument> {
    let sidecar = sidecar_path(weight);
    let metadata = fs::symlink_metadata(&sidecar).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return None;
    }
    let contents = fs::read_to_string(sidecar).ok()?;
    serde_json::from_str(&contents).ok()
}

fn rfc3339(time: SystemTime) -> Option<String> {
    let datetime = chrono::DateTime::<chrono::Utc>::from(time);
    Some(datetime.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::RecipeCatalog;
    use std::fs;

    fn temp_models() -> PathBuf {
        let root = std::env::temp_dir().join(format!("zone-comfy-inv-{}", uuid::Uuid::new_v4()));
        for directory in [
            "checkpoints",
            "diffusion_models",
            "loras",
            "text_encoders",
            "vae",
        ] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        root
    }

    fn write_qwen_sidecar(weight: &Path) {
        write_sidecar(
            weight,
            &WeightSidecar {
                recipe_id: "qwen-image-edit-adapter".into(),
                hf_base: Some("Qwen/Qwen-Image-Edit-2511".into()),
                trigger: None,
                ..Default::default()
            },
        )
        .unwrap();
    }

    #[test]
    fn adapter_is_not_ready_without_base() {
        let root = temp_models();
        let lora = root.join("loras/qwen-image-edit-plus-nsfw-lora.safetensors");
        fs::write(&lora, b"lora").unwrap();
        write_qwen_sidecar(&lora);
        let catalog = RecipeCatalog::packaged().unwrap();
        let items = scan(&root, &catalog);
        let item = items
            .iter()
            .find(|item| item.filename == "qwen-image-edit-plus-nsfw-lora.safetensors")
            .unwrap();
        assert_eq!(item.recipe_id, "qwen-image-edit-adapter");
        assert!(!item.ready);
        assert!(
            item.required_files
                .iter()
                .any(|name| name.contains("qwen_image_edit"))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn adapter_is_ready_when_required_bundle_exists() {
        let root = temp_models();
        let lora = root.join("loras/qwen-image-edit-plus-nsfw-lora.safetensors");
        fs::write(&lora, b"lora").unwrap();
        write_qwen_sidecar(&lora);
        fs::write(
            root.join("diffusion_models/qwen_image_edit_2511_fp8mixed.safetensors"),
            b"unet",
        )
        .unwrap();
        fs::write(
            root.join("text_encoders/qwen_2.5_vl_7b_fp8_scaled.safetensors"),
            b"clip",
        )
        .unwrap();
        fs::write(root.join("vae/qwen_image_vae.safetensors"), b"vae").unwrap();
        let catalog = RecipeCatalog::packaged().unwrap();
        let items = scan(&root, &catalog);
        let item = items
            .iter()
            .find(|item| item.filename == "qwen-image-edit-plus-nsfw-lora.safetensors")
            .unwrap();
        assert!(item.ready);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn coherent_sidecar_selects_adapter_without_filename_inference() {
        let root = temp_models();
        let lora = root.join("loras/custom-style.safetensors");
        fs::write(&lora, b"lora").unwrap();
        write_sidecar(
            &lora,
            &WeightSidecar {
                recipe_id: "qwen-image-edit-adapter".into(),
                hf_base: Some("Qwen/Qwen-Image-Edit-2511".into()),
                trigger: None,
                ..Default::default()
            },
        )
        .unwrap();
        let catalog = RecipeCatalog::packaged().unwrap();
        let items = scan(&root, &catalog);
        assert_eq!(items[0].recipe_id, "qwen-image-edit-adapter");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_or_mismatched_adapter_sidecars_are_not_listed() {
        let root = temp_models();
        let missing = root.join("loras/missing.safetensors");
        let unbound = root.join("loras/unbound.safetensors");
        let mismatched = root.join("loras/mismatched.safetensors");
        let unknown = root.join("loras/unknown.safetensors");
        let malformed = root.join("loras/malformed.safetensors");
        for weight in [&missing, &unbound, &mismatched, &unknown, &malformed] {
            fs::write(weight, b"lora").unwrap();
        }
        write_sidecar(
            &unbound,
            &WeightSidecar {
                recipe_id: "flux-schnell-adapter".into(),
                hf_base: None,
                trigger: None,
                ..Default::default()
            },
        )
        .unwrap();
        write_sidecar(
            &mismatched,
            &WeightSidecar {
                recipe_id: "flux-schnell-adapter".into(),
                hf_base: Some("Qwen/Qwen-Image-Edit-2511".into()),
                trigger: None,
                ..Default::default()
            },
        )
        .unwrap();
        write_sidecar(
            &unknown,
            &WeightSidecar {
                recipe_id: "missing-adapter".into(),
                hf_base: Some("Qwen/Qwen-Image-Edit-2511".into()),
                trigger: None,
                ..Default::default()
            },
        )
        .unwrap();
        fs::write(
            sidecar_path(&malformed),
            serde_json::to_vec(&WeightDocument {
                sidecar: WeightSidecar {
                    recipe_id: "flux-schnell-adapter".into(),
                    hf_base: Some("black-forest-labs/FLUX.1-schnell".into()),
                    trigger: None,
                    ..Default::default()
                },
                generation: Some("not-a-generation".into()),
            })
            .unwrap(),
        )
        .unwrap();

        assert!(scan(&root, &RecipeCatalog::packaged().unwrap()).is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_required_file_reached_through_a_traversal_is_reported_missing() {
        let root = temp_models();
        fs::write(root.join("vae/base.safetensors"), b"vae").unwrap();

        let named = RequiredFile {
            filename: "base.safetensors".into(),
            directory: "vae".into(),
        };
        assert!(
            missing_required(&root, std::slice::from_ref(&named)).is_empty(),
            "the file sits exactly where the catalog names it"
        );

        let traversed = RequiredFile {
            filename: "base.safetensors".into(),
            directory: "loras/../vae".into(),
        };
        assert_eq!(
            missing_required(&root, std::slice::from_ref(&traversed)),
            vec!["base.safetensors".to_string()],
            "a required file may only be named as a segment of the models root"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_models_root_that_is_not_a_directory_has_nothing_to_scan() {
        let root = temp_models();
        let file = root.join("loras/not-a-root");
        fs::write(&file, b"weight").unwrap();

        assert_eq!(models_root(&root), Some(fs::canonicalize(&root).unwrap()));
        assert_eq!(models_root(&file), None, "a file is not a models root");
        assert_eq!(
            models_root(&root.join("absent")),
            None,
            "a root that is not there is not a models root"
        );
        assert!(scan(&file, &RecipeCatalog::packaged().unwrap()).is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pending_publication_is_not_listed() {
        let root = temp_models();
        let lora = root.join("loras/style.safetensors");
        fs::write(&lora, b"lora").unwrap();
        fs::write(
            sidecar_path(&lora),
            serde_json::to_vec(&WeightDocument {
                sidecar: WeightSidecar {
                    recipe_id: "flux-schnell-adapter".into(),
                    hf_base: Some("black-forest-labs/FLUX.1-schnell".into()),
                    trigger: None,
                    ..Default::default()
                },
                generation: Some(uuid::Uuid::new_v4().to_string()),
            })
            .unwrap(),
        )
        .unwrap();
        let marker = publication_marker(&root.join("loras"), "style.safetensors").unwrap();
        fs::create_dir_all(marker.parent().unwrap()).unwrap();
        fs::write(marker, b"pending").unwrap();

        assert!(scan(&root, &RecipeCatalog::packaged().unwrap()).is_empty());
        let _ = fs::remove_dir_all(root);
    }

    fn write_identity(root: &Path, filename: &str, trigger: &str) {
        let lora = root.join("loras").join(filename);
        fs::write(&lora, b"lora").unwrap();
        write_sidecar(
            &lora,
            &WeightSidecar {
                recipe_id: "flux-schnell-adapter".into(),
                hf_base: Some("black-forest-labs/FLUX.1-schnell".into()),
                trigger: Some(trigger.into()),
                ..Default::default()
            },
        )
        .unwrap();
        let checkpoint = root.join("checkpoints/flux1-schnell-fp8.safetensors");
        if !checkpoint.is_file() {
            fs::write(checkpoint, b"ckpt").unwrap();
        }
        let uncensored = root.join("loras/flux-uncensored.safetensors");
        if !uncensored.is_file() {
            fs::write(uncensored, b"uncensored").unwrap();
        }
    }

    #[test]
    fn a_sidecar_without_a_trigger_still_loads() {
        let sidecar: WeightSidecar = serde_json::from_str(
            r#"{"recipe_id":"flux-schnell-adapter","hf_base":"black-forest-labs/FLUX.1-schnell"}"#,
        )
        .unwrap();
        assert_eq!(sidecar.trigger, None);
        assert_eq!(sidecar.embedding, None);
        assert_eq!(sidecar.architecture, None);
        assert_eq!(sidecar.face, None);
    }

    #[test]
    fn a_person_sidecar_keeps_embedding_architecture_and_face() {
        let sidecar: WeightSidecar = serde_json::from_str(
            r#"{"recipe_id":"sdxl-adapter","hf_base":"John6666/lustify-sdxl-nsfw-checkpoint-ggwp-v7-sdxl","trigger":"ohwx","embedding":"ohwx.safetensors","architecture":"sdxl","face":"jerry.face.png"}"#,
        )
        .unwrap();
        assert_eq!(sidecar.embedding.as_deref(), Some("ohwx.safetensors"));
        assert_eq!(sidecar.architecture.as_deref(), Some("sdxl"));
        assert_eq!(sidecar.face.as_deref(), Some("jerry.face.png"));
        assert_eq!(
            face_filename("jerry.safetensors").as_deref(),
            Some("jerry.face.png")
        );
    }

    #[test]
    fn a_video_adapter_is_inventoried_but_not_an_image_identity() {
        let root = temp_models();
        let lora = root.join("loras/jerry-wan.safetensors");
        fs::write(&lora, b"lora").unwrap();
        write_sidecar(
            &lora,
            &WeightSidecar {
                recipe_id: "wan-adapter".into(),
                hf_base: Some("Comfy-Org/Wan_2.2_ComfyUI_Repackaged".into()),
                trigger: Some("ohwx".into()),
                architecture: Some("wan".into()),
                ..Default::default()
            },
        )
        .unwrap();
        for (directory, filename) in [
            ("diffusion_models", "wan2.2_ti2v_5B_fp16.safetensors"),
            ("text_encoders", "umt5_xxl_fp8_e4m3fn_scaled.safetensors"),
            ("vae", "wan2.2_vae.safetensors"),
        ] {
            fs::create_dir_all(root.join(directory)).unwrap();
            fs::write(root.join(directory).join(filename), b"wan").unwrap();
        }
        let catalog = RecipeCatalog::packaged().unwrap();
        let items = scan(&root, &catalog);
        let item = items
            .iter()
            .find(|item| item.filename == "jerry-wan.safetensors")
            .unwrap();
        assert_eq!(item.recipe_id, "wan-adapter");
        assert!(item.ready);
        assert!(item.adapter);
        assert!(identities_among(&items).is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn scan_exposes_the_trigger_written_on_a_ready_adapter() {
        let root = temp_models();
        write_identity(&root, "jake.safetensors", "ohwx");
        let items = scan(&root, &RecipeCatalog::packaged().unwrap());
        let item = items
            .iter()
            .find(|item| item.filename == "jake.safetensors")
            .unwrap();
        assert_eq!(item.trigger.as_deref(), Some("ohwx"));
        assert!(item.ready);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn identity_matching_picks_the_named_ready_adapter() {
        let root = temp_models();
        write_identity(&root, "jake.safetensors", "ohwx");
        write_identity(&root, "teapot.safetensors", "zrkxyz");
        let catalog = RecipeCatalog::packaged().unwrap();

        let matched = identity_for_prompt(&root, &catalog, "draw ohwx at the beach").unwrap();
        assert_eq!(matched.filename, "jake.safetensors");
        assert_eq!(matched.trigger, "ohwx");

        assert!(
            identity_for_prompt(&root, &catalog, "a lighthouse at dusk").is_none(),
            "a prompt that names no identity keeps the pin"
        );
        assert!(
            identity_for_prompt(&root, &catalog, "she asks for a portrait").is_none(),
            "a trigger must not match inside another word"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_longest_trigger_wins_and_a_tie_stays_on_the_pin() {
        let root = temp_models();
        write_identity(&root, "cat.safetensors", "cat");
        write_identity(&root, "blue-cat.safetensors", "blue cat");
        write_identity(&root, "other.safetensors", "ohwx");
        let catalog = RecipeCatalog::packaged().unwrap();

        let matched = identity_for_prompt(&root, &catalog, "a blue cat on a sofa").unwrap();
        assert_eq!(matched.filename, "blue-cat.safetensors");

        write_identity(&root, "twin.safetensors", "ohwx");
        assert!(
            identity_for_prompt(&root, &catalog, "ohwx waving").is_none(),
            "two identities that share a trigger are ambiguous"
        );
        let _ = fs::remove_dir_all(root);
    }

    fn write_person_checkpoint(root: &Path, filename: &str, trigger: Option<&str>) {
        let checkpoint = root.join("checkpoints").join(filename);
        fs::write(&checkpoint, b"ckpt").unwrap();
        write_sidecar(
            &checkpoint,
            &WeightSidecar {
                recipe_id: "sdxl".into(),
                hf_base: Some("John6666/lustify-sdxl-nsfw-checkpoint-ggwp-v7-sdxl".into()),
                trigger: trigger.map(str::to_string),
                ..Default::default()
            },
        )
        .unwrap();
    }

    #[test]
    fn a_ready_checkpoint_with_a_trigger_is_an_identity() {
        let root = temp_models();
        write_person_checkpoint(&root, "jerry.safetensors", Some("ohwx"));
        fs::write(
            root.join("checkpoints/lustifySDXLNSFW_ggwpV7.safetensors"),
            b"lustify",
        )
        .unwrap();
        let catalog = RecipeCatalog::packaged().unwrap();

        let matched = identity_for_prompt(&root, &catalog, "portrait of ohwx").unwrap();
        assert_eq!(matched.filename, "jerry.safetensors");
        assert_eq!(matched.trigger, "ohwx");
        assert!(
            identity_for_prompt(&root, &catalog, "a lighthouse").is_none(),
            "a people base without a trigger is not an identity"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bind_identity_switches_the_checkpoint_only_when_named() {
        let root = temp_models();
        write_identity(&root, "jake.safetensors", "ohwx");
        let mut config = Config {
            models_dir: root.clone(),
            checkpoint: "flux1-schnell-fp8.safetensors".into(),
            ..Default::default()
        };
        let identity = bind_identity(&mut config, "portrait of ohwx").unwrap();
        assert_eq!(identity.filename, "jake.safetensors");
        assert_eq!(config.checkpoint, "jake.safetensors");

        config.checkpoint = "flux1-schnell-fp8.safetensors".into();
        assert!(bind_identity(&mut config, "a lighthouse").is_none());
        assert_eq!(config.checkpoint, "flux1-schnell-fp8.safetensors");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_adapter_without_its_base_is_not_an_identity() {
        let root = temp_models();
        let lora = root.join("loras/jake.safetensors");
        fs::write(&lora, b"lora").unwrap();
        write_sidecar(
            &lora,
            &WeightSidecar {
                recipe_id: "flux-schnell-adapter".into(),
                hf_base: Some("black-forest-labs/FLUX.1-schnell".into()),
                trigger: Some("ohwx".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let catalog = RecipeCatalog::packaged().unwrap();
        assert!(identity_for_prompt(&root, &catalog, "ohwx").is_none());
        let _ = fs::remove_dir_all(root);
    }
}
