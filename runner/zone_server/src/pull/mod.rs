//! Background Ollama pulls that survive WebSocket disconnects.
//!
//! Each model is a job: layers are streamed as chunks, an interrupted
//! stream is retried (Ollama resumes incomplete blobs), and subscribers
//! can attach, detach, and cancel independently of the download.

use dashmap::DashMap;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, watch};

pub const MISSING_MANIFEST: &str = "pull model manifest: file does not exist";

const JOB_TTL: Duration = Duration::from_secs(60);
const EVENT_CAPACITY: usize = 128;
const MAX_RETRIES: usize = 5;
const RETRY_BACKOFF: [Duration; MAX_RETRIES] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(16),
];

#[derive(Clone)]
pub struct PullRegistry {
    jobs: Arc<DashMap<String, Arc<Job>>>,
}

pub struct Subscription {
    job: Arc<Job>,
    live: broadcast::Receiver<Event>,
}

#[derive(Deserialize)]
pub struct Pull {
    pub model: String,
    #[serde(default)]
    pub cancel: bool,
}

#[derive(Deserialize)]
struct Progress {
    status: Option<String>,
    error: Option<String>,
    total: Option<u64>,
    completed: Option<u64>,
    digest: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Step {
        status: String,
    },
    Progress {
        percent: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        completed: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        digest: Option<String>,
    },
    Complete {
        success: bool,
        message: &'static str,
    },
    Error {
        message: String,
    },
}

impl Event {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Complete { .. } | Self::Error { .. })
    }
}

struct Snapshot {
    steps: Vec<String>,
    percent: Option<f64>,
    completed: Option<u64>,
    total: Option<u64>,
    digest: Option<String>,
    terminal: Option<Event>,
}

struct Job {
    model: String,
    events: broadcast::Sender<Event>,
    snapshot: Mutex<Snapshot>,
    cancel: watch::Sender<bool>,
}

impl PullRegistry {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(DashMap::new()),
        }
    }

    pub fn start_or_attach(&self, host: String, model: String) -> Subscription {
        if let Some(existing) = self.jobs.get(&model)
            && existing
                .snapshot
                .lock()
                .expect("pull snapshot")
                .terminal
                .is_none()
        {
            return Subscription::attach(existing.clone());
        }

        let job = Job::new(model.clone());
        self.jobs.insert(model.clone(), job.clone());
        let jobs = self.jobs.clone();
        let running = job.clone();
        let key = model.clone();
        tokio::spawn(async move {
            run_job(host, key.clone(), running.clone()).await;
            tokio::time::sleep(JOB_TTL).await;
            jobs.remove_if(&key, |_, current| Arc::ptr_eq(current, &running));
        });
        Subscription::attach(job)
    }

    pub fn cancel(&self, model: &str) -> bool {
        self.jobs.get(model).is_some_and(|job| {
            job.request_cancel();
            true
        })
    }
}

impl Default for PullRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Subscription {
    fn attach(job: Arc<Job>) -> Self {
        Self {
            live: job.events.subscribe(),
            job,
        }
    }

    pub fn replay(&self) -> Vec<Event> {
        self.job.replay()
    }

    pub async fn next(&mut self) -> Option<Event> {
        loop {
            match self.live.recv().await {
                Ok(event) => return Some(event),
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    if let Some(terminal) = self.job.terminal() {
                        return Some(terminal);
                    }
                }
                Err(broadcast::error::RecvError::Closed) => return self.job.terminal(),
            }
        }
    }
}

impl Job {
    fn new(model: String) -> Arc<Self> {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let (cancel, _) = watch::channel(false);
        Arc::new(Self {
            model,
            events,
            snapshot: Mutex::new(Snapshot {
                steps: Vec::new(),
                percent: None,
                completed: None,
                total: None,
                digest: None,
                terminal: None,
            }),
            cancel,
        })
    }

    fn request_cancel(&self) {
        let _ = self.cancel.send(true);
    }

    fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }

    async fn cancelled(&self) {
        let mut rx = self.cancel.subscribe();
        if *rx.borrow() {
            return;
        }
        while rx.changed().await.is_ok() {
            if *rx.borrow() {
                return;
            }
        }
    }

    fn publish(&self, event: Event) {
        {
            let mut snapshot = self.snapshot.lock().expect("pull snapshot");
            match &event {
                Event::Step { status } => {
                    if !snapshot.steps.iter().any(|step| step == status) {
                        snapshot.steps.push(status.clone());
                    }
                }
                Event::Progress {
                    percent,
                    completed,
                    total,
                    digest,
                } => {
                    snapshot.percent = Some(*percent);
                    snapshot.completed = *completed;
                    snapshot.total = *total;
                    if digest.is_some() {
                        snapshot.digest = digest.clone();
                    }
                }
                Event::Complete { .. } | Event::Error { .. } => {
                    snapshot.terminal = Some(event.clone());
                }
            }
        }
        let _ = self.events.send(event);
    }

    fn replay(&self) -> Vec<Event> {
        let snapshot = self.snapshot.lock().expect("pull snapshot");
        let mut events: Vec<Event> = snapshot
            .steps
            .iter()
            .cloned()
            .map(|status| Event::Step { status })
            .collect();
        if let Some(percent) = snapshot.percent {
            events.push(Event::Progress {
                percent,
                completed: snapshot.completed,
                total: snapshot.total,
                digest: snapshot.digest.clone(),
            });
        }
        if let Some(terminal) = &snapshot.terminal {
            events.push(terminal.clone());
        }
        events
    }

    fn terminal(&self) -> Option<Event> {
        self.snapshot
            .lock()
            .expect("pull snapshot")
            .terminal
            .clone()
    }
}

