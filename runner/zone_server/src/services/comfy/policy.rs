use abnegate_comfy::host::Compute;
use abnegate_comfy::lora::TrainError;
use abnegate_comfy::lora::TrainMethod;
use abnegate_comfy::lora::TrainPolicy;
use abnegate_comfy::lora::TrainRequest;
use abnegate_comfy::lora::TrainRule;
use abnegate_comfy::lora::TrainSubject;
use abnegate_secret::SecretValue;

/// The one base a person trains on: Lustify, an SDXL checkpoint.
pub const PEOPLE_BASE: &str = "sdxl-people";
/// Runs a person LoRA or pivotal LoRA over the people base.
pub const PEOPLE_ADAPTER: &str = "sdxl-adapter";
/// Runs a fine-tuned people checkpoint.
pub const PEOPLE_FINETUNE: &str = "sdxl";
/// Runs a person's video adapter.
pub const VIDEO_ADAPTER: &str = "wan-adapter";
pub const PEOPLE_CHECKPOINT: &str = "checkpoints/lustifySDXLNSFW_ggwpV7.safetensors";
pub const VIDEO_MODEL: &str = "diffusion_models/wan2.2_ti2v_5B_fp16.safetensors";
/// Marks a person's video adapter apart from their image LoRA of the same name.
pub const VIDEO_SUFFIX: &str = "-wan";
pub const LORAS: &str = "loras";
pub const CHECKPOINTS: &str = "checkpoints";
pub const MISSING_RUNPOD_KEY: &str = "Save a Runpod API key in Workspace Settings.";
pub const REMOTE_SUBJECT_UNAVAILABLE: &str = "this subject trains only on this machine";

/// What each base trains and where it publishes: the people base trains a
/// person's LoRA, pivotal LoRA, fine-tune and video adapter, and an object
/// or style LoRA; every other base keeps the crate's default, a LoRA of
/// anything but a person.
pub fn train_policy() -> TrainPolicy {
    TrainPolicy::new()
        .with_rule(
            TrainRule::new(PEOPLE_BASE, TrainMethod::Lora, PEOPLE_ADAPTER, LORAS)
                .with_subjects([TrainSubject::Person, TrainSubject::Other])
                .with_required(PEOPLE_CHECKPOINT),
        )
        .with_rule(
            TrainRule::new(PEOPLE_BASE, TrainMethod::Pivotal, PEOPLE_ADAPTER, LORAS)
                .with_subjects([TrainSubject::Person])
                .with_required(PEOPLE_CHECKPOINT),
        )
        .with_rule(
            TrainRule::new(
                PEOPLE_BASE,
                TrainMethod::Finetune,
                PEOPLE_FINETUNE,
                CHECKPOINTS,
            )
            .with_subjects([TrainSubject::Person])
            .with_required(PEOPLE_CHECKPOINT),
        )
        .with_rule(
            TrainRule::new(PEOPLE_BASE, TrainMethod::Video, VIDEO_ADAPTER, LORAS)
                .with_subjects([TrainSubject::Person])
                .with_required(PEOPLE_CHECKPOINT)
                .with_required(VIDEO_MODEL)
                .with_suffix(VIDEO_SUFFIX),
        )
}

/// Whether `subject` may train on `compute`. Every subject trains on this
/// machine; Runpod takes the image subjects zone knows of.
pub fn allows(subject: TrainSubject, compute: Compute) -> bool {
    match compute {
        Compute::Local => true,
        Compute::Runpod => matches!(subject, TrainSubject::Person | TrainSubject::Other),
        _ => false,
    }
}

/// The key `compute` trains with, from the workspace's saved key: none for
/// this machine, and the saved key for Runpod, which refuses to start
/// without one.
pub fn key(
    compute: Compute,
    saved: Option<SecretValue>,
) -> Result<Option<SecretValue>, TrainError> {
    if compute.key_file().is_none() {
        return Ok(None);
    }
    saved
        .map(|key| key.expose().trim().to_string())
        .filter(|key| !key.is_empty())
        .map(|key| Some(SecretValue::new(key)))
        .ok_or(TrainError::Invalid(MISSING_RUNPOD_KEY))
}

