//! Scan the ComfyUI models directory and join files to packaged recipes.

use super::comfy_recipe::{Recipe, RecipeCatalog, RequiredFile, sanitize_weight_filename};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const SIDECAR_SUFFIX: &str = ".zone.json";

const SCAN_DIRECTORIES: &[(&str, &str)] = &[
    ("checkpoints", "checkpoint"),
    ("diffusion_models", "diffusion_model"),
    ("loras", "lora"),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WeightSidecar {
    pub recipe_id: String,
    #[serde(default)]
    pub hf_base: Option<String>,
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
}

pub fn scan(models_dir: &Path, catalog: &RecipeCatalog) -> Vec<InventoryItem> {
    let mut items = Vec::new();
    for (directory, kind) in SCAN_DIRECTORIES {
        let folder = models_dir.join(directory);
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
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
            let metadata = fs::metadata(&path).ok();
            let size = metadata.as_ref().map(|meta| meta.len()).unwrap_or(0);
            let modified_at = metadata
                .and_then(|meta| meta.modified().ok())
                .and_then(rfc3339);
            let sidecar = read_sidecar(&path);
            let recipe = resolve_recipe(catalog, filename, kind, sidecar.as_ref());
            let Some(recipe) = recipe else {
                continue;
            };
            let missing = missing_required(models_dir, &recipe.required_files);
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
                    super::comfy_recipe::PromptMode::ClipScene => "clip_scene".to_string(),
                    super::comfy_recipe::PromptMode::EditInstruction => {
                        "edit_instruction".to_string()
                    }
                },
            });
        }
    }
    items.sort_by(|left, right| left.filename.cmp(&right.filename));
    items
}

pub fn find<'a>(items: &'a [InventoryItem], filename: &str) -> Option<&'a InventoryItem> {
    items.iter().find(|item| item.filename == filename)
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
    sidecar: Option<&WeightSidecar>,
) -> Option<&'a Recipe> {
    if let Some(sidecar) = sidecar
        && let Some(recipe) = catalog.get(&sidecar.recipe_id)
    {
        return Some(recipe);
    }
    if kind == "lora" {
        return catalog.adapter_recipe_for_filename(filename);
    }
    catalog.image_recipe_for(filename).ok()
}

fn missing_required(models_dir: &Path, required: &[RequiredFile]) -> Vec<String> {
    required
        .iter()
        .filter(|file| {
            !models_dir
                .join(&file.directory)
                .join(&file.filename)
                .is_file()
        })
        .map(|file| file.filename.clone())
        .collect()
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

fn read_sidecar(weight: &Path) -> Option<WeightSidecar> {
    let contents = fs::read_to_string(sidecar_path(weight)).ok()?;
    serde_json::from_str(&contents).ok()
}

fn rfc3339(time: SystemTime) -> Option<String> {
    let datetime = chrono::DateTime::<chrono::Utc>::from(time);
    Some(datetime.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::comfy_recipe::RecipeCatalog;
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

    #[test]
    fn adapter_is_not_ready_without_base() {
        let root = temp_models();
        let lora = root.join("loras/qwen-image-edit-plus-nsfw-lora.safetensors");
        fs::write(&lora, b"lora").unwrap();
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
        fs::write(
            root.join("loras/qwen-image-edit-plus-nsfw-lora.safetensors"),
            b"lora",
        )
        .unwrap();
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
    fn sidecar_selects_qwen_adapter_over_filename_heuristic() {
        let root = temp_models();
        let lora = root.join("loras/custom-style.safetensors");
        fs::write(&lora, b"lora").unwrap();
        write_sidecar(
            &lora,
            &WeightSidecar {
                recipe_id: "qwen-image-edit-adapter".into(),
                hf_base: Some("Qwen/Qwen-Image-Edit-2511".into()),
            },
        )
        .unwrap();
        let catalog = RecipeCatalog::packaged().unwrap();
        let items = scan(&root, &catalog);
        assert_eq!(items[0].recipe_id, "qwen-image-edit-adapter");
        let _ = fs::remove_dir_all(root);
    }
}
