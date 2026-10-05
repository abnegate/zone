//! SDXL person trains run on the host, not inside ComfyUI or the manager
//! container. Job state lives on the models bind-mount so a manager recreate
//! does not cancel a week-long fine-tune.

use crate::lora::{TrainError, TrainProvider};
use crate::train::{TrainProgress, parse_progress};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

pub const ROOT: &str = ".zone-train";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostJob {
    pub schema_version: u32,
    pub id: Uuid,
    pub name: String,
    pub method: String,
    #[serde(default)]
    pub subject: String,
    pub trigger: String,
    pub checkpoint: String,
    #[serde(default)]
    pub provider: TrainProvider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod_id: Option<String>,
    pub status: HostStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default)]
    pub recipe_id: String,
    #[serde(default)]
    pub hf_base: String,
    #[serde(default)]
    pub image_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loss: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previews: Vec<String>,
    pub started_at: DateTime<Utc>,
}

impl HostJob {
    pub fn create(name: &str, method: &str, trigger: &str, checkpoint: &str) -> Self {
        Self {
            schema_version: 1,
            id: Uuid::new_v4(),
            name: name.to_string(),
            method: method.to_string(),
            subject: String::new(),
            trigger: trigger.to_string(),
            checkpoint: checkpoint.to_string(),
            provider: TrainProvider::Local,
            gpu: None,
            pod_id: None,
            status: HostStatus::Queued,
            filename: None,
            recipe_id: String::new(),
            hf_base: String::new(),
            image_count: 0,
            error: None,
            step: None,
            total: None,
            phase: None,
            message: None,
            percent: None,
            loss: None,
            eta_seconds: None,
            pid: None,
            previews: Vec::new(),
            started_at: Utc::now(),
        }
    }

    pub fn busy(&self) -> bool {
        matches!(self.status, HostStatus::Queued | HostStatus::Running)
    }
}

pub fn root(models_dir: &Path) -> PathBuf {
    models_dir.join(ROOT)
}

pub fn job_dir(models_dir: &Path, id: Uuid) -> PathBuf {
    root(models_dir).join(id.to_string())
}

pub fn preview_basename(name: &str) -> Option<&str> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return None;
    }
    Some(name)
}

pub fn preview_file(models_dir: &Path, name: &str) -> Option<PathBuf> {
    let name = preview_basename(name)?;
    let job = current(models_dir)?;
    let path = job_dir(models_dir, job.id).join("previews").join(name);
    path.is_file().then_some(path)
}

pub fn current(models_dir: &Path) -> Option<HostJob> {
    let root = root(models_dir);
    let entries = fs::read_dir(&root).ok()?;
    let mut latest: Option<HostJob> = None;
    for entry in entries.flatten() {
        let path = entry.path().join("job.json");
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(job) = serde_json::from_slice::<HostJob>(&bytes) else {
            continue;
        };
        if latest
            .as_ref()
            .is_none_or(|existing| job.started_at > existing.started_at)
        {
            latest = Some(job);
        }
    }
    latest
}

pub fn busy(models_dir: &Path) -> bool {
    current(models_dir).is_some_and(|job| job.busy())
}

/// Latest job with `progress.json` overlaid so GET /train can survive a
/// manager recreate.
pub fn current_with_progress(models_dir: &Path) -> Option<HostJob> {
    let mut job = current(models_dir)?;
    overlay_progress(&job_dir(models_dir, job.id), &mut job);
    Some(job)
}

pub fn steps_for(image_count: usize) -> u32 {
    (image_count as u32).saturating_mul(20).clamp(500, 8000)
}

fn overlay_progress(dir: &Path, job: &mut HostJob) {
    let Ok(bytes) = fs::read(dir.join("progress.json")) else {
        return;
    };
    let Some(update) = parse_progress(&bytes) else {
        return;
    };
    job.step = Some(update.step);
    job.total = Some(update.total);
    job.phase = update.phase;
    job.message = update.message;
    job.percent = update.percent;
    job.loss = update.loss;
    job.eta_seconds = update.eta_seconds;
    job.previews = update.previews;
}

