//! Settings read from `COMFYUI_*` environment variables.

use std::env;

use reqwest::header::HeaderName;

const OUTAGE_WINDOW_SECS: u64 = 120;
const QUEUE_CHECK_INTERVAL_SECS: u64 = 30;

use crate::client::Error;

/// The header a token-protected ComfyUI is sent its token in, unless
/// `COMFYUI_TOKEN_HEADER` names another.
pub const DEFAULT_TOKEN_HEADER: &str = "x-zone-comfyui-token";

/// Reading the value is the operating system's job; deciding what it means is
/// this crate's, so the two are separable and only one of them needs a process
/// to test.
fn truthy(value: Option<String>, default: bool) -> bool {
    match value {
        Some(value) => matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        None => default,
    }
}

/// A setting that is present but blank is not a setting.
fn text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// `COMFYUI_CAPTION_MODEL` is an override. The instance vision model already
/// accepts images, so captioning uses that when the override is blank.
fn resolve_caption_model(caption: Option<String>, vision: Option<String>) -> String {
    text(caption).or_else(|| text(vision)).unwrap_or_default()
}

fn bounded(value: Option<String>, default: u64, min: u64, max: u64) -> u64 {
    value
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
        .clamp(min, max)
}

/// Direct image generation settings loaded from `COMFYUI_*` environment variables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub base_url: String,
    pub api_token: Option<String>,
    /// The header [`Config::api_token`] is sent in.
    pub token_header: HeaderName,
    pub workflow_path: std::path::PathBuf,
    pub checkpoint: String,
    pub video_workflow_path: std::path::PathBuf,
    pub video_unet: String,
    pub video_clip: String,
    pub video_vae: String,
    pub audio_workflow_path: std::path::PathBuf,
    pub audio_checkpoint: String,
    pub upscale_workflow_path: std::path::PathBuf,
    pub upscale_model: String,
    pub artifact_root: std::path::PathBuf,
    pub classifier_model: String,
    pub classifier_timeout_secs: u64,
    /// Vision model that captions LoRA training images. Empty disables captioning.
    pub caption_model: String,
    pub caption_timeout_secs: u64,
    pub request_timeout_secs: u64,
    pub generation_timeout_secs: u64,
    pub video_generation_timeout_secs: u64,
    pub audio_generation_timeout_secs: u64,
    pub upscale_generation_timeout_secs: u64,
    pub poll_interval_ms: u64,
    /// How long a job keeps retrying ComfyUI requests that fail without a
    /// lasting status before it gives up on a dead or restarted server.
    pub outage_window_secs: u64,
    /// How often a job whose prompt has no history yet confirms ComfyUI still
    /// queues it, so a prompt lost to a restart fails instead of waiting out
    /// the generation deadline.
    pub queue_check_interval_secs: u64,
    /// ComfyUI models root (`checkpoints/`, `loras/`, `diffusion_models/`, ...).
    pub models_dir: std::path::PathBuf,
    /// Optional command used to train a LoRA. Empty uses ComfyUI ZoneTrainLoRA.
    pub train_command: Option<String>,
    /// Wall clock budget for a ComfyUI train job.
    pub train_timeout_secs: u64,
    /// Decoder that turns a submitted clip into training frames.
    pub ffmpeg: String,
    /// Reads a clip's duration, so a long one lowers its sampling rate instead
    /// of being cut short.
    pub ffprobe: String,
    /// Frames kept per second of submitted video.
    pub frame_fps: u32,
    /// Frames one clip contributes to a training set.
    pub frame_limit: u32,
    /// U2-Net weights that locate the subject of a training image. `None`
    /// crops photos on their centre and video frames on whatever moved.
    pub vision_model: Option<std::path::PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: "http://comfyui:8188".to_string(),
            api_token: None,
            token_header: HeaderName::from_static(DEFAULT_TOKEN_HEADER),
            workflow_path: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../comfyui/workflows/sdxl-api.json"),
            checkpoint: "lustifySDXLNSFW_ggwpV7.safetensors".to_string(),
            video_workflow_path: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../comfyui/workflows/wan2.2-a14b-t2v-api.json"),
            video_unet: "wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors".to_string(),
            video_clip: "umt5_xxl_fp8_e4m3fn_scaled.safetensors".to_string(),
            video_vae: "wan_2.1_vae.safetensors".to_string(),
            audio_workflow_path: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../comfyui/workflows/ace-step-v1-3.5b-api.json"),
            audio_checkpoint: "ace_step_v1_3.5b.safetensors".to_string(),
            upscale_workflow_path: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../comfyui/workflows/upscale-image-api.json"),
            upscale_model: "RealESRGAN_x4plus.safetensors".to_string(),
            artifact_root: "/app/artifacts".into(),
            classifier_model: "auto".to_string(),
            classifier_timeout_secs: 3,
            caption_model: String::new(),
            caption_timeout_secs: 60,
            request_timeout_secs: 15,
            generation_timeout_secs: 300,
            video_generation_timeout_secs: 7200,
            audio_generation_timeout_secs: 600,
            upscale_generation_timeout_secs: 600,
            poll_interval_ms: 500,
            outage_window_secs: OUTAGE_WINDOW_SECS,
            queue_check_interval_secs: QUEUE_CHECK_INTERVAL_SECS,
            models_dir: std::path::PathBuf::from("/app/comfyui/models"),
            train_command: None,
            train_timeout_secs: 3600,
            ffmpeg: "ffmpeg".to_string(),
            ffprobe: "ffprobe".to_string(),
            frame_fps: 4,
            frame_limit: 48,
            vision_model: None,
        }
    }
}

