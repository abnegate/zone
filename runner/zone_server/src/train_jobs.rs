//! One in-process LoRA training job. The request that starts it returns before
//! the trainer finishes, so a refresh does not cancel the run.

mod recovery;
mod screening_view;

pub use recovery::recover;
pub use screening_view::ScreeningView;

use crate::services::comfy::language;
use abnegate_comfy::dataset::Finding;
use abnegate_comfy::host;
use abnegate_comfy::host::Status;
use abnegate_comfy::lora::TrainError;
use abnegate_comfy::lora::TrainOutcome;
use abnegate_comfy::quality::Quality;
use abnegate_comfy::train::Phase;
use abnegate_comfy::train::TrainProgress;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use uuid::Uuid;

pub const DISABLED: &str = "LoRA training is not configured on this server";

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
    pub screening: Option<ScreeningView>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub previews: Vec<String>,
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
    provider: Option<String>,
    started_at: DateTime<Utc>,
    snapshot: Mutex<Snapshot>,
}

struct Snapshot {
    status: TrainJobStatus,
    filename: Option<String>,
    quality: Option<Quality>,
    dataset: Option<Vec<Finding>>,
    screening: Option<ScreeningView>,
    error: Option<String>,
    step: Option<u32>,
    total: Option<u32>,
    phase: Option<String>,
    message: Option<String>,
    percent: Option<u8>,
    loss: Option<f32>,
    eta_override: Option<u64>,
    step_started: Option<Instant>,
    previews: Vec<String>,
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
    pub fn start(
        &self,
        name: String,
        method: Option<String>,
        provider: Option<String>,
    ) -> Option<Arc<Job>> {
        self.occupy(Job::running(
            Uuid::new_v4(),
            name,
            method,
            provider,
            Utc::now(),
        ))
    }

    /// Occupies the slot with the host job `job`, queued before a restart,
    /// under its own id and start, so following it again shows the run the
    /// client already knew.
    pub fn resume(&self, job: &host::Job) -> Option<Arc<Job>> {
        self.occupy(Job::running(
            job.id,
            job.name.clone(),
            Some(job.method.clone()),
            Some(job.compute.as_str().to_string()),
            job.started_at,
        ))
    }

    fn occupy(&self, job: Arc<Job>) -> Option<Arc<Job>> {
        let mut slot = self.slot.lock().expect("train job");
        if slot
            .as_ref()
            .is_some_and(|job| job.view().status == TrainJobStatus::Running)
        {
            return None;
        }
        *slot = Some(job.clone());
        Some(job)
    }

    /// Drops a finished in-process job so GET /train returns 204 until a new run.
    pub fn clear_finished(&self) {
        let mut slot = self.slot.lock().expect("train job");
        if slot
            .as_ref()
            .is_some_and(|job| job.view().status == TrainJobStatus::Running)
        {
            return;
        }
        *slot = None;
    }
}

