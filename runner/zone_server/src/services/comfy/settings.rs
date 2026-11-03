use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use abnegate_comfy::Config;
use abnegate_comfy::ConfigError;
use abnegate_comfy::host::Trainer;

use super::contract::contract;
use super::policy::train_policy;

/// Where zone has always read the U2-Net weights path from.
pub const VISION_MODEL_VARIABLE: &str = "ZONE_VISION_MODEL";
/// The instance vision model, which captions training images when
/// `COMFYUI_CAPTION_MODEL` names none.
pub const CAPTION_MODEL_FALLBACK_VARIABLE: &str = "OLLAMA_MODEL_VISION";
pub const BASE_URL: &str = "http://comfyui:8188";
pub const TOKEN_HEADER: &str = "X-Zone-ComfyUI-Token";
pub const CHECKPOINT: &str = "lustifySDXLNSFW_ggwpV7.safetensors";
pub const ARTIFACT_ROOT: &str = "/app/artifacts";
pub const MODELS_DIRECTORY: &str = "/app/comfyui/models";
/// The `comfyui/` tree the server image ships: graphs, recipes and poses.
pub const CONTAINER_DIRECTORY: &str = "/app/comfyui";
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
pub const CAPTION_TIMEOUT: Duration = Duration::from_secs(60);
pub const IMAGE_WORKFLOW: &str = "sdxl-api.json";
pub const VIDEO_WORKFLOW: &str = "wan2.2-a14b-t2v-api.json";
pub const AUDIO_WORKFLOW: &str = "ace-step-v1-3.5b-api.json";
pub const UPSCALE_WORKFLOW: &str = "upscale-image-api.json";

const BASE_URL_VARIABLE: &str = "COMFYUI_BASE_URL";
const TOKEN_HEADER_VARIABLE: &str = "COMFYUI_TOKEN_HEADER";
const WORKFLOW_PATH_VARIABLE: &str = "COMFYUI_WORKFLOW_PATH";
const CHECKPOINT_VARIABLE: &str = "COMFYUI_CHECKPOINT";
const VIDEO_WORKFLOW_PATH_VARIABLE: &str = "COMFYUI_VIDEO_WORKFLOW_PATH";
const AUDIO_WORKFLOW_PATH_VARIABLE: &str = "COMFYUI_AUDIO_WORKFLOW_PATH";
const UPSCALE_WORKFLOW_PATH_VARIABLE: &str = "COMFYUI_UPSCALE_WORKFLOW_PATH";
const ARTIFACT_ROOT_VARIABLE: &str = "COMFYUI_ARTIFACT_ROOT";
const MODELS_DIRECTORY_VARIABLE: &str = "COMFYUI_MODELS_DIRECTORY";
const REQUEST_TIMEOUT_VARIABLE: &str = "COMFYUI_REQUEST_TIMEOUT_SECONDS";
const CAPTION_MODEL_VARIABLE: &str = "COMFYUI_CAPTION_MODEL";
const CAPTION_TIMEOUT_VARIABLE: &str = "COMFYUI_CAPTION_TIMEOUT_SECONDS";
const POSES_DIRECTORY_VARIABLE: &str = "COMFYUI_POSES_DIRECTORY";
const WORKFLOWS: &str = "workflows";
const POSES: &str = "poses";
const VISION_WEIGHTS: &str = "vision/u2net.onnx";

/// Zone's ComfyUI settings from this process's environment: the crate's
/// `COMFYUI_*` names, with zone's defaults where a setting is unset or the
/// crate could not read it, and zone's contract, training rules and host
/// trainer. A setting that cannot work, such as a `COMFYUI_TOKEN_HEADER`
/// that is no header name, is refused here so the server stops at startup
/// instead of on first use.
pub fn load() -> Result<Config, ConfigError> {
    validated(configure(
        Config::from_environment_with_vision_model(VISION_MODEL_VARIABLE),
        |name| env::var(name).ok(),
        Path::new(CONTAINER_DIRECTORY),
    ))
}

