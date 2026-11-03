use abnegate_comfy::Config;
use abnegate_comfy::Error;
use abnegate_comfy::recipe::RecipeCatalog;

/// The recipes zone generates with: `recipes/catalog.json` beside the
/// configured workflow directory, whose people base and uncensored graphs
/// the crate does not ship, or the crate's packaged catalog when the
/// deployment has none.
pub fn catalog(config: &Config) -> Result<RecipeCatalog, Error> {
    RecipeCatalog::load(config.workflow_path.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::comfy::policy::PEOPLE_BASE;
    use crate::services::comfy::settings;
    use abnegate_comfy::recipe::Fill;
    use abnegate_comfy::recipe::MediaKind;
    use abnegate_comfy::recipe::Recipe;
    use abnegate_comfy::recipe::RecipeExtra;
    use serde_json::Value;
    use std::collections::HashMap;

    const UNCENSORED: &str = "15";

    fn repository() -> RecipeCatalog {
        catalog(&settings::development()).expect("the repository's catalog loads")
    }

    fn people(catalog: &RecipeCatalog) -> &Recipe {
        catalog.get(PEOPLE_BASE).expect("the people base")
    }

    fn filled(recipe: &Recipe, fill: impl FnOnce(Fill<'_>) -> Fill<'_>) -> Value {
        let weights = recipe
            .weight_map("trained.safetensors")
            .expect("every weight slot has a file");
        let weights: HashMap<&str, &str> = weights
            .iter()
            .map(|(slot, file)| (slot.as_str(), file.as_str()))
            .collect();
        recipe
            .apply(fill(Fill::new("a portrait", 7, weights)))
            .expect("the graph fills")
    }

    #[test]
    fn the_repository_catalog_loads_through_the_crate() {
        let catalog = repository();
        for id in [PEOPLE_BASE, "sdxl-adapter", "sdxl", "flux-schnell"] {
            assert!(catalog.get(id).is_some(), "{id} is missing");
        }
        let video = catalog.get("wan-a14b").expect("wan-a14b is missing");
        assert_eq!(video.kind, MediaKind::Video);
    }

    #[test]
    fn the_default_image_recipe_is_the_people_base() {
        let catalog = repository();
        assert_eq!(
            catalog
                .image_recipe_for("unrecognised.safetensors")
                .expect("the default image recipe")
                .id,
            PEOPLE_BASE
        );
        assert!(!catalog.is_explicit_image_match("unrecognised.safetensors"));
    }

    #[test]
    fn the_lustify_checkpoint_resolves_to_the_people_base() {
        let catalog = repository();
        assert_eq!(
            catalog
                .image_recipe_for(settings::CHECKPOINT)
                .expect("a recipe for Lustify")
                .id,
            PEOPLE_BASE
        );
        assert!(catalog.is_explicit_image_match(settings::CHECKPOINT));
    }

    #[test]
    fn the_uncensored_graphs_load_from_the_overlay() {
        let catalog = repository();
        for id in [
            "flux-schnell",
            "flux-schnell-adapter",
            "flux-dev",
            "flux-dev-adapter",
            "qwen-image-edit",
            "qwen-image-edit-adapter",
        ] {
            let recipe = catalog.get(id).expect("an uncensored recipe");
            let graph = filled(recipe, |fill| fill);
            assert_eq!(
                graph
                    .pointer(&format!("/{UNCENSORED}/class_type"))
                    .and_then(Value::as_str),
                Some("LoraLoaderModelOnly"),
                "{id} runs the crate's packaged graph instead of zone's"
            );
        }
    }

    #[test]
    fn a_negative_prompt_is_written_into_the_people_graph() {
        let catalog = repository();
        let graph = filled(people(&catalog), |fill| fill.with_negative("blurry"));
        assert_eq!(
            graph.pointer("/7/inputs/text").and_then(Value::as_str),
            Some("blurry")
        );
        assert_eq!(
            graph.pointer("/6/inputs/text").and_then(Value::as_str),
            Some("a portrait")
        );
    }

    #[test]
    fn the_people_graph_is_posed_only_when_asked() {
        let catalog = repository();
        let recipe = people(&catalog);
        assert!(recipe.has_pose_slot());
        let posed = filled(recipe, |fill| fill.with_pose("zone-pose-sitting.png"));
        assert_eq!(
            posed.pointer("/20/inputs/image").and_then(Value::as_str),
            Some("zone-pose-sitting.png")
        );
        let unposed = filled(recipe, |fill| fill);
        for node in ["20", "21", "22"] {
            assert!(
                unposed.get(node).is_none(),
                "node {node} of the ControlNet branch outlived an unposed fill"
            );
        }
    }

    #[test]
    fn the_people_recipes_take_a_face_and_refine_it() {
        let catalog = repository();
        for id in [PEOPLE_BASE, "sdxl-adapter"] {
            let recipe = catalog.get(id).expect("a people recipe");
            assert!(recipe.supports(RecipeExtra::Face), "{id}");
            assert!(recipe.supports(RecipeExtra::Refine), "{id}");
        }
        let sdxl = catalog.get("sdxl").expect("the plain SDXL base");
        assert!(sdxl.supports(RecipeExtra::Face));
        assert!(sdxl.supports(RecipeExtra::IdentityRefine));
    }

    #[test]
    fn without_an_overlay_the_packaged_catalog_is_used() {
        let empty = tempfile::tempdir().expect("an empty deployment");
        let mut config = settings::development();
        config.workflow_path = Some(
            empty
                .path()
                .join("workflows")
                .join(settings::IMAGE_WORKFLOW),
        );
        let packaged = catalog(&config).expect("the packaged catalog");
        assert!(packaged.get(PEOPLE_BASE).is_none());
        let graph = filled(
            packaged.get("flux-schnell").expect("a packaged recipe"),
            |fill| fill,
        );
        assert!(
            graph.get(UNCENSORED).is_none(),
            "the packaged graph carries no uncensored LoRA"
        );
    }
}
