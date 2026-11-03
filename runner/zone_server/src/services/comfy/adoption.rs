use super::overlay;
use super::policy::LORAS;
use super::policy::train_policy;
use abnegate_comfy::Config;
use abnegate_comfy::lora::TrainPolicy;
use abnegate_comfy::recipe::RecipeCatalog;
use serde_json::Map;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::io::Write;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use uuid::Uuid;

const GENERATION: &str = "generation";
const RECIPE: &str = "recipe_id";
const TRIGGER: &str = "trigger";
const TEMPORARY_SUFFIX: &str = ".adopting";

/// Gives every weight zone's trainers published before abnegate-comfy the
/// `generation` the crate looks for before it replaces a weight, so the name
/// trains again. A sidecar that already has one, names a recipe zone does not
/// train or carries no `trigger`, as a downloaded LoRA's does not, is left
/// alone. Returns how many sidecars were adopted.
pub fn adopt(config: &Config) -> usize {
    let catalog = match overlay::catalog(config) {
        Ok(catalog) => catalog,
        Err(error) => {
            tracing::warn!(%error, "Could not load the recipe catalog to adopt trained weights");
            return 0;
        }
    };
    let policy = train_policy();
    let recipes = trained_recipes(&policy, &catalog);
    let mut adopted = 0;
    for directory in directories(&policy) {
        let Some(folder) = confined(&config.models_directory, &directory) else {
            continue;
        };
        for sidecar in sidecars(&folder, &config.contract.sidecar_suffix) {
            match adopt_sidecar(&folder, &sidecar, &recipes) {
                Ok(true) => adopted += 1,
                Ok(false) => {}
                Err(error) => tracing::warn!(
                    %error,
                    path = %folder.join(&sidecar).display(),
                    "Could not adopt a weight trained before abnegate-comfy"
                ),
            }
        }
    }
    tracing::info!(adopted, "Adopted weights trained before abnegate-comfy");
    adopted
}

fn trained_recipes(policy: &TrainPolicy, catalog: &RecipeCatalog) -> BTreeSet<String> {
    let mut recipes: BTreeSet<String> = policy
        .rules
        .iter()
        .map(|rule| rule.recipe_id.clone())
        .collect();
    for recipe in catalog.image_recipes().filter(|recipe| !recipe.adapter) {
        if policy.rules_for(&recipe.id).next().is_some() {
            continue;
        }
        if let Ok(adapter) = recipe.training_adapter() {
            recipes.insert(adapter.recipe_id.clone());
        }
    }
    recipes
}

fn directories(policy: &TrainPolicy) -> BTreeSet<String> {
    policy
        .rules
        .iter()
        .map(|rule| rule.directory.clone())
        .chain([LORAS.to_string()])
        .collect()
}

fn sidecars(folder: &Path, suffix: &str) -> Vec<String> {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            tracing::warn!(
                %error,
                path = %folder.display(),
                "Could not list weights trained before abnegate-comfy"
            );
            return Vec::new();
        }
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.len() > suffix.len() && name.ends_with(suffix))
        .collect()
}