/// Zone's defaults with no environment at all, reading the graphs, recipes
/// and poses from this checkout's `comfyui/` rather than the container's.
pub fn development() -> Config {
    configure(Config::default(), |_| None, &development_directory())
}

/// This checkout's `comfyui/` tree.
pub fn development_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../comfyui")
}

fn validated(config: Config) -> Result<Config, ConfigError> {
    config.validate()?;
    Ok(config)
}

fn configure(
    mut config: Config,
    environment: impl Fn(&str) -> Option<String>,
    directory: &Path,
) -> Config {
    let unset = |name: &str| text(environment(name)).is_none();
    let workflows = directory.join(WORKFLOWS);
    if url(environment(BASE_URL_VARIABLE)).is_none() {
        config.base_url = BASE_URL.to_string();
    }
    if unset(TOKEN_HEADER_VARIABLE) {
        config.token_header = TOKEN_HEADER.to_string();
    }
    if unset(WORKFLOW_PATH_VARIABLE) {
        config.workflow_path = Some(workflows.join(IMAGE_WORKFLOW));
    }
    if unset(CHECKPOINT_VARIABLE) {
        config.checkpoint = CHECKPOINT.to_string();
    }
    if unset(VIDEO_WORKFLOW_PATH_VARIABLE) {
        config.video_workflow_path = Some(workflows.join(VIDEO_WORKFLOW));
    }
    if unset(AUDIO_WORKFLOW_PATH_VARIABLE) {
        config.audio_workflow_path = Some(workflows.join(AUDIO_WORKFLOW));
    }
    if unset(UPSCALE_WORKFLOW_PATH_VARIABLE) {
        config.upscale_workflow_path = Some(workflows.join(UPSCALE_WORKFLOW));
    }
    if unset(ARTIFACT_ROOT_VARIABLE) {
        config.artifact_root = PathBuf::from(ARTIFACT_ROOT);
    }
    if unset(MODELS_DIRECTORY_VARIABLE) {
        config.models_directory = PathBuf::from(MODELS_DIRECTORY);
    }
    if whole_seconds(environment(REQUEST_TIMEOUT_VARIABLE)).is_none() {
        config.request_timeout = REQUEST_TIMEOUT;
    }
    if whole_seconds(environment(CAPTION_TIMEOUT_VARIABLE)).is_none() {
        config = config.with_caption_timeout(CAPTION_TIMEOUT);
    }
    if unset(VISION_MODEL_VARIABLE) {
        config.vision_model =
            Some(config.models_directory.join(VISION_WEIGHTS)).filter(|weights| weights.is_file());
    }
    if unset(CAPTION_MODEL_VARIABLE) {
        config.caption_model =
            text(environment(CAPTION_MODEL_FALLBACK_VARIABLE)).unwrap_or_default();
    }
    if unset(POSES_DIRECTORY_VARIABLE) {
        config.poses_directory = config
            .workflow_path
            .as_deref()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .map(|root| root.join(POSES));
    }
    config.contract = contract();
    config.train_policy = train_policy();
    config.with_host_trainer(Trainer::new())
}

fn text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn url(value: Option<String>) -> Option<String> {
    text(value.map(|url| url.trim().trim_end_matches('/').to_string()))
}

