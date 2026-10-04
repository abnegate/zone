//! One in-process LoRA training job. The request that starts it returns before
//! the trainer finishes, so a refresh does not cancel the run.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;
use zone_comfy::TrainProgress;
use zone_comfy::dataset::Finding;
use zone_comfy::host_train::{HostJob, HostStatus};
use zone_comfy::lora::{Screening, TrainError, TrainOutcome};
use zone_comfy::quality::Quality;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainJobStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct TrainJobView {
    pub id: Uuid,
    pub name: String,
    pub status: TrainJobStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<Quality>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset: Option<Vec<Finding>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screening: Option<Screening>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loss: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    pub started_at: DateTime<Utc>,
}

#[derive(Clone, Default)]
pub struct TrainRegistry {
    slot: Arc<Mutex<Option<Arc<Job>>>>,
}

pub struct Job {
    id: Uuid,
    name: String,
    method: Option<String>,
    started_at: DateTime<Utc>,
    snapshot: Mutex<Snapshot>,
}

struct Snapshot {
    status: TrainJobStatus,
    filename: Option<String>,
    quality: Option<Quality>,
    dataset: Option<Vec<Finding>>,
    screening: Option<Screening>,
    error: Option<String>,
    step: Option<u32>,
    total: Option<u32>,
    phase: Option<String>,
    message: Option<String>,
    percent: Option<u8>,
    loss: Option<f32>,
    eta_override: Option<u64>,
    step_started: Option<Instant>,
}

impl TrainRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current(&self) -> Option<TrainJobView> {
        self.slot
            .lock()
            .expect("train job")
            .as_ref()
            .map(|job| job.view())
    }

    /// Occupies the single training slot. `None` when a run is already in
    /// progress, so the caller attaches to `current` instead of starting two.
    pub fn start(&self, name: String, method: Option<String>) -> Option<Arc<Job>> {
        let mut slot = self.slot.lock().expect("train job");
        if slot
            .as_ref()
            .is_some_and(|job| job.view().status == TrainJobStatus::Running)
        {
            return None;
        }
        let job = Job::running(name, method);
        *slot = Some(job.clone());
        Some(job)
    }
}

impl Job {
    fn running(name: String, method: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            id: Uuid::new_v4(),
            name,
            method,
            started_at: Utc::now(),
            snapshot: Mutex::new(Snapshot {
                status: TrainJobStatus::Running,
                filename: None,
                quality: None,
                dataset: None,
                screening: None,
                error: None,
                step: None,
                total: None,
                phase: None,
                message: None,
                percent: None,
                loss: None,
                eta_override: None,
                step_started: None,
            }),
        })
    }

    pub fn view(&self) -> TrainJobView {
        let snapshot = self.snapshot.lock().expect("train job");
        TrainJobView {
            id: self.id,
            name: self.name.clone(),
            status: snapshot.status,
            filename: snapshot.filename.clone(),
            quality: snapshot.quality.clone(),
            dataset: snapshot.dataset.clone(),
            screening: snapshot.screening.clone(),
            error: snapshot.error.clone(),
            step: snapshot.step,
            total: snapshot.total,
            phase: snapshot.phase.clone(),
            message: snapshot.message.clone(),
            percent: snapshot.percent,
            loss: snapshot.loss,
            eta_seconds: snapshot
                .eta_override
                .filter(|seconds| *seconds > 0)
                .or_else(|| {
                    eta_seconds(
                        snapshot.step,
                        snapshot.total,
                        snapshot.step_started.map(|started| started.elapsed()),
                    )
                }),
            method: self.method.clone(),
            started_at: self.started_at,
        }
    }

    pub fn progress(&self, update: TrainProgress) {
        if update.total == 0 {
            return;
        }
        let mut snapshot = self.snapshot.lock().expect("train job");
        if snapshot.status != TrainJobStatus::Running {
            return;
        }
        snapshot.step = Some(update.step.min(update.total));
        snapshot.total = Some(update.total);
        snapshot.phase = update.phase;
        snapshot.message = update.message;
        snapshot.percent = update.percent;
        snapshot.loss = update.loss;
        snapshot.eta_override = update.eta_seconds;
        if update.step > 0 && snapshot.step_started.is_none() {
            snapshot.step_started = Some(Instant::now());
        }
    }

    pub fn succeed(&self, outcome: TrainOutcome) {
        let filename = outcome
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string);
        let mut snapshot = self.snapshot.lock().expect("train job");
        snapshot.status = TrainJobStatus::Succeeded;
        snapshot.filename = filename;
        snapshot.quality = outcome.quality;
        snapshot.dataset = Some(outcome.dataset);
        snapshot.screening = Some(outcome.screening);
        snapshot.error = None;
    }

    pub fn fail(&self, error: TrainError) {
        let message = match error {
            TrainError::Disabled => "LoRA training is not configured on this server".to_string(),
            TrainError::Invalid(message) => message.to_string(),
            TrainError::Failed(message) => message,
        };
        let mut snapshot = self.snapshot.lock().expect("train job");
        snapshot.status = TrainJobStatus::Failed;
        snapshot.error = Some(message);
    }
}

