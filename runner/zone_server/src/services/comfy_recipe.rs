//! Packaged ComfyUI recipes: a graph pair plus the few slots Zone may write.
//!
//! Chat never picks a workflow. The selected checkpoint filename resolves to a
//! recipe; an attached image selects that recipe's `with_source` graph.

use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;

use super::comfyui::ComfyUiError;

const PACKAGED_CATALOG: &str = include_str!("../../../../comfyui/recipes/catalog.json");

fn packaged_workflow(name: &str) -> Option<&'static str> {
    match name {
        "flux1-schnell-fp8-api.json" => Some(include_str!(
            "../../../../comfyui/workflows/flux1-schnell-fp8-api.json"
        )),
        "flux1-schnell-fp8-img2img-api.json" => Some(include_str!(
            "../../../../comfyui/workflows/flux1-schnell-fp8-img2img-api.json"
        )),
        "sd15-api.json" => Some(include_str!("../../../../comfyui/workflows/sd15-api.json")),
        "sd15-img2img-api.json" => Some(include_str!(
            "../../../../comfyui/workflows/sd15-img2img-api.json"
        )),
        "sdxl-api.json" => Some(include_str!("../../../../comfyui/workflows/sdxl-api.json")),
        "sdxl-img2img-api.json" => Some(include_str!(
            "../../../../comfyui/workflows/sdxl-img2img-api.json"
        )),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeOutput {
    PreviewImage,
}

#[derive(Debug, Clone)]
pub struct Recipe {
    pub id: String,
    pub kind: MediaKind,
    pub label: String,
    bare: Value,
    with_source: Option<Value>,
    slots: RecipeSlots,
}

#[derive(Debug, Clone)]
struct RecipeSlots {
    prompt: String,
    seed: String,
    source: Option<String>,
    weights: HashMap<String, String>,
    output_node: String,
    output: RecipeOutput,
}

pub struct Fill<'a> {
    pub prompt: &'a str,
    pub seed: u64,
    pub weights: HashMap<&'a str, &'a str>,
    pub source: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct CatalogFile {
    schema_version: u32,
    default_image: String,
    recipes: Vec<CatalogRecipe>,
}

#[derive(Debug, Deserialize)]
struct CatalogRecipe {
    id: String,
    kind: MediaKind,
    label: String,
    bare: String,
    with_source: Option<String>,
    output_node: String,
    output: RecipeOutput,
    slots: CatalogSlots,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    base_models: Vec<String>,
    #[serde(default)]
    filename_hints: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CatalogSlots {
    prompt: String,
    seed: String,
    #[serde(default)]
    source: Option<String>,
    weights: HashMap<String, String>,
}

#[derive(Clone)]
pub struct RecipeCatalog {
    default_image: String,
    recipes: Vec<Recipe>,
    files: HashMap<String, String>,
    hints: Vec<(String, String)>,
}

impl RecipeCatalog {
    pub fn packaged() -> Result<Self, ComfyUiError> {
        Self::from_json(PACKAGED_CATALOG, None)
    }

    /// Load recipes from disk when `recipes/catalog.json` sits beside the
    /// workflow directory; otherwise use the packaged catalog. Graphs of the
    /// same filename in that workflow directory overlay the baked-in copies.
    pub fn load(workflow_path: Option<&Path>) -> Result<Self, ComfyUiError> {
        let dir = workflow_path.and_then(Path::parent);
        if let Some(contents) = read_overlay_catalog(dir)? {
            Self::from_json(&contents, dir)
        } else {
            Self::from_json(PACKAGED_CATALOG, dir)
        }
    }

    fn from_json(json: &str, workflow_dir: Option<&Path>) -> Result<Self, ComfyUiError> {
        let file: CatalogFile = serde_json::from_str(json)
            .map_err(|_| ComfyUiError::Configuration("recipe catalog is not valid JSON"))?;
        if file.schema_version != 1 {
            return Err(ComfyUiError::Configuration(
                "unsupported recipe catalog schema",
            ));
        }

        let mut recipes = Vec::with_capacity(file.recipes.len());
        let mut files = HashMap::new();
        let mut hints = Vec::new();

        for spec in file.recipes {
            let slots = RecipeSlots {
                prompt: spec.slots.prompt,
                seed: spec.slots.seed,
                source: spec.slots.source,
                weights: spec.slots.weights,
                output_node: spec.output_node,
                output: spec.output,
            };
            let bare = load_graph(workflow_dir, &spec.bare)?;
            validate_graph(&bare, &slots, false)?;
            let with_source = spec
                .with_source
                .as_deref()
                .map(|name| load_graph(workflow_dir, name))
                .transpose()?;
            if let Some(graph) = &with_source {
                validate_graph(graph, &slots, true)?;
            }
            for filename in spec.files {
                files.insert(filename, spec.id.clone());
            }
            // CivitAI-style family labels are catalog documentation. Matching a
            // checkpoint uses `files` then `filename_hints`, never these labels.
            let _ = spec.base_models;
            for hint in spec.filename_hints {
                hints.push((hint.to_ascii_lowercase(), spec.id.clone()));
            }
            recipes.push(Recipe {
                id: spec.id,
                kind: spec.kind,
                label: spec.label,
                bare,
                with_source,
                slots,
            });
        }

        if !recipes
            .iter()
            .any(|recipe| recipe.id == file.default_image && recipe.kind == MediaKind::Image)
        {
            return Err(ComfyUiError::Configuration(
                "recipe catalog default_image is missing",
            ));
        }

        Ok(Self {
            default_image: file.default_image,
            recipes,
            files,
            hints,
        })
    }

    pub fn get(&self, id: &str) -> Option<&Recipe> {
        self.recipes.iter().find(|recipe| recipe.id == id)
    }

    pub fn image_recipe_for(&self, checkpoint: &str) -> Result<&Recipe, ComfyUiError> {
        let id = self.resolve_image_id(checkpoint);
        self.get(id)
            .filter(|recipe| recipe.kind == MediaKind::Image)
            .ok_or(ComfyUiError::Configuration(
                "no image recipe matches this checkpoint",
            ))
    }

    fn resolve_image_id(&self, checkpoint: &str) -> &str {
        let trimmed = checkpoint.trim();
        if let Some(id) = self.files.get(trimmed) {
            return id;
        }
        let lower = trimmed.to_ascii_lowercase();
        let mut best_id: Option<&str> = None;
        let mut best_len = 0usize;
        for (hint, id) in &self.hints {
            if lower.contains(hint.as_str()) && hint.len() > best_len {
                best_id = Some(id);
                best_len = hint.len();
            }
        }
        best_id.unwrap_or(self.default_image.as_str())
    }
}

impl Recipe {
    pub fn apply(&self, fill: Fill<'_>) -> Result<Value, ComfyUiError> {
        if fill.prompt.trim().is_empty() || fill.prompt.len() > 100_000 {
            return Err(ComfyUiError::Configuration("prompt is empty or too long"));
        }
        let mut workflow = if fill.source.is_some() {
            self.with_source.clone().ok_or(ComfyUiError::Configuration(
                "recipe has no source-image graph",
            ))?
        } else {
            self.bare.clone()
        };
        set_pointer(&mut workflow, &self.slots.prompt, json!(fill.prompt))?;
        set_pointer(&mut workflow, &self.slots.seed, json!(fill.seed))?;
        for (name, pointer) in &self.slots.weights {
            let filename = fill
                .weights
                .get(name.as_str())
                .copied()
                .ok_or(ComfyUiError::Configuration("recipe weight is missing"))?;
            let filename = sanitize_weight_filename(filename)?;
            set_pointer(&mut workflow, pointer, json!(filename))?;
        }
        if let Some(source) = fill.source {
            let pointer = self
                .slots
                .source
                .as_deref()
                .ok_or(ComfyUiError::Configuration(
                    "recipe has no source-image slot",
                ))?;
            set_pointer(&mut workflow, pointer, json!(sanitize_upload_name(source)?))?;
        }
        Ok(workflow)
    }
}

fn read_overlay_catalog(workflow_dir: Option<&Path>) -> Result<Option<String>, ComfyUiError> {
    let Some(path) = workflow_dir
        .and_then(Path::parent)
        .map(|root| root.join("recipes").join("catalog.json"))
        .filter(|path| path.is_file())
    else {
        return Ok(None);
    };
    std::fs::read_to_string(path)
        .map(Some)
        .map_err(|_| ComfyUiError::Configuration("recipe catalog is not readable"))
}

fn load_graph(dir: Option<&Path>, filename: &str) -> Result<Value, ComfyUiError> {
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err(ComfyUiError::Configuration("invalid workflow filename"));
    }
    if let Some(path) = dir
        .map(|dir| dir.join(filename))
        .filter(|path| path.is_file())
    {
        let contents = std::fs::read_to_string(path)
            .map_err(|_| ComfyUiError::Configuration("workflow file is not readable"))?;
        return serde_json::from_str(&contents)
            .map_err(|_| ComfyUiError::Configuration("workflow file is not valid JSON"));
    }
    let packaged = packaged_workflow(filename)
        .ok_or(ComfyUiError::Configuration("packaged workflow is missing"))?;
    serde_json::from_str(packaged)
        .map_err(|_| ComfyUiError::Configuration("packaged workflow is not valid JSON"))
}

fn validate_graph(
    workflow: &Value,
    slots: &RecipeSlots,
    with_source: bool,
) -> Result<(), ComfyUiError> {
    require_pointer(workflow, &slots.prompt)?;
    require_pointer(workflow, &slots.seed)?;
    for pointer in slots.weights.values() {
        require_pointer(workflow, pointer)?;
    }
    if with_source {
        let source = slots.source.as_deref().ok_or(ComfyUiError::Configuration(
            "source recipe is missing a source slot",
        ))?;
        require_pointer(workflow, source)?;
        if workflow
            .pointer(&source_class_pointer(source))
            .and_then(Value::as_str)
            != Some("LoadImage")
        {
            return Err(ComfyUiError::Configuration(
                "source graph must load a source image",
            ));
        }
    }
    let output_class = format!("/{}/class_type", slots.output_node);
    match slots.output {
        RecipeOutput::PreviewImage => {
            if workflow.pointer(&output_class).and_then(Value::as_str) != Some("PreviewImage") {
                return Err(ComfyUiError::Configuration(
                    "workflow output must use temporary PreviewImage storage",
                ));
            }
        }
    }
    Ok(())
}

fn source_class_pointer(source_slot: &str) -> String {
    match source_slot.rsplit_once("/inputs/") {
        Some((prefix, _)) => format!("{prefix}/class_type"),
        None => source_slot.to_string(),
    }
}

fn require_pointer(workflow: &Value, pointer: &str) -> Result<(), ComfyUiError> {
    if workflow.pointer(pointer).is_none() {
        return Err(ComfyUiError::Configuration(
            "workflow does not match the recipe slot contract",
        ));
    }
    Ok(())
}

fn set_pointer(root: &mut Value, pointer: &str, value: Value) -> Result<(), ComfyUiError> {
    if !pointer.starts_with('/') || pointer.len() < 2 || pointer.contains("//") {
        return Err(ComfyUiError::Configuration("invalid recipe slot pointer"));
    }
    let mut current = root;
    let parts: Vec<&str> = pointer[1..].split('/').collect();
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            return Err(ComfyUiError::Configuration("invalid recipe slot pointer"));
        }
        if index + 1 == parts.len() {
            let object = current.as_object_mut().ok_or(ComfyUiError::Configuration(
                "recipe slot pointer is not an object",
            ))?;
            object.insert((*part).to_string(), value);
            return Ok(());
        }
        current = current.get_mut(*part).ok_or(ComfyUiError::Configuration(
            "workflow does not match the recipe slot contract",
        ))?;
    }
    Err(ComfyUiError::Configuration("invalid recipe slot pointer"))
}