fn whole_seconds(value: Option<String>) -> Option<u64> {
    value.and_then(|value| value.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use abnegate_exec::EnvironmentPolicy;
    use std::collections::HashMap;
    use zone_core::variables::Variables;

    fn configured(base: Config, variables: &[(&str, &str)]) -> Config {
        let variables: HashMap<&str, &str> = variables.iter().copied().collect();
        configure(
            base,
            |name| variables.get(name).map(|value| (*value).to_string()),
            Path::new(CONTAINER_DIRECTORY),
        )
    }

    fn container() -> Config {
        configured(Config::default(), &[])
    }

    #[test]
    fn the_container_defaults_are_zones_and_not_the_crates() {
        let config = container();
        assert_eq!(config.base_url, "http://comfyui:8188");
        assert_eq!(config.token_header, "X-Zone-ComfyUI-Token");
        assert_eq!(config.checkpoint, "lustifySDXLNSFW_ggwpV7.safetensors");
        assert_eq!(config.artifact_root, PathBuf::from("/app/artifacts"));
        assert_eq!(
            config.models_directory,
            PathBuf::from("/app/comfyui/models")
        );
        assert_eq!(config.request_timeout, Duration::from_secs(15));
        assert_eq!(config.caption_timeout, Duration::from_secs(60));
        let defaults = Config::default();
        for (ours, theirs) in [
            (&config.base_url, &defaults.base_url),
            (&config.token_header, &defaults.token_header),
            (&config.checkpoint, &defaults.checkpoint),
        ] {
            assert_ne!(ours, theirs, "zone's default is the crate's");
        }
    }

    #[test]
    fn the_container_reads_its_graphs_from_the_image() {
        let config = container();
        for (path, graph) in [
            (&config.workflow_path, "sdxl-api.json"),
            (&config.video_workflow_path, "wan2.2-a14b-t2v-api.json"),
            (&config.audio_workflow_path, "ace-step-v1-3.5b-api.json"),
            (&config.upscale_workflow_path, "upscale-image-api.json"),
        ] {
            assert_eq!(
                path,
                &Some(PathBuf::from("/app/comfyui/workflows").join(graph))
            );
        }
        assert_eq!(
            config.poses_directory,
            Some(PathBuf::from("/app/comfyui/poses"))
        );
    }

    #[test]
    fn the_development_defaults_resolve_this_checkouts_graphs_and_poses() {
        let config = development();
        for path in [
            &config.workflow_path,
            &config.video_workflow_path,
            &config.audio_workflow_path,
            &config.upscale_workflow_path,
        ] {
            let path = path.as_deref().expect("a development graph");
            assert!(path.is_file(), "{} is not a graph", path.display());
        }
        assert!(
            config
                .workflow_path
                .as_deref()
                .is_some_and(|path| path.ends_with("comfyui/workflows/sdxl-api.json"))
        );
        let poses = config
            .poses_directory
            .as_deref()
            .expect("a poses directory");
        assert!(poses.ends_with("comfyui/poses"), "{}", poses.display());
        assert!(poses.join("standing.png").is_file());
        assert_eq!(config.checkpoint, CHECKPOINT);
        assert_eq!(config.base_url, BASE_URL);
        assert!(!config.enabled);
        assert_eq!(config.vision_model, None);
        assert_eq!(config.caption_model, "");
        assert_eq!(validated(config).map(|_| ()), Ok(()));
    }

    #[test]
    fn video_generates_on_the_wan_a14b_expert_pair_by_default() {
        for config in [container(), development()] {
            assert!(
                config
                    .video_workflow_path
                    .as_deref()
                    .is_some_and(|path| path.ends_with("workflows/wan2.2-a14b-t2v-api.json")),
                "{:?}",
                config.video_workflow_path
            );
            assert_eq!(
                config.video_unet,
                "wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors"
            );
            assert_eq!(config.video_vae, "wan_2.1_vae.safetensors");
            assert_eq!(config.video_generation_timeout, Duration::from_secs(7200));
        }
    }

    #[test]
    fn every_setting_carries_zones_contract_rules_and_host_trainer() {
        for config in [container(), development()] {
            assert_eq!(config.contract, contract());
            assert_eq!(config.train_policy, train_policy());
            assert_eq!(config.host_trainer, Some(Trainer::new()));
            assert_eq!(
                config.trainer_environment,
                EnvironmentPolicy::allowlist(),
                "a training command sees the allowlist and nothing else of the server's environment"
            );
        }
    }

    #[test]
    fn a_setting_the_environment_names_beats_zones_default() {
        let mut base = Config::default();
        base.base_url = "http://elsewhere:8188".into();
        base.token_header = "X-Proxy-Token".into();
        base.workflow_path = Some("/srv/workflows/custom.json".into());
        base.checkpoint = "custom.safetensors".into();
        base.video_workflow_path = Some("/srv/workflows/video.json".into());
        base.audio_workflow_path = Some("/srv/workflows/audio.json".into());
        base.upscale_workflow_path = Some("/srv/workflows/upscale.json".into());
        base.artifact_root = "/srv/artifacts".into();
        base.models_directory = "/srv/models".into();
        base.request_timeout = Duration::from_secs(90);
        base.caption_timeout = Duration::from_secs(1200);
        base.vision_model = Some("/srv/u2net.onnx".into());
        base.caption_model = "llava:13b".into();
        base.poses_directory = Some("/srv/poses".into());
        let set = [
            BASE_URL_VARIABLE,
            TOKEN_HEADER_VARIABLE,
            WORKFLOW_PATH_VARIABLE,
            CHECKPOINT_VARIABLE,
            VIDEO_WORKFLOW_PATH_VARIABLE,
            AUDIO_WORKFLOW_PATH_VARIABLE,
            UPSCALE_WORKFLOW_PATH_VARIABLE,
            ARTIFACT_ROOT_VARIABLE,
            MODELS_DIRECTORY_VARIABLE,
            REQUEST_TIMEOUT_VARIABLE,
            CAPTION_TIMEOUT_VARIABLE,
            VISION_MODEL_VARIABLE,
            CAPTION_MODEL_VARIABLE,
            POSES_DIRECTORY_VARIABLE,
            CAPTION_MODEL_FALLBACK_VARIABLE,
        ]
        .map(|name| match name {
            REQUEST_TIMEOUT_VARIABLE => (name, "90"),
            CAPTION_TIMEOUT_VARIABLE => (name, "1200"),
            _ => (name, "set"),
        });
        let config = configured(base.clone(), &set);
        assert_eq!(config.base_url, base.base_url);
        assert_eq!(config.token_header, base.token_header);
        assert_eq!(config.workflow_path, base.workflow_path);
        assert_eq!(config.checkpoint, base.checkpoint);
        assert_eq!(config.video_workflow_path, base.video_workflow_path);
        assert_eq!(config.audio_workflow_path, base.audio_workflow_path);
        assert_eq!(config.upscale_workflow_path, base.upscale_workflow_path);
        assert_eq!(config.artifact_root, base.artifact_root);
        assert_eq!(config.models_directory, base.models_directory);
        assert_eq!(config.request_timeout, base.request_timeout);
        assert_eq!(config.caption_timeout, base.caption_timeout);
        assert_eq!(config.vision_model, base.vision_model);
        assert_eq!(config.caption_model, base.caption_model);
        assert_eq!(config.poses_directory, base.poses_directory);
    }

    #[test]
    fn a_blank_setting_is_no_setting_and_gets_zones_default() {
        let mut base = Config::default();
        base.base_url = "http://127.0.0.1:8188".into();
        let config = configured(base, &[(BASE_URL_VARIABLE, "  ")]);
        assert_eq!(config.base_url, BASE_URL);
    }

    #[test]
    fn a_setting_the_crate_cannot_read_gets_zones_default_and_not_the_crates() {
        let crate_default = Config::default();
        let timeout = configured(crate_default.clone(), &[(REQUEST_TIMEOUT_VARIABLE, "abc")]);
        assert_eq!(timeout.request_timeout, REQUEST_TIMEOUT);
        for slashes in ["/", " // "] {
            let base = configured(crate_default.clone(), &[(BASE_URL_VARIABLE, slashes)]);
            assert_eq!(base.base_url, BASE_URL, "{slashes:?}");
        }
    }

    #[test]
    fn a_caption_waits_zones_caption_timeout_when_none_can_be_read() {
        let mut drifted = Config::default();
        drifted.caption_timeout = Duration::from_secs(1800);
        for variables in [&[][..], &[(CAPTION_TIMEOUT_VARIABLE, "abc")][..]] {
            let config = configured(drifted.clone(), variables);
            assert_eq!(config.caption_timeout, CAPTION_TIMEOUT, "{variables:?}");
        }
        assert_eq!(development().caption_timeout, CAPTION_TIMEOUT);
    }

    #[test]
    fn a_caption_timeout_the_environment_names_is_kept_up_to_an_hour() {
        for (seconds, expected) in [("1200", 1200), ("3600", 3600), ("7200", 3600)] {
            let mut variables = Variables::isolated(&[CAPTION_TIMEOUT_VARIABLE]);
            variables.set(CAPTION_TIMEOUT_VARIABLE, seconds);
            let config = load().expect("zone's settings");
            assert_eq!(
                config.caption_timeout,
                Duration::from_secs(expected),
                "{CAPTION_TIMEOUT_VARIABLE}={seconds}"
            );
        }
    }

    #[test]
    fn captioning_falls_back_to_the_instance_vision_model() {
        let fallback = configured(
            Config::default(),
            &[(CAPTION_MODEL_FALLBACK_VARIABLE, " llava:7b ")],
        );
        assert_eq!(fallback.caption_model, "llava:7b");

        let blank = configured(
            Config::default(),
            &[
                (CAPTION_MODEL_VARIABLE, "  "),
                (CAPTION_MODEL_FALLBACK_VARIABLE, "llava:7b"),
            ],
        );
        assert_eq!(blank.caption_model, "llava:7b");

        let mut overridden = Config::default();
        overridden.caption_model = "clip-override".into();
        let overridden = configured(
            overridden,
            &[
                (CAPTION_MODEL_VARIABLE, "clip-override"),
                (CAPTION_MODEL_FALLBACK_VARIABLE, "llava:7b"),
            ],
        );
        assert_eq!(overridden.caption_model, "clip-override");

        assert_eq!(container().caption_model, "");
    }

    #[test]
    fn the_subject_weights_are_looked_for_under_zones_models_directory() {
        assert_eq!(container().vision_model, None);

        let models = tempfile::tempdir().expect("models directory");
        let mut base = Config::default();
        base.models_directory = models.path().to_path_buf();
        let directory = models.path().display().to_string();
        let variables = [(MODELS_DIRECTORY_VARIABLE, directory.as_str())];
        assert_eq!(
            configured(base.clone(), &variables).vision_model,
            None,
            "a path that is not a file would fail at load"
        );
        let weights = models.path().join("vision/u2net.onnx");
        std::fs::create_dir_all(weights.parent().expect("vision directory"))
            .expect("vision directory");
        std::fs::write(&weights, b"onnx").expect("weights");
        assert_eq!(configured(base, &variables).vision_model, Some(weights));
    }

    #[test]
    fn the_poses_sit_beside_whichever_workflow_directory_is_configured() {
        let mut base = Config::default();
        base.workflow_path = Some("/srv/comfyui/workflows/custom.json".into());
        let config = configured(base, &[(WORKFLOW_PATH_VARIABLE, "set")]);
        assert_eq!(
            config.poses_directory,
            Some(PathBuf::from("/srv/comfyui/poses"))
        );
    }

    #[test]
    fn a_token_header_that_is_no_header_name_stops_the_server() {
        let mut config = development();
        config.token_header = "not a header".into();
        let refused = validated(config).map(|_| ()).expect_err("refused");
        assert!(
            refused.message().contains("COMFYUI_TOKEN_HEADER"),
            "{refused}"
        );
    }
}