impl TrainJobView {
    pub fn from_host(job: HostJob) -> Self {
        let status = match job.status {
            HostStatus::Queued | HostStatus::Running => TrainJobStatus::Running,
            HostStatus::Succeeded => TrainJobStatus::Succeeded,
            HostStatus::Failed => TrainJobStatus::Failed,
        };
        let elapsed = Utc::now()
            .signed_duration_since(job.started_at)
            .to_std()
            .ok();
        let phase = job.phase;
        let eta_seconds =
            job.eta_seconds
                .filter(|seconds| *seconds > 0)
                .or_else(|| match phase.as_deref() {
                    Some("training") | None => eta_seconds(job.step, job.total, elapsed),
                    _ => None,
                });
        Self {
            id: job.id,
            name: job.name,
            status,
            filename: job.filename,
            quality: None,
            dataset: None,
            screening: None,
            error: job.error,
            step: job.step,
            total: job.total,
            phase,
            message: job.message,
            percent: job.percent,
            loss: job.loss,
            eta_seconds,
            method: Some(job.method),
            started_at: job.started_at,
        }
    }
}

fn eta_seconds(step: Option<u32>, total: Option<u32>, elapsed: Option<Duration>) -> Option<u64> {
    let step = step.filter(|step| *step > 0)?;
    let total = total.filter(|total| *total > step)?;
    let elapsed = elapsed.filter(|elapsed| !elapsed.is_zero())?;
    let remaining = elapsed.as_secs_f64() * f64::from(total - step) / f64::from(step);
    Some(remaining.round() as u64)
}

#[cfg(test)]
mod tests {
    use super::{TrainJobStatus, TrainRegistry, eta_seconds};
    use std::time::Duration;
    use zone_comfy::TrainProgress;
    use zone_comfy::lora::TrainError;

    #[test]
    fn a_running_job_refuses_a_second_start() {
        let registry = TrainRegistry::new();
        let job = registry
            .start("jerry".into(), Some("lora".into()))
            .expect("first start");
        assert!(registry.start("other".into(), None).is_none());
        assert_eq!(registry.current().unwrap().name, "jerry");
        assert_eq!(registry.current().unwrap().status, TrainJobStatus::Running);
        job.fail(TrainError::Failed("stopped".into()));
        let next = registry
            .start("other".into(), Some("finetune".into()))
            .expect("finished job frees the slot");
        assert_eq!(next.view().name, "other");
        assert_eq!(next.view().status, TrainJobStatus::Running);
    }

    #[test]
    fn progress_is_visible_on_the_running_job() {
        let registry = TrainRegistry::new();
        let job = registry
            .start("jerry".into(), Some("finetune".into()))
            .expect("start");
        job.progress(
            TrainProgress::new(0, 8000)
                .phase("class_images", "Generating class image 12 of 2000")
                .percent(8)
                .eta_seconds(14400),
        );
        let view = registry.current().unwrap();
        assert_eq!(view.step, Some(0));
        assert_eq!(view.total, Some(8000));
        assert_eq!(view.phase.as_deref(), Some("class_images"));
        assert_eq!(view.percent, Some(8));
        assert_eq!(view.eta_seconds, Some(14400));
        assert_eq!(view.method.as_deref(), Some("finetune"));
        job.progress(TrainProgress::new(12, 400).loss(0.21));
        let view = registry.current().unwrap();
        assert_eq!(view.step, Some(12));
        assert_eq!(view.total, Some(400));
        assert_eq!(view.loss, Some(0.21));
        assert!(view.started_at.timestamp() > 0);
        job.fail(TrainError::Failed("stopped".into()));
        job.progress(TrainProgress::new(13, 400));
        assert_eq!(job.view().step, Some(12));
    }

    #[test]
    fn eta_scales_remaining_steps_by_elapsed_time() {
        assert_eq!(
            eta_seconds(Some(10), Some(20), Some(Duration::from_secs(50))),
            Some(50)
        );
        assert_eq!(
            eta_seconds(Some(0), Some(20), Some(Duration::from_secs(5))),
            None
        );
        assert_eq!(
            eta_seconds(Some(20), Some(20), Some(Duration::from_secs(5))),
            None
        );
    }

    #[test]
    fn a_queued_host_job_looks_running_to_the_client() {
        let mut job = zone_comfy::host_train::HostJob::create(
            "jerry",
            "finetune",
            "ohwx",
            "lustifySDXLNSFW_ggwpV7.safetensors",
        );
        job.step = Some(4);
        job.total = Some(20);
        let view = super::TrainJobView::from_host(job);
        assert_eq!(view.name, "jerry");
        assert_eq!(view.status, TrainJobStatus::Running);
        assert_eq!(view.step, Some(4));
        assert_eq!(view.total, Some(20));
        assert_eq!(view.method.as_deref(), Some("finetune"));
    }

    #[test]
    fn host_eta_prefers_the_sidecar_over_job_age() {
        let mut job = zone_comfy::host_train::HostJob::create(
            "jerry",
            "finetune",
            "ohwx",
            "lustifySDXLNSFW_ggwpV7.safetensors",
        );
        job.step = Some(0);
        job.total = Some(8000);
        job.phase = Some("class_images".into());
        job.message = Some("Generating class image 12 of 2000".into());
        job.percent = Some(8);
        job.eta_seconds = Some(14_400);
        job.loss = Some(0.21);
        let view = super::TrainJobView::from_host(job);
        assert_eq!(view.eta_seconds, Some(14_400));
        assert_eq!(view.percent, Some(8));
        assert_eq!(view.phase.as_deref(), Some("class_images"));
        assert_eq!(view.loss, Some(0.21));
    }
}