fn adopt_sidecar(folder: &Path, name: &str, recipes: &BTreeSet<String>) -> io::Result<bool> {
    let path =
        confined(folder, name).ok_or_else(|| invalid("the sidecar name is not one entry"))?;
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.file_type().is_file() {
        return Err(invalid("the sidecar is not a regular file"));
    }
    let mut document: Map<String, Value> = serde_json::from_slice(&fs::read(&path)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let trained = document
        .get(RECIPE)
        .and_then(Value::as_str)
        .is_some_and(|recipe| recipes.contains(recipe));
    if document.contains_key(GENERATION) || !document.contains_key(TRIGGER) || !trained {
        return Ok(false);
    }
    document.insert(
        GENERATION.to_string(),
        Value::String(Uuid::new_v4().to_string()),
    );
    let encoded = serde_json::to_vec_pretty(&document)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    replace(folder, &path, name, &encoded)?;
    Ok(true)
}

fn replace(folder: &Path, path: &Path, name: &str, contents: &[u8]) -> io::Result<()> {
    let temporary = confined(
        folder,
        &format!(".{name}.{}{TEMPORARY_SUFFIX}", Uuid::new_v4()),
    )
    .ok_or_else(|| invalid("the temporary name is not one entry"))?;
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut file| {
            file.write_all(contents)?;
            file.sync_all()
        })
        .and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// `directory` joined with `name` when `name` is exactly one plain entry.
fn confined(directory: &Path, name: &str) -> Option<PathBuf> {
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(entry)), None) if entry == name => Some(directory.join(entry)),
        _ => None,
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::comfy::policy::CHECKPOINTS;
    use crate::services::comfy::policy::PEOPLE_ADAPTER;
    use crate::services::comfy::policy::PEOPLE_BASE;
    use crate::services::comfy::policy::PEOPLE_CHECKPOINT;
    use crate::services::comfy::policy::PEOPLE_FINETUNE;
    use crate::services::comfy::settings;
    use abnegate_comfy::lora::TrainError;
    use abnegate_comfy::lora::TrainImage;
    use abnegate_comfy::lora::TrainMethod;
    use abnegate_comfy::lora::TrainRequest;
    use abnegate_comfy::lora::TrainSubject;
    use abnegate_comfy::lora::validate_request;
    use serde_json::json;

    const FOREIGN_WEIGHT: &str = "a weight training did not publish already has this name";
    const HUGGINGFACE_BASE: &str = "stabilityai/stable-diffusion-xl-base-1.0";

    fn harness() -> (tempfile::TempDir, Config) {
        let models = tempfile::tempdir().expect("models directory");
        let mut config = settings::development();
        config.enabled = true;
        config.models_directory = models.path().to_path_buf();
        let checkpoint = config.models_directory.join(PEOPLE_CHECKPOINT);
        fs::create_dir_all(checkpoint.parent().expect("checkpoints")).expect("checkpoints");
        fs::write(checkpoint, b"weights").expect("people checkpoint");
        (models, config)
    }

    fn publish(config: &Config, directory: &str, weight: &str, sidecar: &Value) -> PathBuf {
        let folder = config.models_directory.join(directory);
        fs::create_dir_all(&folder).expect("weight folder");
        fs::write(folder.join(weight), b"weights").expect("weight");
        let path = folder.join(format!("{weight}{}", config.contract.sidecar_suffix));
        fs::write(&path, serde_json::to_vec(sidecar).expect("sidecar")).expect("sidecar");
        path
    }

    fn read(path: &Path) -> Map<String, Value> {
        serde_json::from_slice(&fs::read(path).expect("sidecar")).expect("a JSON object")
    }

    fn pivotal() -> Value {
        json!({
            "recipe_id": PEOPLE_ADAPTER,
            "hf_base": HUGGINGFACE_BASE,
            "trigger": "ohwx",
            "embedding": "jerry_emb.safetensors",
        })
    }

    fn person(method: TrainMethod) -> TrainRequest {
        TrainRequest::new(
            "jerry",
            PEOPLE_BASE,
            vec![TrainImage::from_bytes("one.png", "", b"image".to_vec())],
        )
        .with_trigger("ohwx")
        .with_subject(TrainSubject::Person)
        .with_method(method)
    }

    fn unchanged(config: &Config, sidecar: &Path) {
        let before = fs::read(sidecar).expect("sidecar");
        assert_eq!(adopt(config), 0);
        assert_eq!(fs::read(sidecar).expect("sidecar"), before);
    }

    #[test]
    fn a_person_trained_before_the_upgrade_trains_again_under_the_same_name() {
        let (_models, config) = harness();
        let sidecar = publish(&config, LORAS, "jerry.safetensors", &pivotal());
        let request = person(TrainMethod::Pivotal);
        match validate_request(&config, &request) {
            Err(TrainError::Invalid(message)) => assert_eq!(message, FOREIGN_WEIGHT),
            other => panic!("the crate took a weight it did not publish: {other:?}"),
        }

        assert_eq!(adopt(&config), 1);

        validate_request(&config, &request).expect("the adopted name trains again");
        let mut adopted = read(&sidecar);
        let generation = adopted.remove(GENERATION).expect("a generation");
        assert!(
            generation
                .as_str()
                .is_some_and(|generation| Uuid::parse_str(generation).is_ok()),
            "{generation}"
        );
        assert_eq!(Value::Object(adopted), pivotal(), "every other key is kept");
    }

    #[test]
    fn a_fine_tune_trained_before_the_upgrade_is_adopted_among_the_checkpoints() {
        let (_models, config) = harness();
        let sidecar = publish(
            &config,
            CHECKPOINTS,
            "jerry.safetensors",
            &json!({"recipe_id": PEOPLE_FINETUNE, "hf_base": HUGGINGFACE_BASE, "trigger": "ohwx"}),
        );

        assert_eq!(adopt(&config), 1);

        assert!(read(&sidecar).contains_key(GENERATION));
        validate_request(&config, &person(TrainMethod::Finetune))
            .expect("the adopted fine-tune trains again");
    }

    #[test]
    fn a_sidecar_with_a_generation_is_left_alone() {
        let (_models, config) = harness();
        let mut published = pivotal();
        published[GENERATION] = json!("not-a-generation");
        let sidecar = publish(&config, LORAS, "jerry.safetensors", &published);

        unchanged(&config, &sidecar);
    }

    #[test]
    fn a_weight_for_a_recipe_zone_does_not_train_is_left_alone() {
        let (_models, config) = harness();
        let sidecar = publish(
            &config,
            LORAS,
            "walk.safetensors",
            &json!({"recipe_id": "wan-adapter", "hf_base": "Wan-AI/Wan2.2-T2V-A14B", "trigger": "walk"}),
        );

        unchanged(&config, &sidecar);
    }

    #[test]
    fn a_downloaded_lora_with_no_trigger_is_left_alone() {
        let (_models, config) = harness();
        let sidecar = publish(
            &config,
            LORAS,
            "style.safetensors",
            &json!({"recipe_id": "flux-schnell-adapter", "hf_base": "black-forest-labs/FLUX.1-schnell"}),
        );

        unchanged(&config, &sidecar);
    }

    #[test]
    fn a_symlinked_sidecar_is_refused() {
        let (models, config) = harness();
        let outside = models.path().join("outside.json");
        fs::write(&outside, serde_json::to_vec(&pivotal()).expect("sidecar")).expect("target");
        let folder = config.models_directory.join(LORAS);
        fs::create_dir_all(&folder).expect("loras");
        fs::write(folder.join("jerry.safetensors"), b"weights").expect("weight");
        let link = folder.join(format!(
            "jerry.safetensors{}",
            config.contract.sidecar_suffix
        ));
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");

        unchanged(&config, &outside);

        assert!(
            fs::symlink_metadata(&link)
                .expect("link")
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn adopting_twice_changes_nothing_the_second_time() {
        let (_models, config) = harness();
        let sidecar = publish(&config, LORAS, "jerry.safetensors", &pivotal());

        assert_eq!(adopt(&config), 1);
        unchanged(&config, &sidecar);

        let leftovers: Vec<String> = fs::read_dir(config.models_directory.join(LORAS))
            .expect("loras")
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.ends_with(TEMPORARY_SUFFIX))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn every_trained_recipe_comes_from_the_policy_and_catalog() {
        let policy = train_policy();
        let catalog = overlay::catalog(&settings::development()).expect("catalog");
        let recipes = trained_recipes(&policy, &catalog);
        for recipe in [PEOPLE_ADAPTER, PEOPLE_FINETUNE, "flux-schnell-adapter"] {
            assert!(
                recipes.contains(recipe),
                "{recipe} is missing from {recipes:?}"
            );
        }
        assert!(!recipes.contains("wan-adapter"), "{recipes:?}");
        assert_eq!(
            directories(&policy),
            BTreeSet::from([CHECKPOINTS.to_string(), LORAS.to_string()])
        );
    }

    #[test]
    fn a_name_that_is_not_one_entry_is_not_confined() {
        let root = Path::new("/models");
        assert_eq!(
            confined(root, "jerry.safetensors.zone.json"),
            Some(root.join("jerry.safetensors.zone.json"))
        );
        for name in ["", ".", "..", "../jerry", "loras/jerry", "/jerry"] {
            assert_eq!(confined(root, name), None, "{name}");
        }
    }
}