/// `request` placed on `compute`, carrying the key it trains with.
pub fn place(
    request: TrainRequest,
    compute: Compute,
    saved: Option<SecretValue>,
) -> Result<TrainRequest, TrainError> {
    if !allows(request.subject, compute) {
        return Err(TrainError::Invalid(REMOTE_SUBJECT_UNAVAILABLE));
    }
    let placed = request.with_compute(compute);
    Ok(match key(compute, saved)? {
        Some(key) => placed.with_compute_key(key),
        None => placed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::comfy::settings;
    use abnegate_comfy::Config;
    use abnegate_comfy::lora::TrainImage;
    use abnegate_comfy::lora::validate_request;
    use std::fs;

    fn harness() -> (tempfile::TempDir, Config) {
        let models = tempfile::tempdir().expect("models directory");
        let mut config = settings::development();
        config.enabled = true;
        config.models_directory = models.path().to_path_buf();
        (models, config)
    }

    fn install(config: &Config, file: &str) {
        let path = config.models_directory.join(file);
        fs::create_dir_all(path.parent().expect("model folder")).expect("model folder");
        fs::write(path, b"weights").expect("weights");
    }

    fn request(base: &str, subject: TrainSubject, method: TrainMethod) -> TrainRequest {
        TrainRequest::new(
            "jerry",
            base,
            vec![TrainImage::from_bytes("one.png", "", b"image".to_vec())],
        )
        .with_trigger("ohwx")
        .with_subject(subject)
        .with_method(method)
    }

    fn refusal(config: &Config, request: &TrainRequest) -> &'static str {
        match validate_request(config, request) {
            Err(TrainError::Invalid(message)) => message,
            other => panic!("expected an invalid request, got {other:?}"),
        }
    }

    #[test]
    fn zones_rules_are_ones_the_crate_accepts() {
        let mut config = Config::default();
        config.train_policy = train_policy();
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn the_people_base_trains_every_method_and_only_a_lora_for_anything_else() {
        let policy = train_policy();
        let person = |method| {
            policy
                .rule(PEOPLE_BASE, method)
                .is_some_and(|rule| rule.subjects.contains(&TrainSubject::Person))
        };
        for method in [
            TrainMethod::Lora,
            TrainMethod::Pivotal,
            TrainMethod::Finetune,
            TrainMethod::Video,
        ] {
            assert!(person(method), "a person cannot train {method:?}");
        }
        let other: Vec<TrainMethod> = policy
            .rules_for(PEOPLE_BASE)
            .filter(|rule| rule.subjects.contains(&TrainSubject::Other))
            .map(|rule| rule.method)
            .collect();
        assert_eq!(other, [TrainMethod::Lora]);
        assert_eq!(policy.rules_for("flux-schnell").count(), 0);
    }

    #[test]
    fn each_method_publishes_where_its_recipe_loads_it() {
        let policy = train_policy();
        let rule = |method| policy.rule(PEOPLE_BASE, method).expect("a rule");
        assert_eq!(rule(TrainMethod::Finetune).directory, "checkpoints");
        assert_eq!(rule(TrainMethod::Finetune).recipe_id, "sdxl");
        for method in [TrainMethod::Lora, TrainMethod::Pivotal] {
            assert_eq!(rule(method).directory, "loras");
            assert_eq!(rule(method).recipe_id, "sdxl-adapter");
        }
        assert_eq!(rule(TrainMethod::Video).directory, "loras");
        assert_eq!(rule(TrainMethod::Video).recipe_id, "wan-adapter");
        assert_eq!(rule(TrainMethod::Video).suffix.as_deref(), Some("-wan"));
    }

    #[test]
    fn person_training_rejects_a_flux_base_and_fine_tune_on_other() {
        let (_models, config) = harness();
        assert_eq!(
            refusal(
                &config,
                &request("flux-schnell", TrainSubject::Person, TrainMethod::Lora)
            ),
            "this base does not train that subject"
        );
        for method in [
            TrainMethod::Finetune,
            TrainMethod::Pivotal,
            TrainMethod::Video,
        ] {
            assert_eq!(
                refusal(
                    &config,
                    &request("flux-schnell", TrainSubject::Other, method)
                ),
                "this base does not train with that method",
                "{method:?} on flux"
            );
            assert_eq!(
                refusal(&config, &request(PEOPLE_BASE, TrainSubject::Other, method)),
                "this base does not train that subject",
                "{method:?} for anything but a person"
            );
        }
    }

    #[test]
    fn a_person_trains_once_the_people_bundle_is_installed() {
        let (_models, config) = harness();
        let mut person = request(PEOPLE_BASE, TrainSubject::Person, TrainMethod::Lora);
        assert_eq!(
            refusal(&config, &person),
            "install the files this base needs before training on it"
        );
        install(&config, PEOPLE_CHECKPOINT);
        validate_request(&config, &person).expect("the people base on disk is enough to start");
        validate_request(
            &config,
            &request(PEOPLE_BASE, TrainSubject::Other, TrainMethod::Lora),
        )
        .expect("an object or style LoRA trains on the people base");
        person.trigger = None;
        assert_eq!(
            refusal(&config, &person),
            "trigger word is required so the weight can retain identity"
        );
        for method in [TrainMethod::Pivotal, TrainMethod::Finetune] {
            validate_request(&config, &request(PEOPLE_BASE, TrainSubject::Person, method))
                .unwrap_or_else(|error| panic!("{method:?} uses the people base: {error}"));
        }
    }

    #[test]
    fn a_person_video_adapter_needs_the_video_bundle_and_a_clip() {
        let (_models, config) = harness();
        install(&config, PEOPLE_CHECKPOINT);
        let mut video = request(PEOPLE_BASE, TrainSubject::Person, TrainMethod::Video);
        assert_eq!(
            refusal(&config, &video),
            "install the files this base needs before training on it"
        );
        install(&config, VIDEO_MODEL);
        assert_eq!(refusal(&config, &video), "video training needs clips");
        video
            .images
            .push(TrainImage::from_bytes("walk.mp4", "", b"clip".to_vec()));
        validate_request(&config, &video)
            .expect("the video bundle and a clip on disk are enough to start");
    }

    #[test]
    fn runpod_trains_every_image_subject() {
        for subject in [TrainSubject::Person, TrainSubject::Other] {
            assert!(allows(subject, Compute::Runpod), "{subject:?}");
            assert!(allows(subject, Compute::Local), "{subject:?}");
        }
    }

    #[test]
    fn runpod_without_a_key_is_refused() {
        for saved in [
            None,
            Some(SecretValue::new("")),
            Some(SecretValue::new(" \n")),
        ] {
            let refused = place(
                request(PEOPLE_BASE, TrainSubject::Person, TrainMethod::Lora),
                Compute::Runpod,
                saved,
            );
            assert!(
                matches!(refused, Err(TrainError::Invalid(MISSING_RUNPOD_KEY))),
                "{refused:?}"
            );
        }
        assert_eq!(
            MISSING_RUNPOD_KEY,
            "Save a Runpod API key in Workspace Settings."
        );
    }

    #[test]
    fn runpod_with_a_key_carries_it_trimmed() {
        for subject in [TrainSubject::Person, TrainSubject::Other] {
            let placed = place(
                request(PEOPLE_BASE, subject, TrainMethod::Lora),
                Compute::Runpod,
                Some(SecretValue::new(" rp-secret \n")),
            )
            .expect("a saved key places the run on Runpod");
            assert_eq!(placed.compute, Compute::Runpod);
            assert_eq!(
                placed.compute_key.as_ref().map(SecretValue::expose),
                Some("rp-secret")
            );
        }
    }

    #[test]
    fn local_training_never_carries_the_saved_key() {
        let placed = place(
            request(PEOPLE_BASE, TrainSubject::Person, TrainMethod::Lora),
            Compute::Local,
            Some(SecretValue::new("rp-secret")),
        )
        .expect("this machine needs no key");
        assert_eq!(placed.compute, Compute::Local);
        assert!(placed.compute_key.is_none());
        assert_eq!(
            key(Compute::Local, None).map(|key| key.is_none()).ok(),
            Some(true)
        );
    }

    #[test]
    fn runpod_person_does_not_need_the_local_people_checkpoint() {
        let (_models, config) = harness();
        let person = place(
            request(PEOPLE_BASE, TrainSubject::Person, TrainMethod::Finetune),
            Compute::Runpod,
            Some(SecretValue::new("rp-secret")),
        )
        .expect("placed on Runpod");
        assert!(!config.models_directory.join(PEOPLE_CHECKPOINT).is_file());
        validate_request(&config, &person).expect("Runpod brings its own base");
    }

    #[test]
    fn runpod_other_trains_on_the_host_with_a_key() {
        let (_models, config) = harness();
        let other = place(
            request("flux-schnell", TrainSubject::Other, TrainMethod::Lora),
            Compute::Runpod,
            Some(SecretValue::new("rp-secret")),
        )
        .expect("placed on Runpod");
        validate_request(&config, &other).expect("the host trainer takes a Runpod LoRA");
    }
}