pub fn sanitize_weight_filename(name: &str) -> Result<String, ComfyUiError> {
    if name.is_empty()
        || name.len() > 256
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
    {
        return Err(ComfyUiError::Configuration("invalid checkpoint filename"));
    }
    Ok(name.to_string())
}

pub fn sanitize_upload_name(name: &str) -> Result<String, ComfyUiError> {
    if name.is_empty()
        || name.len() > 128
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        return Err(ComfyUiError::Configuration("invalid source image filename"));
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> RecipeCatalog {
        RecipeCatalog::packaged().unwrap()
    }

    #[test]
    fn packaged_catalog_validates_every_graph() {
        let catalog = catalog();
        assert!(catalog.get("flux-schnell").is_some());
        assert!(catalog.get("sd15").is_some());
        assert!(catalog.get("sdxl").is_some());
        for name in [
            "flux1-schnell-fp8-api.json",
            "flux1-schnell-fp8-img2img-api.json",
            "sd15-api.json",
            "sd15-img2img-api.json",
            "sdxl-api.json",
            "sdxl-img2img-api.json",
        ] {
            assert!(packaged_workflow(name).is_some(), "{name}");
        }
    }

    #[test]
    fn checkpoint_filename_selects_recipe() {
        let catalog = catalog();
        assert_eq!(
            catalog
                .image_recipe_for("flux1-schnell-fp8.safetensors")
                .unwrap()
                .id,
            "flux-schnell"
        );
        assert_eq!(
            catalog
                .image_recipe_for("juggernautXL_ragnarok.safetensors")
                .unwrap()
                .id,
            "sdxl"
        );
        assert_eq!(
            catalog
                .image_recipe_for("mystery-finetune.safetensors")
                .unwrap()
                .id,
            "flux-schnell"
        );
        assert_eq!(
            catalog
                .image_recipe_for("v1-5-pruned-emaonly.safetensors")
                .unwrap()
                .id,
            "sd15"
        );
        assert_eq!(
            catalog
                .image_recipe_for("ponyDiffusionV6XL_v6.safetensors")
                .unwrap()
                .id,
            "sdxl"
        );
        // Family labels are not checkpoint filenames; an unknown name stays
        // on the default image recipe instead of matching "SD 1.5".
        assert_eq!(
            catalog.image_recipe_for("SD 1.5").unwrap().id,
            "flux-schnell"
        );
    }

    #[test]
    fn apply_writes_only_recipe_slots() {
        let catalog = catalog();
        let recipe = catalog
            .image_recipe_for("flux1-schnell-fp8.safetensors")
            .unwrap();
        let workflow = recipe
            .apply(Fill {
                prompt: "a blue fox",
                seed: 42,
                weights: HashMap::from([("checkpoint", "custom-image.safetensors")]),
                source: None,
            })
            .unwrap();
        assert_eq!(
            workflow["4"]["inputs"]["ckpt_name"],
            "custom-image.safetensors"
        );
        assert_eq!(workflow["6"]["inputs"]["text"], "a blue fox");
        assert_eq!(workflow["3"]["inputs"]["seed"], 42);
        assert_eq!(workflow["3"]["inputs"]["steps"], 4);
        assert_eq!(workflow["5"]["inputs"]["width"], 1024);
        assert!(workflow.get("10").is_none());
    }

    #[test]
    fn sdxl_recipe_keeps_packaged_sampler() {
        let catalog = catalog();
        let recipe = catalog
            .image_recipe_for("sd_xl_base_1.0.safetensors")
            .unwrap();
        let workflow = recipe
            .apply(Fill {
                prompt: "a fox",
                seed: 1,
                weights: HashMap::from([("checkpoint", "juggernautXL.safetensors")]),
                source: None,
            })
            .unwrap();
        assert_eq!(workflow["3"]["inputs"]["steps"], 25);
        assert_eq!(workflow["5"]["inputs"]["width"], 1024);
        assert_eq!(
            workflow["4"]["inputs"]["ckpt_name"],
            "juggernautXL.safetensors"
        );
    }

    #[test]
    fn source_graph_fills_load_image() {
        let catalog = catalog();
        let recipe = catalog.get("sd15").unwrap();
        let workflow = recipe
            .apply(Fill {
                prompt: "make it dusk",
                seed: 7,
                weights: HashMap::from([("checkpoint", "model.safetensors")]),
                source: Some("zone-img2img-source.png"),
            })
            .unwrap();
        assert_eq!(workflow["10"]["inputs"]["image"], "zone-img2img-source.png");
        assert_eq!(workflow["11"]["inputs"]["width"], 512);
        assert_eq!(workflow["3"]["inputs"]["denoise"], 0.7);
    }

    #[test]
    fn rejects_pathful_weights_and_sources() {
        let catalog = catalog();
        let recipe = catalog.get("flux-schnell").unwrap();
        assert!(
            recipe
                .apply(Fill {
                    prompt: "fox",
                    seed: 1,
                    weights: HashMap::from([("checkpoint", "../secret")]),
                    source: None,
                })
                .is_err()
        );
        assert!(
            recipe
                .apply(Fill {
                    prompt: "fox",
                    seed: 1,
                    weights: HashMap::from([("checkpoint", "ok.safetensors")]),
                    source: Some("../secret.png"),
                })
                .is_err()
        );
    }

    #[test]
    fn unknown_packaged_workflow_name_is_rejected() {
        assert!(packaged_workflow("missing.json").is_none());
    }

    #[test]
    fn overlay_catalog_replaces_packaged_recipes() {
        let root =
            std::env::temp_dir().join(format!("zone-recipe-overlay-{}", uuid::Uuid::new_v4()));
        let workflows = root.join("workflows");
        let recipes = root.join("recipes");
        std::fs::create_dir_all(&workflows).unwrap();
        std::fs::create_dir_all(&recipes).unwrap();
        std::fs::write(
            workflows.join("custom-api.json"),
            packaged_workflow("sd15-api.json").unwrap(),
        )
        .unwrap();
        std::fs::write(
            workflows.join("custom-img2img-api.json"),
            packaged_workflow("sd15-img2img-api.json").unwrap(),
        )
        .unwrap();
        std::fs::write(
            recipes.join("catalog.json"),
            r#"{
              "schema_version": 1,
              "default_image": "custom",
              "recipes": [{
                "id": "custom",
                "kind": "image",
                "label": "Custom",
                "bare": "custom-api.json",
                "with_source": "custom-img2img-api.json",
                "output_node": "9",
                "output": "preview_image",
                "slots": {
                  "prompt": "/6/inputs/text",
                  "seed": "/3/inputs/seed",
                  "source": "/10/inputs/image",
                  "weights": { "checkpoint": "/4/inputs/ckpt_name" }
                },
                "files": ["custom.safetensors"]
              }]
            }"#,
        )
        .unwrap();

        let catalog = RecipeCatalog::load(Some(&workflows.join("custom-api.json"))).unwrap();
        assert_eq!(
            catalog.image_recipe_for("custom.safetensors").unwrap().id,
            "custom"
        );
        assert!(catalog.get("flux-schnell").is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn persistent_output_nodes_are_rejected() {
        let mut workflow: Value = serde_json::from_str(
            packaged_workflow("flux1-schnell-fp8-api.json").expect("packaged flux graph"),
        )
        .unwrap();
        workflow["9"]["class_type"] = json!("SaveImage");
        let slots = RecipeSlots {
            prompt: "/6/inputs/text".into(),
            seed: "/3/inputs/seed".into(),
            source: None,
            weights: HashMap::from([("checkpoint".into(), "/4/inputs/ckpt_name".into())]),
            output_node: "9".into(),
            output: RecipeOutput::PreviewImage,
        };
        assert!(
            validate_graph(&workflow, &slots, false)
                .unwrap_err()
                .to_string()
                .contains("PreviewImage")
        );
    }
}
