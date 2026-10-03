//! One in-process LoRA training job. The request that starts it returns before
//! the trainer finishes, so a refresh does not cancel the run.

use serde::Serialize;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use zone_comfy::dataset::Finding;
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
}

#[derive(Clone, Default)]
pub struct TrainRegistry {
    slot: Arc<Mutex<Option<Arc<Job>>>>,
}

pub struct Job {
    id: Uuid,
    name: String,
    snapshot: Mutex<Snapshot>,
}

struct Snapshot {
    status: TrainJobStatus,
    filename: Option<String>,
    quality: Option<Quality>,
    dataset: Option<Vec<Finding>>,
    screening: Option<Screening>,
    error: Option<String>,
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
            snapshot: Mutex::new(Snapshot {
                status: TrainJobStatus::Running,
                filename: None,
                quality: None,
                dataset: None,
                screening: None,
                error: None,
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

#[cfg(test)]
mod tests {
    use super::{TrainJobStatus, TrainRegistry};
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
}