impl Job {
    fn running(
        id: Uuid,
        name: String,
        method: Option<String>,
        provider: Option<String>,
        started_at: DateTime<Utc>,
    ) -> Arc<Self> {
        Arc::new(Self {
            id,
            name,
            method,
            provider,
            started_at,
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
                previews: Vec::new(),
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
            subject: None,
            provider: self.provider.clone(),
            gpu: None,
            pod_id: None,
            previews: snapshot.previews.clone(),
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
        snapshot.phase = update.phase.map(String::from);
        snapshot.message = update.message;
        snapshot.percent = update.percent;
        snapshot.loss = update.loss;
        snapshot.eta_override = update.eta.map(|eta| eta.as_secs());
        snapshot.previews = update.previews;
        if update.step > 0 && snapshot.step_started.is_none() {
            snapshot.step_started = Some(Instant::now());
        }
    }

    /// Runs `training` to its end, keeping the progress it reports through
    /// `receiver` on this job, the last update included.
    pub async fn follow<T>(
        &self,
        mut receiver: watch::Receiver<Option<TrainProgress>>,
        training: impl Future<Output = Result<T, TrainError>>,
    ) -> Result<T, TrainError> {
        tokio::pin!(training);
        let mut open = true;
        loop {
            tokio::select! {
                biased;
                changed = receiver.changed(), if open => match changed {
                    Ok(()) => {
                        let update = receiver.borrow_and_update().clone();
                        if let Some(update) = update {
                            self.progress(update);
                        }
                    }
                    Err(_) => open = false,
                },
                result = &mut training => {
                    let latest = receiver.borrow_and_update();
                    if latest.has_changed()
                        && let Some(update) = latest.clone()
                    {
                        self.progress(update);
                    }
                    return result;
                }
            }
        }
    }

    pub fn succeed(&self, outcome: TrainOutcome) {
        self.finish(
            &outcome.path,
            outcome.quality,
            Some(outcome.dataset),
            Some(outcome.screening.into()),
        );
    }

    pub fn succeed_language(&self, outcome: language::Outcome) {
        self.finish(
            &outcome.path,
            None,
            Some(Vec::new()),
            Some(ScreeningView::kept(outcome.examples)),
        );
    }

    /// A resumed host job published at `path`. Its quality and what it kept
    /// of its set went with the process that queued it.
    pub fn succeed_resumed(&self, path: &Path) {
        self.finish(path, None, None, None);
    }

    fn finish(
        &self,
        path: &Path,
        quality: Option<Quality>,
        dataset: Option<Vec<Finding>>,
        screening: Option<ScreeningView>,
    ) {
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string);
        let mut snapshot = self.snapshot.lock().expect("train job");
        snapshot.status = TrainJobStatus::Succeeded;
        snapshot.filename = filename;
        snapshot.quality = quality;
        snapshot.dataset = dataset;
        snapshot.screening = screening;
        snapshot.error = None;
    }

    pub fn fail(&self, error: TrainError) {
        let message = match error {
            TrainError::Disabled => DISABLED.to_string(),
            TrainError::Invalid(message) => message.to_string(),
            TrainError::Failed(message) => message,
            other => other.to_string(),
        };
        let mut snapshot = self.snapshot.lock().expect("train job");
        snapshot.status = TrainJobStatus::Failed;
        snapshot.error = Some(message);
    }
}

impl TrainJobView {
    pub fn from_host(job: host::Job) -> Self {
        let status = match job.status {
            Status::Succeeded => TrainJobStatus::Succeeded,
            Status::Failed => TrainJobStatus::Failed,
            _ => TrainJobStatus::Running,
        };
        let elapsed = Utc::now()
            .signed_duration_since(job.started_at)
            .to_std()
            .ok();
        let progress = job.progress;
        let step = progress.as_ref().map(|progress| progress.step);
        let total = progress.as_ref().map(|progress| progress.total);
        let phase = progress
            .as_ref()
            .and_then(|progress| progress.phase.clone());
        let eta_seconds = progress
            .as_ref()
            .and_then(|progress| progress.eta)
            .map(|eta| eta.as_secs())
            .filter(|seconds| *seconds > 0)
            .or_else(|| match phase {
                Some(Phase::Training) | None => eta_seconds(step, total, elapsed),
                _ => None,
            });
        let (message, percent, loss, previews) = match progress {
            Some(progress) => (
                progress.message,
                progress.percent,
                progress.loss,
                progress.previews,
            ),
            None => (None, None, None, Vec::new()),
        };
        Self {
            id: job.id,
            name: job.name,
            status,
            filename: job.filename,
            quality: None,
            dataset: None,
            screening: None,
            error: job.error,
            step,
            total,
            phase: phase.map(String::from),
            message,
            percent,
            loss,
            eta_seconds,
            method: Some(job.method),
            subject: (!job.subject.is_empty()).then_some(job.subject),
            provider: Some(job.compute.as_str().to_string()),
            gpu: job.gpu,
            pod_id: job.pod_id,
            previews,
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
    use super::{DISABLED, TrainJobStatus, TrainJobView, TrainRegistry, eta_seconds};
    use crate::services::comfy::language;
    use abnegate_comfy::host::Compute;
    use abnegate_comfy::host::Job;
    use abnegate_comfy::lora::TrainError;
    use abnegate_comfy::train::Phase;
    use abnegate_comfy::train::TrainProgress;
    use std::path::PathBuf;
    use std::time::Duration;

    const CHECKPOINT: &str = "lustifySDXLNSFW_ggwpV7.safetensors";

    fn host_job(name: &str, method: &str, trigger: &str, checkpoint: &str) -> Job {
        Job::new(name, method, checkpoint).with_trigger(trigger)
    }

    #[test]
    fn a_running_job_refuses_a_second_start() {
        let registry = TrainRegistry::new();
        let job = registry
            .start("jerry".into(), Some("lora".into()), None)
            .expect("first start");
        assert!(registry.start("other".into(), None, None).is_none());
        assert_eq!(registry.current().unwrap().name, "jerry");
        assert_eq!(registry.current().unwrap().status, TrainJobStatus::Running);
        job.fail(TrainError::Failed("stopped".into()));
        let next = registry
            .start("other".into(), Some("finetune".into()), None)
            .expect("finished job frees the slot");
        assert_eq!(next.view().name, "other");
        assert_eq!(next.view().status, TrainJobStatus::Running);
    }

    #[test]
    fn clear_finished_drops_a_terminal_job_and_keeps_a_running_one() {
        let registry = TrainRegistry::new();
        let job = registry
            .start("yvonne".into(), Some("finetune".into()), None)
            .expect("start");
        registry.clear_finished();
        assert_eq!(registry.current().unwrap().name, "yvonne");
        job.fail(TrainError::Failed("cancelled".into()));
        registry.clear_finished();
        assert!(registry.current().is_none());
    }

    #[test]
    fn progress_is_visible_on_the_running_job() {
        let registry = TrainRegistry::new();
        let job = registry
            .start(
                "jerry".into(),
                Some("finetune".into()),
                Some("runpod".into()),
            )
            .expect("start");
        job.progress(
            TrainProgress::new(0, 8000)
                .with_phase(Phase::ClassImages)
                .with_message("Generating class image 12 of 2000")
                .with_percent(8)
                .with_eta(Duration::from_secs(14400)),
        );
        let view = registry.current().unwrap();
        assert_eq!(view.step, Some(0));
        assert_eq!(view.total, Some(8000));
        assert_eq!(view.phase.as_deref(), Some("class_images"));
        assert_eq!(view.percent, Some(8));
        assert_eq!(view.eta_seconds, Some(14400));
        assert_eq!(view.method.as_deref(), Some("finetune"));
        assert_eq!(view.provider.as_deref(), Some("runpod"));
        job.progress(TrainProgress::new(12, 400).with_loss(0.21));
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
        let mut job = host_job("jerry", "finetune", "ohwx", CHECKPOINT);
        job.progress = Some(TrainProgress::new(4, 20));
        let view = TrainJobView::from_host(job);
        assert_eq!(view.name, "jerry");
        assert_eq!(view.status, TrainJobStatus::Running);
        assert_eq!(view.step, Some(4));
        assert_eq!(view.total, Some(20));
        assert_eq!(view.method.as_deref(), Some("finetune"));
        assert_eq!(view.subject, None);
        assert_eq!(view.provider.as_deref(), Some("local"));
        assert_eq!(view.gpu, None);
        assert_eq!(view.pod_id, None);
    }

    #[test]
    fn host_job_compute_is_surfaced_when_set() {
        let mut job =
            host_job("jerry", "finetune", "ohwx", CHECKPOINT).with_compute(Compute::Runpod);
        job.gpu = Some("A40".into());
        job.pod_id = Some("pod-1".into());
        let view = TrainJobView::from_host(job);
        assert_eq!(view.provider.as_deref(), Some("runpod"));
        assert_eq!(view.gpu.as_deref(), Some("A40"));
        assert_eq!(view.pod_id.as_deref(), Some("pod-1"));
        let encoded = serde_json::to_value(&view).unwrap();
        assert_eq!(encoded["provider"], "runpod");
        assert_eq!(encoded["gpu"], "A40");
        assert!(encoded.get("runpod_api_key").is_none());
    }

    #[test]
    fn host_job_subject_is_surfaced_when_set() {
        let job = host_job("support-bot", "lora", "", "llama3.2:1b").with_subject("language");
        let view = TrainJobView::from_host(job);
        assert_eq!(view.subject.as_deref(), Some("language"));
        let encoded = serde_json::to_value(&view).unwrap();
        assert_eq!(encoded["subject"], "language");
    }

    #[test]
    fn host_eta_prefers_the_sidecar_over_job_age() {
        let mut job = host_job("jerry", "finetune", "ohwx", CHECKPOINT);
        job.progress = Some(
            TrainProgress::new(0, 8000)
                .with_phase(Phase::ClassImages)
                .with_message("Generating class image 12 of 2000")
                .with_percent(8)
                .with_eta(Duration::from_secs(14_400))
                .with_loss(0.21)
                .with_previews(vec!["previews/step-250-0.png".into()]),
        );
        let view = TrainJobView::from_host(job);
        assert_eq!(view.eta_seconds, Some(14_400));
        assert_eq!(view.percent, Some(8));
        assert_eq!(view.phase.as_deref(), Some("class_images"));
        assert_eq!(
            view.message.as_deref(),
            Some("Generating class image 12 of 2000")
        );
        assert_eq!(view.loss, Some(0.21));
        assert_eq!(view.previews, ["previews/step-250-0.png"]);
    }

    #[test]
    fn a_host_job_without_progress_shows_no_steps() {
        let view = TrainJobView::from_host(host_job("jerry", "lora", "ohwx", CHECKPOINT));
        assert_eq!(view.step, None);
        assert_eq!(view.total, None);
        assert_eq!(view.phase, None);
        assert_eq!(view.eta_seconds, None);
        assert!(view.previews.is_empty());
    }

    #[test]
    fn a_finished_language_run_reports_its_examples_as_kept() {
        let registry = TrainRegistry::new();
        let job = registry
            .start(
                "support-bot".into(),
                Some("lora".into()),
                Some("local".into()),
            )
            .expect("start");
        job.succeed_language(language::Outcome {
            path: PathBuf::from("/models/.zone-train/job/support-bot"),
            examples: 42,
        });
        let view = registry.current().unwrap();
        assert_eq!(view.status, TrainJobStatus::Succeeded);
        assert_eq!(view.filename.as_deref(), Some("support-bot"));
        assert!(view.quality.is_none());
        let encoded = serde_json::to_value(&view).unwrap();
        assert_eq!(encoded["dataset"], serde_json::json!([]));
        assert_eq!(
            encoded["screening"],
            serde_json::json!({"kept": 42, "dropped": [], "attempted": []})
        );
    }

    #[test]
    fn a_failed_run_reads_the_reason_the_client_shows() {
        let registry = TrainRegistry::new();
        let job = registry.start("jerry".into(), None, None).expect("start");
        job.fail(TrainError::Disabled);
        assert_eq!(registry.current().unwrap().error.as_deref(), Some(DISABLED));
    }
}