pub fn write_progress(dir: &Path, update: &TrainProgress) -> Result<(), TrainError> {
    fs::create_dir_all(dir).map_err(|error| TrainError::Failed(error.to_string()))?;
    let mut payload = serde_json::Map::new();
    payload.insert("step".into(), serde_json::json!(update.step));
    payload.insert("total".into(), serde_json::json!(update.total));
    if let Some(phase) = &update.phase {
        payload.insert("phase".into(), serde_json::json!(phase));
    }
    if let Some(message) = &update.message {
        payload.insert("message".into(), serde_json::json!(message));
    }
    if let Some(percent) = update.percent {
        payload.insert("percent".into(), serde_json::json!(percent));
    }
    if let Some(loss) = update.loss {
        payload.insert("loss".into(), serde_json::json!(loss));
    }
    if let Some(eta_seconds) = update.eta_seconds {
        payload.insert("eta_seconds".into(), serde_json::json!(eta_seconds));
    }
    if !update.previews.is_empty() {
        payload.insert("previews".into(), serde_json::json!(update.previews));
    }
    let encoded = serde_json::to_vec(&serde_json::Value::Object(payload))
        .map_err(|error| TrainError::Failed(error.to_string()))?;
    let temporary = dir.join("progress.json.tmp");
    fs::write(&temporary, encoded).map_err(|error| TrainError::Failed(error.to_string()))?;
    fs::rename(temporary, dir.join("progress.json"))
        .map_err(|error| TrainError::Failed(error.to_string()))
}

pub fn write_queued_progress(dir: &Path, image_count: usize) -> Result<(), TrainError> {
    write_progress(
        dir,
        &TrainProgress::new(0, steps_for(image_count))
            .phase("queued", "Waiting for the host trainer")
            .percent(0),
    )
}

pub fn write_job(dir: &Path, job: &HostJob) -> Result<(), TrainError> {
    fs::create_dir_all(dir).map_err(|error| TrainError::Failed(error.to_string()))?;
    let encoded =
        serde_json::to_vec_pretty(job).map_err(|error| TrainError::Failed(error.to_string()))?;
    let temporary = dir.join("job.json.tmp");
    fs::write(&temporary, encoded).map_err(|error| TrainError::Failed(error.to_string()))?;
    fs::rename(temporary, dir.join("job.json"))
        .map_err(|error| TrainError::Failed(error.to_string()))
}

/// Write the Runpod API key beside the job. Mode 0o600, never job.json.
pub fn write_runpod_key(dir: &Path, key: &str) -> Result<(), TrainError> {
    fs::create_dir_all(dir).map_err(|error| TrainError::Failed(error.to_string()))?;
    let path = dir.join("runpod.key");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|error| TrainError::Failed(error.to_string()))?;
    file.write_all(key.trim().as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .map_err(|error| TrainError::Failed(error.to_string()))
}

