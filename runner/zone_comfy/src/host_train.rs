//! SDXL person trains run on the host, not inside ComfyUI or the manager
//! container. Job state lives on the models bind-mount so a manager recreate
//! does not cancel a week-long fine-tune.

use crate::lora::TrainError;
use crate::train::{TrainProgress, parse_progress};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
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
    pub trigger: String,
    pub checkpoint: String,
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
    pub pid: Option<u32>,
    pub started_at: DateTime<Utc>,
}

impl HostJob {
    pub fn create(name: &str, method: &str, trigger: &str, checkpoint: &str) -> Self {
        Self {
            schema_version: 1,
            id: Uuid::new_v4(),
            name: name.to_string(),
            method: method.to_string(),
            trigger: trigger.to_string(),
            checkpoint: checkpoint.to_string(),
            status: HostStatus::Queued,
            filename: None,
            recipe_id: String::new(),
            hf_base: String::new(),
            image_count: 0,
            error: None,
            step: None,
            total: None,
            pid: None,
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

fn overlay_progress(dir: &Path, job: &mut HostJob) {
    let Ok(bytes) = fs::read(dir.join("progress.json")) else {
        return;
    };
    let Some(update) = parse_progress(&bytes) else {
        return;
    };
    job.step = Some(update.step);
    job.total = Some(update.total);
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

pub fn read_job(dir: &Path) -> Result<HostJob, TrainError> {
    let bytes =
        fs::read(dir.join("job.json")).map_err(|error| TrainError::Failed(error.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|error| TrainError::Failed(error.to_string()))
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
    fn current_with_progress_reads_the_sidecar() {
        let root = tempfile::tempdir().unwrap();
        let models = root.path();
        let job = HostJob::create("jerry", "lora", "ohwx", "base.safetensors");
        let dir = job_dir(models, job.id);
        write_job(&dir, &job).unwrap();
        fs::write(dir.join("progress.json"), br#"{"step":12,"total":400}"#).unwrap();
        let viewed = current_with_progress(models).unwrap();
        assert_eq!(viewed.step, Some(12));
        assert_eq!(viewed.total, Some(400));
        assert_eq!(viewed.name, "jerry");
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
