//! One in-process LoRA training job. The request that starts it returns before
//! the trainer finishes, so a refresh does not cancel the run.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;
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
    pub eta_seconds: Option<u64>,
    pub started_at: DateTime<Utc>,
}

#[derive(Clone, Default)]
pub struct TrainRegistry {
    slot: Arc<Mutex<Option<Arc<Job>>>>,
}

pub struct Job {
    id: Uuid,
    name: String,
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
    pub fn start(&self, name: String) -> Option<Arc<Job>> {
        let mut slot = self.slot.lock().expect("train job");
        if slot
            .as_ref()
            .is_some_and(|job| job.view().status == TrainJobStatus::Running)
        {
            return None;
        }
        let job = Job::running(name);
        *slot = Some(job.clone());
        Some(job)
    }
}

impl Job {
    fn running(name: String) -> Arc<Self> {
        Arc::new(Self {
            id: Uuid::new_v4(),
            name,
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
            eta_seconds: eta_seconds(
                snapshot.step,
                snapshot.total,
                snapshot.step_started.map(|started| started.elapsed()),
            ),
            started_at: self.started_at,
        }
    }

    pub fn progress(&self, step: u32, total: u32) {
        if total == 0 {
            return;
        }
        let mut snapshot = self.snapshot.lock().expect("train job");
        if snapshot.status != TrainJobStatus::Running {
            return;
        }
        snapshot.step = Some(step.min(total));
        snapshot.total = Some(total);
        if step > 0 && snapshot.step_started.is_none() {
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
            eta_seconds: eta_seconds(job.step, job.total, elapsed),
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
    use zone_comfy::lora::TrainError;

    #[test]
    fn a_running_job_refuses_a_second_start() {
        let registry = TrainRegistry::new();
        let job = registry.start("jerry".into()).expect("first start");
        assert!(registry.start("other".into()).is_none());
        assert_eq!(registry.current().unwrap().name, "jerry");
        assert_eq!(registry.current().unwrap().status, TrainJobStatus::Running);
        job.fail(TrainError::Failed("stopped".into()));
        let next = registry
            .start("other".into())
            .expect("finished job frees the slot");
        assert_eq!(next.view().name, "other");
        assert_eq!(next.view().status, TrainJobStatus::Running);
    }

    #[test]
    fn progress_is_visible_on_the_running_job() {
        let registry = TrainRegistry::new();
        let job = registry.start("jerry".into()).expect("start");
        job.progress(12, 400);
        let view = registry.current().unwrap();
        assert_eq!(view.step, Some(12));
        assert_eq!(view.total, Some(400));
        assert!(view.started_at.timestamp() > 0);
        job.fail(TrainError::Failed("stopped".into()));
        job.progress(13, 400);
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
    }
}