impl Config {
    /// Settings from this process's environment. A `COMFYUI_TOKEN_HEADER` that
    /// is not a header name is refused rather than replaced by the default,
    /// which a proxy checking the configured header would reject.
    pub fn from_env() -> Result<Self, Error> {
        Self::from_lookup(|name| env::var(name).ok())
    }

    fn from_lookup(environment: impl Fn(&str) -> Option<String>) -> Result<Self, Error> {
        let models_dir: std::path::PathBuf = environment("COMFYUI_MODELS_DIRECTORY")
            .unwrap_or_else(|| "/app/comfyui/models".to_string())
            .into();
        let token_header = match text(environment("COMFYUI_TOKEN_HEADER")) {
            Some(name) => HeaderName::try_from(name).map_err(|_| {
                Error::Configuration("COMFYUI_TOKEN_HEADER is not a valid header name")
            })?,
            None => HeaderName::from_static(DEFAULT_TOKEN_HEADER),
        };
        Ok(Self {
            enabled: truthy(environment("COMFYUI_ENABLED"), false),
            base_url: environment("COMFYUI_BASE_URL")
                .unwrap_or_else(|| "http://comfyui:8188".to_string())
                .trim_end_matches('/')
                .to_string(),
            api_token: environment("COMFYUI_API_TOKEN").filter(|token| !token.trim().is_empty()),
            token_header,
            workflow_path: environment("COMFYUI_WORKFLOW_PATH")
                .unwrap_or_else(|| "/app/comfyui/workflows/sdxl-api.json".to_string())
                .into(),
            checkpoint: environment("COMFYUI_CHECKPOINT")
                .unwrap_or_else(|| "lustifySDXLNSFW_ggwpV7.safetensors".to_string()),
            video_workflow_path: environment("COMFYUI_VIDEO_WORKFLOW_PATH")
                .unwrap_or_else(|| "/app/comfyui/workflows/wan2.2-a14b-t2v-api.json".to_string())
                .into(),
            video_unet: environment("COMFYUI_VIDEO_UNET")
                .unwrap_or_else(|| "wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors".to_string()),
            video_clip: environment("COMFYUI_VIDEO_CLIP")
                .unwrap_or_else(|| "umt5_xxl_fp8_e4m3fn_scaled.safetensors".to_string()),
            video_vae: environment("COMFYUI_VIDEO_VAE")
                .unwrap_or_else(|| "wan_2.1_vae.safetensors".to_string()),
            audio_workflow_path: environment("COMFYUI_AUDIO_WORKFLOW_PATH")
                .unwrap_or_else(|| "/app/comfyui/workflows/ace-step-v1-3.5b-api.json".to_string())
                .into(),
            audio_checkpoint: environment("COMFYUI_AUDIO_CHECKPOINT")
                .unwrap_or_else(|| "ace_step_v1_3.5b.safetensors".to_string()),
            upscale_workflow_path: environment("COMFYUI_UPSCALE_WORKFLOW_PATH")
                .unwrap_or_else(|| "/app/comfyui/workflows/upscale-image-api.json".to_string())
                .into(),
            upscale_model: environment("COMFYUI_UPSCALE_MODEL")
                .unwrap_or_else(|| "RealESRGAN_x4plus.safetensors".to_string()),
            artifact_root: environment("COMFYUI_ARTIFACT_ROOT")
                .unwrap_or_else(|| "/app/artifacts".to_string())
                .into(),
            classifier_model: text(environment("COMFYUI_CLASSIFIER_MODEL"))
                .unwrap_or_else(|| "auto".to_string()),
            classifier_timeout_secs: bounded(
                environment("COMFYUI_CLASSIFIER_TIMEOUT_SECONDS"),
                3,
                1,
                30,
            ),
            caption_model: resolve_caption_model(
                environment("COMFYUI_CAPTION_MODEL"),
                environment("OLLAMA_MODEL_VISION"),
            ),
            caption_timeout_secs: bounded(
                environment("COMFYUI_CAPTION_TIMEOUT_SECONDS"),
                60,
                5,
                600,
            ),
            request_timeout_secs: bounded(
                environment("COMFYUI_REQUEST_TIMEOUT_SECONDS"),
                15,
                1,
                120,
            ),
            generation_timeout_secs: bounded(
                environment("COMFYUI_GENERATION_TIMEOUT_SECONDS"),
                300,
                10,
                3600,
            ),
            video_generation_timeout_secs: bounded(
                environment("COMFYUI_VIDEO_GENERATION_TIMEOUT_SECONDS"),
                7200,
                10,
                14400,
            ),
            audio_generation_timeout_secs: bounded(
                environment("COMFYUI_AUDIO_GENERATION_TIMEOUT_SECONDS"),
                600,
                10,
                3600,
            ),
            upscale_generation_timeout_secs: bounded(
                environment("COMFYUI_UPSCALE_GENERATION_TIMEOUT_SECONDS"),
                600,
                10,
                3600,
            ),
            poll_interval_ms: bounded(
                environment("COMFYUI_POLL_INTERVAL_MILLISECONDS"),
                500,
                50,
                5000,
            ),
            outage_window_secs: OUTAGE_WINDOW_SECS,
            queue_check_interval_secs: QUEUE_CHECK_INTERVAL_SECS,
            vision_model: text(environment("ZONE_VISION_MODEL"))
                .map(std::path::PathBuf::from)
                .or_else(|| Some(models_dir.join("vision/u2net.onnx")))
                .filter(|path| path.is_file()),
            models_dir,
            train_command: text(environment("COMFYUI_TRAIN_COMMAND")),
            train_timeout_secs: bounded(
                environment("COMFYUI_TRAIN_TIMEOUT_SECONDS"),
                3600,
                60,
                14400,
            ),
            ffmpeg: text(environment("COMFYUI_FFMPEG")).unwrap_or_else(|| "ffmpeg".to_string()),
            ffprobe: text(environment("COMFYUI_FFPROBE")).unwrap_or_else(|| "ffprobe".to_string()),
            frame_fps: bounded(environment("COMFYUI_TRAIN_FRAME_RATE"), 4, 1, 30) as u32,
            frame_limit: bounded(environment("COMFYUI_TRAIN_FRAME_LIMIT"), 48, 1, 400) as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_defaults_cover_dev_and_container_paths() {
        let development = Config::default();
        assert!(
            development
                .workflow_path
                .ends_with("comfyui/workflows/sdxl-api.json"),
            "dev default must resolve the packaged people graph, got {:?}",
            development.workflow_path
        );
        assert_eq!(development.checkpoint, "lustifySDXLNSFW_ggwpV7.safetensors");

        let container = configured(&[]);
        assert_eq!(
            container.workflow_path,
            std::path::PathBuf::from("/app/comfyui/workflows/sdxl-api.json")
        );
        assert_eq!(container.checkpoint, "lustifySDXLNSFW_ggwpV7.safetensors");
    }

    #[test]
    fn video_defaults_cover_a14b() {
        let development = Config::default();
        assert!(
            development
                .video_workflow_path
                .ends_with("comfyui/workflows/wan2.2-a14b-t2v-api.json"),
            "dev default must resolve the packaged A14B graph, got {:?}",
            development.video_workflow_path
        );
        assert_eq!(
            development.video_unet,
            "wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors"
        );
        assert_eq!(development.video_vae, "wan_2.1_vae.safetensors");
        assert_eq!(development.video_generation_timeout_secs, 7200);

        let container = configured(&[]);
        assert_eq!(
            container.video_workflow_path,
            std::path::PathBuf::from("/app/comfyui/workflows/wan2.2-a14b-t2v-api.json")
        );
        assert_eq!(
            container.video_unet,
            "wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors"
        );
        assert_eq!(container.video_vae, "wan_2.1_vae.safetensors");
    }

    #[test]
    fn audio_defaults_cover_dev_and_container_paths() {
        let development = Config::default();
        assert!(
            development
                .audio_workflow_path
                .ends_with("comfyui/workflows/ace-step-v1-3.5b-api.json"),
            "dev default must resolve the packaged graph, got {:?}",
            development.audio_workflow_path
        );
        assert_eq!(development.audio_checkpoint, "ace_step_v1_3.5b.safetensors");
        assert_eq!(development.audio_generation_timeout_secs, 600);

        let container = configured(&[]);
        assert_eq!(
            container.audio_workflow_path,
            std::path::PathBuf::from("/app/comfyui/workflows/ace-step-v1-3.5b-api.json")
        );
    }

    fn configured(variables: &[(&str, &str)]) -> Config {
        Config::from_lookup(|name| {
            variables
                .iter()
                .find(|(variable, _)| *variable == name)
                .map(|(_, value)| value.to_string())
        })
        .expect("the settings load")
    }

    #[test]
    fn settings_are_read_under_their_full_names() {
        let config = configured(&[
            ("COMFYUI_CLASSIFIER_TIMEOUT_SECONDS", "7"),
            ("COMFYUI_CAPTION_TIMEOUT_SECONDS", "70"),
            ("COMFYUI_REQUEST_TIMEOUT_SECONDS", "90"),
            ("COMFYUI_GENERATION_TIMEOUT_SECONDS", "400"),
            ("COMFYUI_VIDEO_GENERATION_TIMEOUT_SECONDS", "700"),
            ("COMFYUI_AUDIO_GENERATION_TIMEOUT_SECONDS", "800"),
            ("COMFYUI_UPSCALE_GENERATION_TIMEOUT_SECONDS", "900"),
            ("COMFYUI_TRAIN_TIMEOUT_SECONDS", "7200"),
            ("COMFYUI_POLL_INTERVAL_MILLISECONDS", "250"),
            ("COMFYUI_MODELS_DIRECTORY", "/srv/comfyui/models"),
            ("COMFYUI_TRAIN_FRAME_RATE", "6"),
            ("COMFYUI_ARTIFACT_ROOT", "/srv/artifacts"),
            ("COMFYUI_TOKEN_HEADER", "X-Proxy-Token"),
        ]);
        assert_eq!(config.classifier_timeout_secs, 7);
        assert_eq!(config.caption_timeout_secs, 70);
        assert_eq!(config.request_timeout_secs, 90);
        assert_eq!(config.generation_timeout_secs, 400);
        assert_eq!(config.video_generation_timeout_secs, 700);
        assert_eq!(config.audio_generation_timeout_secs, 800);
        assert_eq!(config.upscale_generation_timeout_secs, 900);
        assert_eq!(config.train_timeout_secs, 7200);
        assert_eq!(config.poll_interval_ms, 250);
        assert_eq!(
            config.models_dir,
            std::path::PathBuf::from("/srv/comfyui/models")
        );
        assert_eq!(config.frame_fps, 6);
        assert_eq!(
            config.artifact_root,
            std::path::PathBuf::from("/srv/artifacts")
        );
        assert_eq!(config.token_header, "x-proxy-token");
    }

    #[test]
    fn the_abbreviated_names_are_no_longer_read() {
        let config = configured(&[
            ("COMFYUI_CLASSIFIER_TIMEOUT_SECS", "7"),
            ("COMFYUI_CAPTION_TIMEOUT_SECS", "70"),
            ("COMFYUI_REQUEST_TIMEOUT_SECS", "90"),
            ("COMFYUI_GENERATION_TIMEOUT_SECS", "400"),
            ("COMFYUI_VIDEO_GENERATION_TIMEOUT_SECS", "700"),
            ("COMFYUI_AUDIO_GENERATION_TIMEOUT_SECS", "800"),
            ("COMFYUI_UPSCALE_GENERATION_TIMEOUT_SECS", "900"),
            ("COMFYUI_TRAIN_TIMEOUT_SECS", "7200"),
            ("COMFYUI_POLL_INTERVAL_MS", "250"),
            ("COMFYUI_MODELS_DIR", "/srv/comfyui/models"),
            ("COMFYUI_TRAIN_FRAME_FPS", "6"),
            ("ARTIFACT_ROOT", "/srv/artifacts"),
        ]);
        let defaults = configured(&[]);
        assert_eq!(config, defaults, "an old name is ignored, not honoured");
        assert_eq!(defaults.classifier_timeout_secs, 3);
        assert_eq!(defaults.request_timeout_secs, 15);
        assert_eq!(defaults.poll_interval_ms, 500);
        assert_eq!(
            defaults.models_dir,
            std::path::PathBuf::from("/app/comfyui/models")
        );
        assert_eq!(defaults.frame_fps, 4);
        assert_eq!(
            defaults.artifact_root,
            std::path::PathBuf::from("/app/artifacts")
        );
    }

    #[test]
    fn the_token_header_defaults_to_zones_and_refuses_a_name_that_is_no_header() {
        assert_eq!(configured(&[]).token_header, DEFAULT_TOKEN_HEADER);
        assert_eq!(
            configured(&[("COMFYUI_TOKEN_HEADER", "  ")]).token_header,
            DEFAULT_TOKEN_HEADER,
            "a blank header is no setting at all"
        );
        let refused = Config::from_lookup(|name| {
            (name == "COMFYUI_TOKEN_HEADER").then(|| "not a header".to_string())
        });
        assert!(
            matches!(
                refused,
                Err(Error::Configuration(message)) if message.contains("COMFYUI_TOKEN_HEADER")
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn a_setting_that_is_set_beats_its_default_and_a_missing_one_does_not() {
        for value in ["1", "true", "TRUE", "yes", "on"] {
            assert!(
                truthy(Some(value.into()), false),
                "{value} should read true"
            );
        }
        for value in ["0", "false", "no", "off", "", "maybe"] {
            assert!(
                !truthy(Some(value.into()), true),
                "{value} should read false"
            );
        }
        assert!(truthy(None, true), "an unset setting keeps its default");
        assert!(!truthy(None, false));
    }

    #[test]
    fn a_setting_that_is_blank_is_no_setting_at_all() {
        assert_eq!(text(Some("  ffmpeg  ".into())), Some("ffmpeg".into()));
        assert_eq!(text(Some("   ".into())), None, "whitespace is not a value");
        assert_eq!(text(Some(String::new())), None);
        assert_eq!(text(None), None);
    }

    #[test]
    fn captioning_uses_the_instance_vision_model_when_no_caption_model_is_set() {
        assert_eq!(
            resolve_caption_model(Some("clip-override".into()), Some("llava:7b".into())),
            "clip-override"
        );
        assert_eq!(
            resolve_caption_model(Some("  ".into()), Some("llava:7b".into())),
            "llava:7b"
        );
        assert_eq!(
            resolve_caption_model(None, Some(" llava:7b ".into())),
            "llava:7b"
        );
        assert_eq!(resolve_caption_model(None, None), "");
    }

    #[test]
    fn a_setting_outside_its_range_is_clamped_rather_than_taken() {
        assert_eq!(bounded(Some("7".into()), 1, 0, 10), 7);
        assert_eq!(bounded(Some("99".into()), 1, 0, 10), 10, "over the ceiling");
        assert_eq!(bounded(Some("0".into()), 5, 2, 10), 2, "under the floor");
        assert_eq!(
            bounded(Some("not a number".into()), 5, 0, 10),
            5,
            "an unparseable setting is no setting at all"
        );
        assert_eq!(
            bounded(Some("-3".into()), 5, 0, 10),
            5,
            "so is a negative one"
        );
        assert_eq!(bounded(None, 5, 0, 10), 5);
    }

    #[test]
    fn frame_sampling_has_defaults_a_clip_can_be_trained_on() {
        let defaults = Config::default();
        assert_eq!(defaults.frame_fps, 4);
        assert_eq!(defaults.frame_limit, 48);
        assert_eq!(defaults.ffmpeg, "ffmpeg");
        assert_eq!(defaults.ffprobe, "ffprobe");
        assert_eq!(
            defaults.vision_model, None,
            "the weights are opt-in, so nothing is assumed present"
        );
    }

    #[test]
    fn subject_detection_stays_off_until_the_weights_are_actually_there() {
        let models = tempfile::tempdir().expect("models directory");
        let directory = models.path().display().to_string();
        let without = configured(&[("COMFYUI_MODELS_DIRECTORY", &directory)]);
        assert_eq!(
            without.vision_model, None,
            "a path that is not a file would fail at load and cost a request its crop"
        );

        let weights = models.path().join("vision/u2net.onnx");
        std::fs::create_dir_all(weights.parent().unwrap()).unwrap();
        std::fs::write(&weights, b"onnx").unwrap();
        let with = configured(&[("COMFYUI_MODELS_DIRECTORY", &directory)]);
        assert_eq!(with.vision_model, Some(weights));
    }
}