pub fn read_job(dir: &Path) -> Result<HostJob, TrainError> {
    let bytes =
        fs::read(dir.join("job.json")).map_err(|error| TrainError::Failed(error.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|error| TrainError::Failed(error.to_string()))
}

pub fn video_steps_for(window_count: usize) -> u32 {
    (window_count as u32).saturating_mul(20).clamp(150, 2000)
}

pub fn write_queued_video_progress(dir: &Path, window_count: usize) -> Result<(), TrainError> {
    write_progress(
        dir,
        &TrainProgress::new(0, video_steps_for(window_count))
            .phase("queued", "Waiting for the host trainer")
            .percent(0),
    )
}

pub fn language_steps_for(count: usize) -> u32 {
    (count as u32).saturating_mul(1).clamp(200, 1000)
}

pub fn write_queued_language_progress(dir: &Path, count: usize) -> Result<(), TrainError> {
    write_progress(
        dir,
        &TrainProgress::new(0, language_steps_for(count))
            .phase("queued", "Waiting for the host trainer")
            .percent(0),
    )
}

/// Copy staged `data/` and `targets/` into the host job directories.
pub fn stage_language(attempt: &Path, job: &Path) -> Result<(), TrainError> {
    copy_flat(&attempt.join("data"), &job.join("data"))?;
    let targets = attempt.join("targets");
    if targets.is_dir() {
        copy_flat(&targets, &job.join("dataset"))?;
    }
    Ok(())
}

fn copy_flat(source: &Path, destination: &Path) -> Result<(), TrainError> {
    fs::create_dir_all(destination).map_err(|error| TrainError::Failed(error.to_string()))?;
    for entry in fs::read_dir(source).map_err(|error| TrainError::Failed(error.to_string()))? {
        let entry = entry.map_err(|error| TrainError::Failed(error.to_string()))?;
        let name = entry.file_name();
        fs::copy(entry.path(), destination.join(name))
            .map_err(|error| TrainError::Failed(error.to_string()))?;
    }
    Ok(())
}

pub fn stage_clips(attempt: &Path, job: &Path) -> Result<(), TrainError> {
    let source = crate::dataset::clips_dir(attempt);
    if !source.is_dir() {
        return Ok(());
    }
    let destination = crate::dataset::clips_dir(job);
    fs::create_dir_all(&destination).map_err(|error| TrainError::Failed(error.to_string()))?;
    for entry in fs::read_dir(&source).map_err(|error| TrainError::Failed(error.to_string()))? {
        let entry = entry.map_err(|error| TrainError::Failed(error.to_string()))?;
        let name = entry.file_name();
        fs::copy(entry.path(), destination.join(name))
            .map_err(|error| TrainError::Failed(error.to_string()))?;
    }
    Ok(())
}

/// Copy staged `targets/` into the host job dataset directory.
pub fn stage_dataset(attempt: &Path, job: &Path) -> Result<(), TrainError> {
    let source = attempt.join("targets");
    let destination = job.join("dataset");
    fs::create_dir_all(&destination).map_err(|error| TrainError::Failed(error.to_string()))?;
    for entry in fs::read_dir(&source).map_err(|error| TrainError::Failed(error.to_string()))? {
        let entry = entry.map_err(|error| TrainError::Failed(error.to_string()))?;
        let name = entry.file_name();
        fs::copy(entry.path(), destination.join(name))
            .map_err(|error| TrainError::Failed(error.to_string()))?;
    }
    Ok(())
}

const PICKUP: Duration = Duration::from_secs(120);

pub async fn wait(
    dir: &Path,
    progress: Option<mpsc::UnboundedSender<TrainProgress>>,
) -> Result<HostJob, TrainError> {
    let progress_path = dir.join("progress.json");
    let queued_at = std::time::Instant::now();
    let mut seen_running = false;
    loop {
        if let Ok(bytes) = fs::read(&progress_path)
            && let Some(update) = parse_progress(&bytes)
            && let Some(progress) = &progress
        {
            let _ = progress.send(update);
        }
        let job = read_job(dir)?;
        match job.status {
            HostStatus::Succeeded => return Ok(job),
            HostStatus::Failed => {
                return Err(TrainError::Failed(
                    job.error.unwrap_or_else(|| "host trainer failed".into()),
                ));
            }
            HostStatus::Running => {
                seen_running = true;
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            HostStatus::Queued if !seen_running && queued_at.elapsed() > PICKUP => {
                return Err(TrainError::Failed(
                    "host trainer did not pick up the job; run ./scripts/setup-comfyui-macos.sh --install-trainer".into(),
                ));
            }
            HostStatus::Queued => {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn current_picks_the_latest_job() {
        let root = tempfile::tempdir().unwrap();
        let models = root.path();
        let older = HostJob::create("first", "lora", "ohwx", "base.safetensors");
        let mut newer = HostJob::create("second", "finetune", "ohwx", "base.safetensors");
        newer.started_at = older.started_at + chrono::Duration::seconds(5);
        write_job(&job_dir(models, older.id), &older).unwrap();
        write_job(&job_dir(models, newer.id), &newer).unwrap();
        let current = current(models).unwrap();
        assert_eq!(current.name, "second");
        assert!(busy(models));
        newer.status = HostStatus::Succeeded;
        write_job(&job_dir(models, newer.id), &newer).unwrap();
        assert!(!busy(models));
    }

    #[test]
    fn stage_dataset_copies_png_and_caption() {
        let root = tempfile::tempdir().unwrap();
        let attempt = root.path().join("attempt");
        fs::create_dir_all(attempt.join("targets")).unwrap();
        fs::write(attempt.join("targets/0000.png"), b"png").unwrap();
        fs::write(attempt.join("targets/0000.txt"), b"ohwx person").unwrap();
        let job = root.path().join("job");
        stage_dataset(&attempt, &job).unwrap();
        assert_eq!(fs::read(job.join("dataset/0000.png")).unwrap(), b"png");
        assert_eq!(
            fs::read(job.join("dataset/0000.txt")).unwrap(),
            b"ohwx person"
        );
    }

    #[test]
    fn stage_dataset_copies_mask_kind_and_pose() {
        let root = tempfile::tempdir().unwrap();
        let attempt = root.path().join("attempt");
        fs::create_dir_all(attempt.join("targets")).unwrap();
        fs::write(attempt.join("targets/0000.png"), b"png").unwrap();
        fs::write(attempt.join("targets/0000.txt"), b"ohwx person").unwrap();
        fs::write(attempt.join("targets/0000.mask.png"), b"mask").unwrap();
        fs::write(attempt.join("targets/0000.kind"), b"body").unwrap();
        fs::write(attempt.join("targets/0000.pose"), b"standing").unwrap();
        fs::write(attempt.join("targets/rebalance.json"), b"{}").unwrap();
        let job = root.path().join("job");
        stage_dataset(&attempt, &job).unwrap();
        assert_eq!(
            fs::read(job.join("dataset/0000.mask.png")).unwrap(),
            b"mask"
        );
        assert_eq!(fs::read(job.join("dataset/0000.kind")).unwrap(), b"body");
        assert_eq!(
            fs::read(job.join("dataset/0000.pose")).unwrap(),
            b"standing"
        );
        assert_eq!(fs::read(job.join("dataset/rebalance.json")).unwrap(), b"{}");
    }

    #[test]
    fn current_with_progress_reads_the_sidecar() {
        let root = tempfile::tempdir().unwrap();
        let models = root.path();
        let job = HostJob::create("jerry", "lora", "ohwx", "base.safetensors");
        let dir = job_dir(models, job.id);
        write_job(&dir, &job).unwrap();
        fs::write(
            dir.join("progress.json"),
            br#"{"step":0,"total":8000,"phase":"class_images","message":"Generating class image 12 of 2000","percent":8,"eta_seconds":14400,"loss":0.21}"#,
        )
        .unwrap();
        let viewed = current_with_progress(models).unwrap();
        assert_eq!(viewed.step, Some(0));
        assert_eq!(viewed.total, Some(8000));
        assert_eq!(viewed.phase.as_deref(), Some("class_images"));
        assert_eq!(
            viewed.message.as_deref(),
            Some("Generating class image 12 of 2000")
        );
        assert_eq!(viewed.percent, Some(8));
        assert_eq!(viewed.eta_seconds, Some(14400));
        assert_eq!(viewed.loss, Some(0.21));
        assert!(viewed.previews.is_empty());
        assert_eq!(viewed.name, "jerry");
        fs::write(
            dir.join("progress.json"),
            br#"{"step":250,"total":8000,"previews":["previews/step-250-0.png"]}"#,
        )
        .unwrap();
        let viewed = current_with_progress(models).unwrap();
        assert_eq!(viewed.previews, vec!["previews/step-250-0.png".to_string()]);
    }

    #[test]
    fn preview_file_resolves_a_basename_under_the_current_job() {
        let root = tempfile::tempdir().unwrap();
        let models = root.path();
        let job = HostJob::create("jerry", "lora", "ohwx", "base.safetensors");
        let dir = job_dir(models, job.id);
        write_job(&dir, &job).unwrap();
        let previews = dir.join("previews");
        fs::create_dir_all(&previews).unwrap();
        let png = previews.join("step-250-0.png");
        fs::write(&png, b"png").unwrap();
        assert_eq!(preview_file(models, "step-250-0.png"), Some(png));
        assert!(preview_file(models, "../job.json").is_none());
        assert!(preview_file(models, "missing.png").is_none());
        let empty = tempfile::tempdir().unwrap();
        assert!(preview_file(empty.path(), "step-250-0.png").is_none());
    }

    #[test]
    fn queued_progress_keeps_parse_progress_alive() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("job");
        write_queued_progress(&dir, 40).unwrap();
        let bytes = fs::read(dir.join("progress.json")).unwrap();
        let update = parse_progress(&bytes).unwrap();
        assert_eq!(update.step, 0);
        assert_eq!(update.total, 800);
        assert_eq!(update.phase.as_deref(), Some("queued"));
        assert_eq!(update.percent, Some(0));
    }

    #[test]
    fn person_step_budget_matches_the_host_config() {
        assert_eq!(steps_for(1), 500);
        assert_eq!(steps_for(25), 500);
        assert_eq!(steps_for(26), 520);
        assert_eq!(steps_for(400), 8000);
        assert_eq!(steps_for(500), 8000);
    }

    #[test]
    fn video_step_budget_matches_the_wan_config() {
        assert_eq!(video_steps_for(1), 150);
        assert_eq!(video_steps_for(7), 150);
        assert_eq!(video_steps_for(8), 160);
        assert_eq!(video_steps_for(100), 2000);
        assert_eq!(video_steps_for(200), 2000);
    }

    #[test]
    fn language_step_budget_clamps_to_the_chat_range() {
        assert_eq!(language_steps_for(1), 200);
        assert_eq!(language_steps_for(200), 200);
        assert_eq!(language_steps_for(1000), 1000);
        assert_eq!(language_steps_for(5000), 1000);
    }

    #[test]
    fn host_job_without_subject_deserializes() {
        let job = HostJob::create("yvonne", "finetune", "ohwx", "base.safetensors");
        let mut value = serde_json::to_value(&job).unwrap();
        value.as_object_mut().unwrap().remove("subject");
        let loaded: HostJob = serde_json::from_value(value).unwrap();
        assert_eq!(loaded.subject, "");
        assert_eq!(loaded.name, "yvonne");
        assert_eq!(loaded.method, "finetune");
    }

    #[test]
    fn host_job_defaults_provider_to_local_and_round_trips_runpod_without_a_key() {
        let job = HostJob::create("jerry", "lora", "ohwx", "base.safetensors");
        assert_eq!(job.provider, TrainProvider::Local);
        let mut value = serde_json::to_value(&job).unwrap();
        value.as_object_mut().unwrap().remove("provider");
        let loaded: HostJob = serde_json::from_value(value).unwrap();
        assert_eq!(loaded.provider, TrainProvider::Local);

        let mut runpod = HostJob::create("jerry", "finetune", "ohwx", "base.safetensors");
        runpod.provider = TrainProvider::Runpod;
        runpod.gpu = Some("A40".into());
        runpod.pod_id = Some("pod-1".into());
        let encoded = serde_json::to_value(&runpod).unwrap();
        assert_eq!(encoded["provider"], "runpod");
        assert_eq!(encoded["gpu"], "A40");
        assert_eq!(encoded["pod_id"], "pod-1");
        assert!(encoded.get("runpod_api_key").is_none());
        assert!(encoded.get("api_key").is_none());
        let loaded: HostJob = serde_json::from_value(encoded).unwrap();
        assert_eq!(loaded.provider, TrainProvider::Runpod);
        assert_eq!(loaded.gpu.as_deref(), Some("A40"));
        assert_eq!(loaded.pod_id.as_deref(), Some("pod-1"));
    }

    #[test]
    fn write_runpod_key_is_one_line_and_mode_600() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("job");
        write_runpod_key(&dir, " rp-secret \n").unwrap();
        let path = dir.join("runpod.key");
        assert_eq!(fs::read_to_string(&path).unwrap(), "rp-secret\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn language_host_job_round_trips_subject() {
        let mut job = HostJob::create("support-bot", "lora", "", "qwen2.5:7b");
        job.subject = "language".into();
        job.recipe_id = "chat".into();
        job.filename = Some("support-bot".into());
        let encoded = serde_json::to_vec(&job).unwrap();
        let loaded: HostJob = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(loaded.subject, "language");
        assert_eq!(loaded.recipe_id, "chat");
        assert_eq!(loaded.checkpoint, "qwen2.5:7b");
        assert_eq!(loaded.filename.as_deref(), Some("support-bot"));
        assert_eq!(loaded.trigger, "");
        assert_eq!(loaded.method, "lora");
    }

    #[test]
    fn stage_language_copies_train_jsonl() {
        let root = tempfile::tempdir().unwrap();
        let attempt = root.path().join("attempt");
        fs::create_dir_all(attempt.join("data")).unwrap();
        fs::write(attempt.join("data/train.jsonl"), b"{\"text\":\"hi\"}\n").unwrap();
        fs::write(attempt.join("data/format.json"), b"{\"mask_prompt\":false}").unwrap();
        let job = root.path().join("job");
        stage_language(&attempt, &job).unwrap();
        assert_eq!(
            fs::read(job.join("data/train.jsonl")).unwrap(),
            b"{\"text\":\"hi\"}\n"
        );
        assert_eq!(
            fs::read(job.join("data/format.json")).unwrap(),
            b"{\"mask_prompt\":false}"
        );
        assert!(!job.join("dataset").exists());
    }

    #[test]
    fn stage_clips_copies_windows() {
        let root = tempfile::tempdir().unwrap();
        let attempt = root.path().join("attempt");
        let clips = crate::dataset::clips_dir(&attempt);
        fs::create_dir_all(&clips).unwrap();
        let window = crate::dataset::ClipWindow {
            start_s: 0.5,
            end_s: 2.5,
            pose: "standing".into(),
        };
        crate::dataset::write_clip_window(&clips, "0000", b"mp4", &window).unwrap();
        let job = root.path().join("job");
        stage_clips(&attempt, &job).unwrap();
        let staged = crate::dataset::clips_dir(&job);
        assert_eq!(fs::read(staged.join("0000.mp4")).unwrap(), b"mp4");
        let loaded: crate::dataset::ClipWindow =
            serde_json::from_slice(&fs::read(staged.join("0000.json")).unwrap()).unwrap();
        assert_eq!(loaded, window);
    }

    #[tokio::test]
    async fn wait_returns_when_the_host_job_succeeds() {
        let root = tempfile::tempdir().unwrap();
        let job = HostJob::create("jerry", "lora", "ohwx", "base.safetensors");
        let dir = job_dir(root.path(), job.id);
        write_job(&dir, &job).unwrap();
        let watching = dir.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            fs::write(watching.join("progress.json"), br#"{"step":3,"total":10}"#).unwrap();
            let mut finished = read_job(&watching).unwrap();
            finished.status = HostStatus::Succeeded;
            finished.filename = Some("jerry.safetensors".into());
            write_job(&watching, &finished).unwrap();
        });
        let finished = wait(&dir, None).await.unwrap();
        assert_eq!(finished.status, HostStatus::Succeeded);
        assert_eq!(finished.filename.as_deref(), Some("jerry.safetensors"));
    }
}