fn retryable(message: &str) -> bool {
    message.starts_with("Model download interrupted")
        || message.starts_with("Could not connect to Ollama")
}

fn missing_manifest(model: &str, message: String) -> String {
    if message == MISSING_MANIFEST {
        format!(
            "{message}. Ollama could not find \"{model}\". Use an Ollama model:tag or hf.co/owner/GGUF-repository reference."
        )
    } else {
        message
    }
}

async fn run_job(host: String, model: String, job: Arc<Job>) {
    for attempt in 0..=MAX_RETRIES {
        if job.is_cancelled() {
            job.publish(Event::Error {
                message: "Installation cancelled".to_string(),
            });
            return;
        }
        match download_once(&host, &model, &job).await {
            Ok(()) => return,
            Err(_) if job.is_cancelled() => {
                job.publish(Event::Error {
                    message: "Installation cancelled".to_string(),
                });
                return;
            }
            Err(message) if retryable(&message) && attempt < MAX_RETRIES => {
                job.publish(Event::Step {
                    status: "resuming download".to_string(),
                });
                tokio::select! {
                    () = tokio::time::sleep(RETRY_BACKOFF[attempt]) => {}
                    () = job.cancelled() => {
                        job.publish(Event::Error {
                            message: "Installation cancelled".to_string(),
                        });
                        return;
                    }
                }
            }
            Err(message) => {
                job.publish(Event::Error {
                    message: missing_manifest(&model, message),
                });
                return;
            }
        }
    }
}

async fn download_once(host: &str, model: &str, job: &Job) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| format!("Could not connect to Ollama: {error}"))?;
    let response = client
        .post(format!("{}/api/pull", host.trim_end_matches('/')))
        .json(&serde_json::json!({ "model": model, "stream": true }))
        .send()
        .await
        .map_err(|error| format!("Could not connect to Ollama: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .map_err(|error| format!("Could not read Ollama error: {error}"))?;
        return Err(provider_error(status, &body));
    }

    let mut stream = response.bytes_stream();
    let mut pending = Vec::new();
    loop {
        tokio::select! {
            () = job.cancelled() => return Err("Installation cancelled".to_string()),
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(bytes)) => {
                        pending.extend_from_slice(&bytes);
                        while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                            let line: Vec<u8> = pending.drain(..=end).collect();
                            if process(job, &line)? {
                                return Ok(());
                            }
                        }
                    }
                    Some(Err(error)) => {
                        return Err(format!("Model download interrupted: {error}"));
                    }
                    None => break,
                }
            }
        }
    }
    if !pending.is_empty() && process(job, &pending)? {
        return Ok(());
    }
    Err("Model download ended before installation completed".to_string())
}

fn provider_error(status: reqwest::StatusCode, body: &str) -> String {
    serde_json::from_str::<Progress>(body)
        .ok()
        .and_then(|progress| progress.error)
        .filter(|error| !error.trim().is_empty())
        .unwrap_or_else(|| {
            if body.trim().is_empty() {
                format!("Ollama returned HTTP {status}")
            } else {
                body.to_string()
            }
        })
}

fn process(job: &Job, line: &[u8]) -> Result<bool, String> {
    if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(false);
    }
    let progress: Progress = serde_json::from_slice(line)
        .map_err(|_| "Ollama returned invalid download progress".to_string())?;
    if let Some(error) = progress.error {
        return Err(error);
    }
    if let Some(status) = progress.status {
        if status == "success" {
            job.publish(Event::Complete {
                success: true,
                message: "Model installed successfully",
            });
            return Ok(true);
        }
        let fresh = {
            let snapshot = job.snapshot.lock().expect("pull snapshot");
            !snapshot.steps.iter().any(|step| step == &status)
        };
        if fresh {
            job.publish(Event::Step { status });
        }
    }
    if let (Some(total), Some(completed)) = (progress.total, progress.completed)
        && total > 0
    {
        job.publish(Event::Progress {
            percent: (completed as f64 / total as f64 * 100.0).clamp(0.0, 100.0),
            completed: Some(completed),
            total: Some(total),
            digest: progress.digest,
        });
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::{Event, retryable};

    #[test]
    fn retries_interrupted_layer_streams() {
        assert!(retryable("Model download interrupted: connection reset"));
        assert!(retryable("Could not connect to Ollama: timeout"));
        assert!(!retryable(
            "Model download ended before installation completed"
        ));
        assert!(!retryable("pull model manifest: file does not exist"));
    }

    #[test]
    fn terminal_events() {
        assert!(
            Event::Complete {
                success: true,
                message: "done",
            }
            .is_terminal()
        );
        assert!(
            Event::Error {
                message: "nope".into(),
            }
            .is_terminal()
        );
        assert!(
            !Event::Step {
                status: "pulling".into(),
            }
            .is_terminal()
        );
    }
}
