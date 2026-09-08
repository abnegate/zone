//! Settings read from `COMFYUI_*` environment variables.

use std::env;

fn env_truthy(name: &str, default: bool) -> bool {
    match env::var(name) {
        Ok(s) => matches!(s.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => default,
    }
}

fn env_u64(name: &str, default: u64, min: u64, max: u64) -> u64 {
    env::var(name)
        .ok()
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
    pub workflow_path: std::path::PathBuf,
    pub checkpoint: String,
    pub video_workflow_path: std::path::PathBuf,
    pub video_unet: String,
    pub video_clip: String,
    pub video_vae: String,
    pub audio_workflow_path: std::path::PathBuf,
    pub audio_checkpoint: String,
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
    pub poll_interval_ms: u64,
    /// ComfyUI models root (`checkpoints/`, `loras/`, `diffusion_models/`, ...).
    pub models_dir: std::path::PathBuf,
    /// Optional command used to train a LoRA. Empty uses ComfyUI ZoneTrainLoRA.
    pub train_command: Option<String>,
    /// Wall clock budget for a ComfyUI train job.
    pub train_timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: "http://comfyui:8188".to_string(),
            api_token: None,
            workflow_path: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../comfyui/workflows/flux1-schnell-fp8-api.json"),
            checkpoint: "flux1-schnell-fp8.safetensors".to_string(),
            video_workflow_path: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../comfyui/workflows/wan2.2-ti2v-5b-api.json"),
            video_unet: "wan2.2_ti2v_5B_fp16.safetensors".to_string(),
            video_clip: "umt5_xxl_fp8_e4m3fn_scaled.safetensors".to_string(),
            video_vae: "wan2.2_vae.safetensors".to_string(),
            audio_workflow_path: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../comfyui/workflows/ace-step-v1-3.5b-api.json"),
            audio_checkpoint: "ace_step_v1_3.5b.safetensors".to_string(),
            artifact_root: "/app/artifacts".into(),
            classifier_model: "auto".to_string(),
            classifier_timeout_secs: 3,
            caption_model: String::new(),
            caption_timeout_secs: 60,
            request_timeout_secs: 15,
            generation_timeout_secs: 300,
            video_generation_timeout_secs: 600,
            audio_generation_timeout_secs: 600,
            poll_interval_ms: 500,
            models_dir: std::path::PathBuf::from("/app/comfyui/models"),
            train_command: None,
            train_timeout_secs: 3600,
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            enabled: env_truthy("COMFYUI_ENABLED", false),
            base_url: env::var("COMFYUI_BASE_URL")
                .unwrap_or_else(|_| "http://comfyui:8188".to_string())
                .trim_end_matches('/')
                .to_string(),
            api_token: env::var("COMFYUI_API_TOKEN")
                .ok()
                .filter(|token| !token.trim().is_empty()),
            workflow_path: env::var("COMFYUI_WORKFLOW_PATH")
                .unwrap_or_else(|_| "/app/comfyui/workflows/flux1-schnell-fp8-api.json".to_string())
                .into(),
            checkpoint: env::var("COMFYUI_CHECKPOINT")
                .unwrap_or_else(|_| "flux1-schnell-fp8.safetensors".to_string()),
            video_workflow_path: env::var("COMFYUI_VIDEO_WORKFLOW_PATH")
                .unwrap_or_else(|_| "/app/comfyui/workflows/wan2.2-ti2v-5b-api.json".to_string())
                .into(),
            video_unet: env::var("COMFYUI_VIDEO_UNET")
                .unwrap_or_else(|_| "wan2.2_ti2v_5B_fp16.safetensors".to_string()),
            video_clip: env::var("COMFYUI_VIDEO_CLIP")
                .unwrap_or_else(|_| "umt5_xxl_fp8_e4m3fn_scaled.safetensors".to_string()),
            video_vae: env::var("COMFYUI_VIDEO_VAE")
                .unwrap_or_else(|_| "wan2.2_vae.safetensors".to_string()),
            audio_workflow_path: env::var("COMFYUI_AUDIO_WORKFLOW_PATH")
                .unwrap_or_else(|_| "/app/comfyui/workflows/ace-step-v1-3.5b-api.json".to_string())
                .into(),
            audio_checkpoint: env::var("COMFYUI_AUDIO_CHECKPOINT")
                .unwrap_or_else(|_| "ace_step_v1_3.5b.safetensors".to_string()),
            artifact_root: env::var("ARTIFACT_ROOT")
                .unwrap_or_else(|_| "/app/artifacts".to_string())
                .into(),
            classifier_model: env::var("COMFYUI_CLASSIFIER_MODEL")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "auto".to_string()),
            classifier_timeout_secs: env_u64("COMFYUI_CLASSIFIER_TIMEOUT_SECS", 3, 1, 30),
            caption_model: env::var("COMFYUI_CAPTION_MODEL")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_default(),
            caption_timeout_secs: env_u64("COMFYUI_CAPTION_TIMEOUT_SECS", 60, 5, 600),
            request_timeout_secs: env_u64("COMFYUI_REQUEST_TIMEOUT_SECS", 15, 1, 120),
            generation_timeout_secs: env_u64("COMFYUI_GENERATION_TIMEOUT_SECS", 300, 10, 3600),
            video_generation_timeout_secs: env_u64(
                "COMFYUI_VIDEO_GENERATION_TIMEOUT_SECS",
                600,
                10,
                3600,
            ),
            audio_generation_timeout_secs: env_u64(
                "COMFYUI_AUDIO_GENERATION_TIMEOUT_SECS",
                600,
                10,
                3600,
            ),
            poll_interval_ms: env_u64("COMFYUI_POLL_INTERVAL_MS", 500, 50, 5000),
            models_dir: env::var("COMFYUI_MODELS_DIR")
                .unwrap_or_else(|_| "/app/comfyui/models".to_string())
                .into(),
            train_command: env::var("COMFYUI_TRAIN_COMMAND")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            train_timeout_secs: env_u64("COMFYUI_TRAIN_TIMEOUT_SECS", 3600, 60, 14400),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        if env::var_os("COMFYUI_AUDIO_WORKFLOW_PATH").is_none() {
            let container = Config::from_env();
            assert_eq!(
                container.audio_workflow_path,
                std::path::PathBuf::from("/app/comfyui/workflows/ace-step-v1-3.5b-api.json")
            );
        }
    }
}
