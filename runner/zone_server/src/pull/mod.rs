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
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub recipe_id: Option<String>,
    #[serde(default)]
    pub hf_base: Option<String>,
}

#[derive(Clone)]
pub struct ComfyPull {
    pub models_dir: std::path::PathBuf,
    pub recipe_id: Option<String>,
    pub hf_base: Option<String>,
    pub hub_origin: String,
}

#[derive(Clone)]
pub struct PullStart {
    pub model: String,
    pub ollama_host: String,
    pub comfy: Option<ComfyPull>,
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
        self.start(PullStart {
            model,
            ollama_host: host,
            comfy: None,
        })
    }

    pub fn start(&self, request: PullStart) -> Subscription {
        let model = request.model.clone();
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
            run_job(request, running.clone()).await;
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
        || message.starts_with("Could not download image weights")
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

async fn run_job(request: PullStart, job: Arc<Job>) {
    if let Some(comfy) = request.comfy.clone() {
        download_comfy(&request.model, &comfy, &job).await;
        return;
    }
    let host = request.ollama_host;
    let model = request.model;
    for backoff in RETRY_BACKOFF.into_iter().map(Some).chain([None]) {
        if job.is_cancelled() {
            job.publish(Event::Error {
                message: "Installation cancelled".to_string(),
            });
            return;
        }
        match (download_once(&host, &model, &job).await, backoff) {
            (Ok(()), _) => return,
            (Err(_), _) if job.is_cancelled() => {
                job.publish(Event::Error {
                    message: "Installation cancelled".to_string(),
                });
                return;
            }
            (Err(message), Some(backoff)) if retryable(&message) => {
                job.publish(Event::Step {
                    status: "resuming download".to_string(),
                });
                tokio::select! {
                    () = tokio::time::sleep(backoff) => {}
                    () = job.cancelled() => {
                        job.publish(Event::Error {
                            message: "Installation cancelled".to_string(),
                        });
                        return;
                    }
                }
            }
            (Err(message), _) => {
                job.publish(Event::Error {
                    message: missing_manifest(&model, message),
                });
                return;
            }
        }
    }
}

fn parse_comfy_ref(model: &str) -> Result<(String, String), String> {
    let trimmed = model.trim();
    let (repo, filename) = trimmed
        .rsplit_once(':')
        .ok_or_else(|| "Image weights need owner/repo:filename.safetensors".to_string())?;
    if repo.is_empty()
        || filename.is_empty()
        || filename.contains('/')
        || filename.contains('\\')
        || filename.contains("..")
        || repo.contains("..")
        || repo.contains('\\')
        || !filename.ends_with(".safetensors")
    {
        return Err("Invalid image weight filename".to_string());
    }
    if repo.split('/').count() != 2
        || repo
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("Image weights need an owner/repo HuggingFace id".to_string());
    }
    Ok((repo.to_string(), filename.to_string()))
}

async fn download_comfy(model: &str, comfy: &ComfyPull, job: &Job) {
    let (repo, filename) = match parse_comfy_ref(model) {
        Ok(parsed) => parsed,
        Err(message) => {
            job.publish(Event::Error { message });
            return;
        }
    };
    job.publish(Event::Step {
        status: "downloading image weights".to_string(),
    });
    let directory = comfy.models_dir.join("loras");
    if let Err(error) = std::fs::create_dir_all(&directory) {
        job.publish(Event::Error {
            message: format!("Could not create loras directory: {error}"),
        });
        return;
    }
    let target = directory.join(&filename);
    let partial = directory.join(format!("{filename}.part"));
    let origin = comfy.hub_origin.trim_end_matches('/');
    let url = format!("{origin}/{repo}/resolve/main/{filename}");
    match download_file(&url, &partial, job).await {
        Ok(()) => {
            if let Err(error) = std::fs::rename(&partial, &target) {
                job.publish(Event::Error {
                    message: format!("Could not store image weights: {error}"),
                });
                return;
            }
            if let Some(recipe_id) = comfy.recipe_id.as_deref() {
                let sidecar = serde_json::json!({
                    "recipe_id": recipe_id,
                    "hf_base": comfy.hf_base,
                });
                let _ = std::fs::write(
                    directory.join(format!("{filename}.zone.json")),
                    sidecar.to_string(),
                );
            }
            job.publish(Event::Complete {
                success: true,
                message: "Model installed successfully",
            });
        }
        Err(message) => {
            job.publish(Event::Error { message });
        }
    }
}

async fn download_file(url: &str, dest: &std::path::Path, job: &Job) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent("ZoneManager/1.0")
        .build()
        .map_err(|error| format!("Could not download image weights: {error}"))?;
    let mut existing = std::fs::metadata(dest).map(|meta| meta.len()).unwrap_or(0);
    let mut request = client.get(url);
    if existing > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={existing}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("Could not download image weights: {error}"))?;
    if response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        return Ok(());
    }
    if response.status() == reqwest::StatusCode::OK {
        existing = 0;
        let _ = std::fs::remove_file(dest);
    } else if !response.status().is_success()
        && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
    {
        return Err(format!(
            "Could not download image weights: HTTP {}",
            response.status()
        ));
    }
    let total = response
        .content_length()
        .map(|length| length + existing)
        .unwrap_or(0);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(existing > 0)
        .write(true)
        .truncate(existing == 0)
        .open(dest)
        .map_err(|error| format!("Could not store image weights: {error}"))?;
    let mut stream = response.bytes_stream();
    let mut completed = existing;
    loop {
        tokio::select! {
            () = job.cancelled() => return Err("Installation cancelled".to_string()),
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(bytes)) => {
                        std::io::Write::write_all(&mut file, &bytes)
                            .map_err(|error| format!("Could not store image weights: {error}"))?;
                        completed += bytes.len() as u64;
                        if total > 0 {
                            job.publish(Event::Progress {
                                percent: (completed as f64 / total as f64 * 100.0).clamp(0.0, 100.0),
                                completed: Some(completed),
                                total: Some(total),
                                digest: None,
                            });
                        }
                    }
                    Some(Err(error)) => {
                        return Err(format!("Could not download image weights: {error}"));
                    }
                    None => break,
                }
            }
        }
    }
    Ok(())
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
    fn parses_owner_repo_safetensors_refs() {
        assert_eq!(
            super::parse_comfy_ref(
                "ScottzillaSystems/qwen-image-edit-plus-nsfw-lora:qwen-image-edit-plus-nsfw-lora.safetensors"
            )
            .unwrap(),
            (
                "ScottzillaSystems/qwen-image-edit-plus-nsfw-lora".into(),
                "qwen-image-edit-plus-nsfw-lora.safetensors".into()
            )
        );
        assert!(super::parse_comfy_ref("llama3.2:3b").is_err());
        assert!(super::parse_comfy_ref("../evil:model.safetensors").is_err());
    }

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
