//! Background Ollama pulls that survive WebSocket disconnects.
//!
//! Each model is a job: layers are streamed as chunks, an interrupted
//! stream is retried (Ollama resumes incomplete blobs), and subscribers
//! can attach, detach, and cancel independently of the download.

mod comfy_reference;
mod partial;
mod transfer_lock;

use abnegate_llm::download::{self, Checksum, DownloadError, DownloadProgress};
use comfy_reference::ComfyReference;
use dashmap::DashMap;
use futures::StreamExt;
use partial::Partial;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, atomic::Ordering};
use std::task::Poll;
use std::time::Duration;
use tokio::sync::{broadcast, watch};
use tokio::time::MissedTickBehavior;

pub const MISSING_MANIFEST: &str = "pull model manifest: file does not exist";

const CANCELLED: &str = "Installation cancelled";
const DOWNLOAD_FAILED: &str = "Could not download image weights";
const STORE_FAILED: &str = "Could not store image weights";
const CHECKSUM_MISMATCH: &str = "Image weight checksum mismatch";
const INVALID_DIGEST: &str = "Image weight manifest has an invalid SHA-256 digest";
const WAITING_FOR_RELEASE: &str = "waiting for another download of these image weights";
const ALREADY_DOWNLOADING: &str = "These image weights are already downloading";
const STAGED_SUFFIX: &str = ".download";
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const RELEASE_WAITS: [Duration; 4] = [
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(400),
    Duration::from_millis(800),
];
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
    pub models_directory: std::path::PathBuf,
    pub recipe_id: Option<String>,
    pub hf_base: Option<String>,
    pub hub_origin: String,
    pub manifest: Option<ManifestFile>,
}

#[derive(Clone)]
pub struct ManifestFile {
    pub url: String,
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
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

    pub fn status(&self, name: &str) -> Option<PullView> {
        let job = self.jobs.get(name)?;
        let snapshot = job.snapshot.lock().expect("pull snapshot");
        Some(PullView {
            name: job.model.clone(),
            percent: snapshot.percent,
            completed: snapshot.completed,
            total: snapshot.total,
            steps: snapshot.steps.clone(),
            terminal: snapshot.terminal.clone(),
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct PullView {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    pub steps: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<Event>,
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
                message: CANCELLED.to_string(),
            });
            return;
        }
        match (download_once(&host, &model, &job).await, backoff) {
            (Ok(()), _) => return,
            (Err(_), _) if job.is_cancelled() => {
                job.publish(Event::Error {
                    message: CANCELLED.to_string(),
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
                            message: CANCELLED.to_string(),
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

async fn download_comfy(model: &str, comfy: &ComfyPull, job: &Job) {
    if let Some(manifest) = &comfy.manifest {
        download_manifest(&comfy.models_directory, manifest, job).await;
        return;
    }
    let reference = match ComfyReference::parse(model) {
        Ok(reference) => reference,
        Err(message) => {
            job.publish(Event::Error { message });
            return;
        }
    };
    let Some(url) = reference.url(&comfy.hub_origin) else {
        job.publish(Event::Error {
            message: format!(
                "{DOWNLOAD_FAILED}: {} is not a usable HuggingFace origin",
                comfy.hub_origin
            ),
        });
        return;
    };
    job.publish(Event::Step {
        status: "downloading image weights".to_string(),
    });
    let directory = comfy.models_directory.join("loras");
    if let Err(error) = std::fs::create_dir_all(&directory) {
        job.publish(Event::Error {
            message: format!("Could not create loras directory: {error}"),
        });
        return;
    }
    let target = directory.join(reference.filename());
    if let Err(message) = fetch_weights(url.as_str(), &target, None, None, job).await {
        job.publish(Event::Error { message });
        return;
    }
    if let Some(recipe_id) = comfy.recipe_id.as_deref() {
        let sidecar = serde_json::json!({
            "recipe_id": recipe_id,
            "hf_base": comfy.hf_base,
        });
        let _ = std::fs::write(
            directory.join(format!("{}.zone.json", reference.filename())),
            sidecar.to_string(),
        );
    }
    job.publish(Event::Complete {
        success: true,
        message: "Model installed successfully",
    });
}

async fn download_manifest(models_directory: &Path, manifest: &ManifestFile, job: &Job) {
    let Some(target) = crate::setup::confined_relative(models_directory, &manifest.relative_path)
    else {
        job.publish(Event::Error {
            message: "Invalid image weight path".to_string(),
        });
        return;
    };
    if std::fs::metadata(&target)
        .ok()
        .is_some_and(|metadata| metadata.is_file() && metadata.len() == manifest.size_bytes)
    {
        job.publish(Event::Complete {
            success: true,
            message: "Model installed successfully",
        });
        return;
    }
    let checksum = match Checksum::from_hexadecimal(&manifest.sha256) {
        Ok(checksum) => checksum,
        Err(error) => {
            job.publish(Event::Error {
                message: describe(error),
            });
            return;
        }
    };
    job.publish(Event::Step {
        status: "downloading image weights".to_string(),
    });
    if let Some(parent) = target.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        job.publish(Event::Error {
            message: format!("Could not create model directory: {error}"),
        });
        return;
    }
    let staged = staged(&target);
    if let Err(error) = Partial::of(&target).move_to(&Partial::of(&staged)) {
        tracing::warn!(
            "Could not carry an interrupted download of {} over: {error}",
            target.display()
        );
    }
    if let Err(message) = fetch_weights(
        &manifest.url,
        &staged,
        Some(checksum),
        Some(manifest.size_bytes),
        job,
    )
    .await
    {
        job.publish(Event::Error { message });
        return;
    }
    if let Err(message) = install(&staged, &target, manifest.size_bytes) {
        job.publish(Event::Error { message });
        return;
    }
    job.publish(Event::Complete {
        success: true,
        message: "Model installed successfully",
    });
}

/// Where a manifest weight is downloaded before it is checked, so a weight
/// of the wrong size never replaces the one already at `target`.
fn staged(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(STAGED_SUFFIX);
    PathBuf::from(name)
}

fn install(staged: &Path, target: &Path, size: u64) -> Result<(), String> {
    let length = std::fs::metadata(staged)
        .map_err(|error| format!("{STORE_FAILED}: {error}"))?
        .len();
    if length != size {
        let _ = std::fs::remove_file(staged);
        return Err(size_mismatch(length, size));
    }
    std::fs::rename(staged, target).map_err(|error| format!("{STORE_FAILED}: {error}"))
}

/// Transfer `url` to `target`, waiting a while for another transfer of the
/// same weight to let go of it. A known `size` ends the transfer as soon as
/// the server announces a different one, rather than downloading all of it.
async fn fetch_weights(
    url: &str,
    target: &Path,
    checksum: Option<Checksum>,
    size: Option<u64>,
    job: &Job,
) -> Result<(), String> {
    for wait in RELEASE_WAITS {
        match transfer_weights(url, target, checksum, size, job).await? {
            Err(DownloadError::InProgress) => {
                job.publish(Event::Step {
                    status: WAITING_FOR_RELEASE.to_string(),
                });
                tokio::select! {
                    () = tokio::time::sleep(wait) => {}
                    () = job.cancelled() => return Err(CANCELLED.to_string()),
                }
            }
            outcome => return outcome.map_err(describe),
        }
    }
    transfer_weights(url, target, checksum, size, job)
        .await?
        .map_err(describe)
}

/// One transfer of `url` to `target`, or the message that ended it early:
/// the job was cancelled, or the server announced a length other than `size`.
async fn transfer_weights(
    url: &str,
    target: &Path,
    checksum: Option<Checksum>,
    size: Option<u64>,
    job: &Job,
) -> Result<Result<(), DownloadError>, String> {
    let progress = Arc::new(DownloadProgress::new());
    let mut transfer = std::pin::pin!(download::fetch(
        url,
        target,
        checksum,
        Arc::clone(&progress)
    ));
    let mut guarded = std::pin::pin!(std::future::poll_fn(|context| {
        match transfer.as_mut().poll(context) {
            Poll::Ready(outcome) => Poll::Ready(Ok(outcome)),
            Poll::Pending => match announced_mismatch(&progress, size) {
                Some(message) => Poll::Ready(Err(message)),
                None => Poll::Pending,
            },
        }
    }));
    let mut pump = tokio::time::interval(PROGRESS_INTERVAL);
    pump.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut reported = None;
    loop {
        tokio::select! {
            outcome = &mut guarded => {
                report_progress(job, &progress, &mut reported);
                return outcome;
            }
            () = job.cancelled() => return Err(CANCELLED.to_string()),
            _ = pump.tick() => report_progress(job, &progress, &mut reported),
        }
    }
}

/// Zone's size-mismatch message once the server has announced a length
/// other than the `expected` one.
fn announced_mismatch(progress: &DownloadProgress, expected: Option<u64>) -> Option<String> {
    let expected = expected?;
    let announced = progress.total_bytes.load(Ordering::Relaxed);
    (announced != 0 && announced != expected).then(|| size_mismatch(announced, expected))
}

fn size_mismatch(actual: u64, expected: u64) -> String {
    format!("Image weight size mismatch: got {actual} bytes, expected {expected}")
}

fn report_progress(job: &Job, progress: &DownloadProgress, reported: &mut Option<(u64, u64)>) {
    let total = progress.total_bytes.load(Ordering::Relaxed);
    let completed = progress.downloaded_bytes.load(Ordering::Relaxed);
    if total == 0 || *reported == Some((completed, total)) {
        return;
    }
    *reported = Some((completed, total));
    job.publish(Event::Progress {
        percent: (completed as f64 / total as f64 * 100.0).clamp(0.0, 100.0),
        completed: Some(completed),
        total: Some(total),
        digest: None,
    });
}

fn describe(error: DownloadError) -> String {
    match error {
        DownloadError::Http(error) => format!("{DOWNLOAD_FAILED}: {error}"),
        DownloadError::Status(code) => format!("{DOWNLOAD_FAILED}: HTTP {}", status_text(code)),
        DownloadError::Io(error) => format!("{STORE_FAILED}: {error}"),
        DownloadError::Checksum { .. } => CHECKSUM_MISMATCH.to_string(),
        DownloadError::InvalidChecksum(_) => INVALID_DIGEST.to_string(),
        DownloadError::InProgress => ALREADY_DOWNLOADING.to_string(),
        error => format!("{DOWNLOAD_FAILED}: {error}"),
    }
}

fn status_text(code: u16) -> String {
    reqwest::StatusCode::from_u16(code)
        .map_or_else(|_| code.to_string(), |status| status.to_string())
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
            () = job.cancelled() => return Err(CANCELLED.to_string()),
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
    use super::{
        ALREADY_DOWNLOADING, CANCELLED, CHECKSUM_MISMATCH, ComfyPull, Event, INVALID_DIGEST,
        ManifestFile, PullRegistry, PullStart, STAGED_SUFFIX, WAITING_FOR_RELEASE, describe,
        retryable,
    };
    use abnegate_llm::download::{Checksum, DownloadError};
    use axum::{
        Router,
        body::{Body, Bytes},
        http::{HeaderMap, StatusCode, header},
        response::Response,
        routing::get,
    };
    use futures::StreamExt;
    use sha2::{Digest, Sha256};
    use std::fs::File;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::net::TcpListener;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{self, method, path},
    };

    const BODY: &[u8] = b"zone-setup-weight-bytes-for-a-resumable-transfer";
    const HEAD_LENGTH: usize = 16;
    const ENTITY_TAG: &str = "\"weight-v1\"";
    const RELATIVE_PATH: &str = "checkpoints/tiny.safetensors";
    const POLL: Duration = Duration::from_millis(20);
    const POLLS: usize = 250;
    /// abnegate-llm locks a transfer through this sibling of its `.part`
    /// file, and does not export the name.
    const TRANSFER_LOCK_SUFFIX: &str = ".lock";

    fn sha256(body: &[u8]) -> String {
        Checksum::new(Sha256::digest(body).into()).to_string()
    }

    fn manifest_pull(directory: &Path, url: String, size_bytes: u64, sha256: String) -> PullStart {
        PullStart {
            model: "tiny".into(),
            ollama_host: String::new(),
            comfy: Some(ComfyPull {
                models_directory: directory.to_path_buf(),
                recipe_id: None,
                hf_base: None,
                hub_origin: "http://127.0.0.1:1".into(),
                manifest: Some(ManifestFile {
                    url,
                    relative_path: RELATIVE_PATH.into(),
                    size_bytes,
                    sha256,
                }),
            }),
        }
    }

    fn body_pull(directory: &Path, url: String) -> PullStart {
        manifest_pull(directory, url, BODY.len() as u64, sha256(BODY))
    }

    fn staged(directory: &Path) -> PathBuf {
        directory.join(format!("{RELATIVE_PATH}{STAGED_SUFFIX}"))
    }

    fn part(directory: &Path) -> PathBuf {
        directory.join(format!("{RELATIVE_PATH}{STAGED_SUFFIX}.part"))
    }

    async fn terminal(registry: &PullRegistry, model: &str) -> Event {
        for _ in 0..POLLS {
            if let Some(terminal) = registry.status(model).and_then(|view| view.terminal) {
                return terminal;
            }
            tokio::time::sleep(POLL).await;
        }
        panic!("pull did not finish");
    }

    async fn assert_complete(registry: &PullRegistry, model: &str) {
        match terminal(registry, model).await {
            Event::Complete { success, .. } => assert!(success),
            other => panic!("download failed: {other:?}"),
        }
    }

    async fn assert_error(registry: &PullRegistry, model: &str) -> String {
        match terminal(registry, model).await {
            Event::Error { message } => message,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    async fn wait_for_progress(registry: &PullRegistry, model: &str, completed: usize) {
        for _ in 0..POLLS {
            if registry
                .status(model)
                .is_some_and(|view| view.completed == Some(completed as u64))
            {
                return;
            }
            tokio::time::sleep(POLL).await;
        }
        panic!("{model} never reported {completed} bytes");
    }

    fn transfer_lock(part: &Path) -> PathBuf {
        PathBuf::from(format!("{}{TRANSFER_LOCK_SUFFIX}", part.display()))
    }

    fn hold_transfer_lock(directory: &Path) -> File {
        let partial = part(directory);
        std::fs::create_dir_all(partial.parent().unwrap()).unwrap();
        let held = File::create(transfer_lock(&partial)).unwrap();
        held.lock().unwrap();
        held
    }

    async fn wait_for_step(registry: &PullRegistry, model: &str, step: &str) {
        for _ in 0..POLLS {
            if registry
                .status(model)
                .is_some_and(|view| view.steps.iter().any(|seen| seen == step))
            {
                return;
            }
            tokio::time::sleep(POLL).await;
        }
        panic!("{model} never reported {step:?}");
    }

    async fn wait_for_release(part: &Path) {
        let lock = transfer_lock(part);
        for _ in 0..POLLS {
            if !lock.exists() {
                return;
            }
            tokio::time::sleep(POLL).await;
        }
        panic!("{} was never released", lock.display());
    }

    async fn serve(router: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{address}")
    }

    fn weight(
        status: StatusCode,
        headers: &[(header::HeaderName, String)],
        body: Body,
    ) -> Response {
        let mut response = Response::builder().status(status);
        for (name, value) in headers {
            response = response.header(name, value);
        }
        response.body(body).unwrap()
    }

    fn head_then<Tail>(tail: Tail) -> Body
    where
        Tail: futures::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    {
        let head = futures::stream::once(async {
            Ok::<_, std::io::Error>(Bytes::from_static(&BODY[..HEAD_LENGTH]))
        });
        Body::from_stream(head.chain(tail))
    }

    async fn requests_to(server: &MockServer) -> Vec<wiremock::Request> {
        server.received_requests().await.unwrap_or_default()
    }

    fn legacy_part(directory: &Path) -> PathBuf {
        directory.join(format!("{RELATIVE_PATH}.part"))
    }

    fn validator(part: &Path) -> PathBuf {
        PathBuf::from(format!("{}.validator", part.display()))
    }

    fn seed_partial(part: &Path, bytes: &[u8], url: Option<&str>) {
        std::fs::create_dir_all(part.parent().unwrap()).unwrap();
        std::fs::write(part, bytes).unwrap();
        if let Some(url) = url {
            let stored = serde_json::json!({ "url": url, "entity_tag": ENTITY_TAG });
            std::fs::write(validator(part), stored.to_string()).unwrap();
        }
    }

    /// Serves `BODY` whole, or its tail from `HEAD_LENGTH` to a request for
    /// that range guarded by `ENTITY_TAG`.
    async fn ranged_origin() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(matchers::header(
                header::RANGE.as_str(),
                format!("bytes={HEAD_LENGTH}-").as_str(),
            ))
            .and(matchers::header(header::IF_RANGE.as_str(), ENTITY_TAG))
            .respond_with(
                ResponseTemplate::new(StatusCode::PARTIAL_CONTENT.as_u16())
                    .insert_header(header::ETAG.as_str(), ENTITY_TAG)
                    .insert_header(
                        header::CONTENT_RANGE.as_str(),
                        format!("bytes {HEAD_LENGTH}-{}/{}", BODY.len() - 1, BODY.len()).as_str(),
                    )
                    .set_body_bytes(&BODY[HEAD_LENGTH..]),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK.as_u16())
                    .insert_header(header::ETAG.as_str(), ENTITY_TAG)
                    .set_body_bytes(BODY),
            )
            .mount(&server)
            .await;
        server
    }

    fn assert_no_legacy_partial(directory: &Path) {
        let part = legacy_part(directory);
        for leftover in [part.clone(), validator(&part), transfer_lock(&part)] {
            assert!(
                !leftover.exists(),
                "{} outlived the pull",
                leftover.display()
            );
        }
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
    fn a_failed_image_weight_download_is_not_an_ollama_retry() {
        assert!(!retryable(&describe(DownloadError::Status(503))));
    }

    #[tokio::test]
    async fn status_reads_the_live_snapshot() {
        let registry = super::PullRegistry::new();
        assert!(registry.status("llama3.2").is_none());
        let _subscription =
            registry.start_or_attach("http://127.0.0.1:1".into(), "llama3.2".into());
        let view = registry.status("llama3.2").expect("the job is attached");
        assert_eq!(view.name, "llama3.2");
        assert!(view.terminal.is_none());
        assert!(registry.cancel("llama3.2"));
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

    #[test]
    fn download_errors_read_as_zone_messages() {
        assert_eq!(
            describe(DownloadError::Status(404)),
            "Could not download image weights: HTTP 404 Not Found"
        );
        assert_eq!(
            describe(DownloadError::Io(std::io::Error::other("disk full"))),
            "Could not store image weights: disk full"
        );
        assert_eq!(
            describe(DownloadError::InvalidChecksum("deadbeef".into())),
            INVALID_DIGEST
        );
        assert_eq!(describe(DownloadError::InProgress), ALREADY_DOWNLOADING);
        assert_eq!(
            describe(DownloadError::Inconsistent),
            format!(
                "Could not download image weights: {}",
                DownloadError::Inconsistent
            )
        );
    }

    #[tokio::test]
    async fn skips_manifest_weight_when_size_matches() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join(RELATIVE_PATH);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"already-there").unwrap();
        let registry = PullRegistry::new();
        let _subscription = registry.start(manifest_pull(
            directory.path(),
            "http://127.0.0.1:1/missing.bin".into(),
            13,
            "deadbeef".into(),
        ));
        assert_complete(&registry, "tiny").await;
    }

    #[tokio::test]
    async fn downloads_manifest_weight_into_relative_path() {
        let payload = BODY.to_vec();
        let origin = serve(Router::new().route(
            "/file.bin",
            get(move || {
                let payload = payload.clone();
                async move { payload }
            }),
        ))
        .await;

        let directory = tempfile::tempdir().unwrap();
        let registry = PullRegistry::new();
        let _subscription =
            registry.start(body_pull(directory.path(), format!("{origin}/file.bin")));
        assert_complete(&registry, "tiny").await;
        let stored = std::fs::read(directory.path().join(RELATIVE_PATH)).unwrap();
        assert_eq!(stored, BODY);
        assert!(!part(directory.path()).exists());
        assert!(!staged(directory.path()).exists());
    }

    #[tokio::test]
    async fn downloads_a_lora_reference_into_loras_with_its_sidecar() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/owner/style/resolve/main/style.safetensors"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let registry = PullRegistry::new();
        let _subscription = registry.start(PullStart {
            model: "owner/style:style.safetensors".into(),
            ollama_host: String::new(),
            comfy: Some(ComfyPull {
                models_directory: directory.path().to_path_buf(),
                recipe_id: Some("flux-lora".into()),
                hf_base: Some("black-forest-labs/FLUX.1-schnell".into()),
                hub_origin: format!("{}/", server.uri()),
                manifest: None,
            }),
        });
        assert_complete(&registry, "owner/style:style.safetensors").await;
        let loras = directory.path().join("loras");
        assert_eq!(
            std::fs::read(loras.join("style.safetensors")).unwrap(),
            BODY
        );
        let sidecar: serde_json::Value = serde_json::from_slice(
            &std::fs::read(loras.join("style.safetensors.zone.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(sidecar["recipe_id"], "flux-lora");
        assert_eq!(sidecar["hf_base"], "black-forest-labs/FLUX.1-schnell");
    }

    #[tokio::test]
    async fn saves_a_lora_filename_with_spaces_under_its_own_name() {
        const MODEL: &str = "owner/style:My Lora (v2).safetensors";
        const FILENAME: &str = "My Lora (v2).safetensors";
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/owner/style/resolve/main/My%20Lora%20(v2).safetensors",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let registry = PullRegistry::new();
        let _subscription = registry.start(PullStart {
            model: MODEL.into(),
            ollama_host: String::new(),
            comfy: Some(ComfyPull {
                models_directory: directory.path().to_path_buf(),
                recipe_id: Some("flux-lora".into()),
                hf_base: None,
                hub_origin: format!("{}/", server.uri()),
                manifest: None,
            }),
        });
        assert_complete(&registry, MODEL).await;
        let loras = directory.path().join("loras");
        assert_eq!(std::fs::read(loras.join(FILENAME)).unwrap(), BODY);
        assert!(loras.join(format!("{FILENAME}.zone.json")).is_file());
        let mut saved: Vec<String> = std::fs::read_dir(&loras)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        saved.sort();
        assert_eq!(
            saved,
            [FILENAME.to_string(), format!("{FILENAME}.zone.json")]
        );
    }

    #[tokio::test]
    async fn resumes_an_interrupted_body_with_if_range() {
        let seen: Arc<Mutex<Vec<HeaderMap>>> = Arc::default();
        let recorded = Arc::clone(&seen);
        let origin = serve(Router::new().route(
            "/file.bin",
            get(move |headers: HeaderMap| {
                let recorded = Arc::clone(&recorded);
                async move {
                    let range = headers
                        .get(header::RANGE)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.strip_prefix("bytes="))
                        .and_then(|value| value.strip_suffix('-'))
                        .and_then(|value| value.parse::<usize>().ok());
                    recorded.lock().unwrap().push(headers);
                    let length = BODY.len();
                    match range {
                        None => weight(
                            StatusCode::OK,
                            &[
                                (header::ETAG, ENTITY_TAG.to_string()),
                                (header::CONTENT_LENGTH, length.to_string()),
                            ],
                            head_then(futures::stream::once(async {
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                Err(std::io::Error::other("connection dropped"))
                            })),
                        ),
                        Some(start) => weight(
                            StatusCode::PARTIAL_CONTENT,
                            &[
                                (header::ETAG, ENTITY_TAG.to_string()),
                                (
                                    header::CONTENT_RANGE,
                                    format!("bytes {start}-{}/{length}", length - 1),
                                ),
                            ],
                            Body::from(BODY[start..].to_vec()),
                        ),
                    }
                }
            }),
        ))
        .await;

        let directory = tempfile::tempdir().unwrap();
        let registry = PullRegistry::new();
        let url = format!("{origin}/file.bin");
        let _first = registry.start(body_pull(directory.path(), url.clone()));
        let interrupted = assert_error(&registry, "tiny").await;
        assert!(
            interrupted.starts_with("Could not download image weights: "),
            "{interrupted}"
        );
        let kept = std::fs::metadata(part(directory.path())).unwrap().len();
        assert_eq!(kept, HEAD_LENGTH as u64);

        let _second = registry.start(body_pull(directory.path(), url));
        assert_complete(&registry, "tiny").await;
        assert_eq!(
            std::fs::read(directory.path().join(RELATIVE_PATH)).unwrap(),
            BODY
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[0].get(header::RANGE).is_none());
        assert_eq!(seen[1][header::RANGE], format!("bytes={kept}-"));
        assert_eq!(seen[1][header::IF_RANGE], ENTITY_TAG);
    }

    #[tokio::test]
    async fn a_checksum_mismatch_installs_nothing() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let registry = PullRegistry::new();
        let _subscription = registry.start(manifest_pull(
            directory.path(),
            format!("{}/file.bin", server.uri()),
            BODY.len() as u64,
            sha256(b"some other weight"),
        ));
        assert_eq!(assert_error(&registry, "tiny").await, CHECKSUM_MISMATCH);
        assert!(!directory.path().join(RELATIVE_PATH).exists());
        assert!(!part(directory.path()).exists());
    }

    #[tokio::test]
    async fn a_malformed_digest_is_refused_before_any_request() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let registry = PullRegistry::new();
        let _subscription = registry.start(manifest_pull(
            directory.path(),
            format!("{}/file.bin", server.uri()),
            BODY.len() as u64,
            "deadbeef".into(),
        ));
        assert_eq!(assert_error(&registry, "tiny").await, INVALID_DIGEST);
        assert!(requests_to(&server).await.is_empty());
        assert!(!directory.path().join(RELATIVE_PATH).exists());
    }

    #[tokio::test]
    async fn an_unannounced_size_mismatch_removes_the_weight() {
        let origin = serve(Router::new().route(
            "/file.bin",
            get(|| async {
                Body::from_stream(futures::stream::once(async {
                    Ok::<_, std::io::Error>(Bytes::from_static(BODY))
                }))
            }),
        ))
        .await;
        let directory = tempfile::tempdir().unwrap();
        let registry = PullRegistry::new();
        let expected = BODY.len() as u64 + 1;
        let _subscription = registry.start(manifest_pull(
            directory.path(),
            format!("{origin}/file.bin"),
            expected,
            sha256(BODY),
        ));
        assert_eq!(
            assert_error(&registry, "tiny").await,
            format!(
                "Image weight size mismatch: got {} bytes, expected {expected}",
                BODY.len()
            )
        );
        assert!(!directory.path().join(RELATIVE_PATH).exists());
        assert!(!staged(directory.path()).exists());
    }

    #[tokio::test]
    async fn an_unannounced_size_mismatch_keeps_the_installed_weight() {
        const INSTALLED: &[u8] = b"the weight installed before the manifest changed";
        let origin = serve(Router::new().route(
            "/file.bin",
            get(|| async {
                Body::from_stream(futures::stream::once(async {
                    Ok::<_, std::io::Error>(Bytes::from_static(BODY))
                }))
            }),
        ))
        .await;
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join(RELATIVE_PATH);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, INSTALLED).unwrap();
        let registry = PullRegistry::new();
        let _subscription = registry.start(manifest_pull(
            directory.path(),
            format!("{origin}/file.bin"),
            BODY.len() as u64 + 1,
            sha256(BODY),
        ));
        let message = assert_error(&registry, "tiny").await;
        assert!(
            message.starts_with("Image weight size mismatch: "),
            "{message}"
        );
        assert_eq!(std::fs::read(&target).unwrap(), INSTALLED);
        assert!(!staged(directory.path()).exists());
    }

    #[tokio::test]
    async fn an_announced_length_other_than_the_manifests_installs_nothing() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let registry = PullRegistry::new();
        let expected = BODY.len() as u64 * 2;
        let _subscription = registry.start(manifest_pull(
            directory.path(),
            format!("{}/file.bin", server.uri()),
            expected,
            sha256(BODY),
        ));
        assert_eq!(
            assert_error(&registry, "tiny").await,
            format!(
                "Image weight size mismatch: got {} bytes, expected {expected}",
                BODY.len()
            )
        );
        wait_for_release(&part(directory.path())).await;
        assert!(!directory.path().join(RELATIVE_PATH).exists());
    }

    #[tokio::test]
    async fn an_announced_length_mismatch_keeps_the_installed_weight() {
        const INSTALLED: &[u8] = b"the weight installed before the manifest changed";
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join(RELATIVE_PATH);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, INSTALLED).unwrap();
        let registry = PullRegistry::new();
        let _subscription = registry.start(manifest_pull(
            directory.path(),
            format!("{}/file.bin", server.uri()),
            BODY.len() as u64 + 1,
            sha256(BODY),
        ));
        assert!(
            assert_error(&registry, "tiny")
                .await
                .starts_with("Image weight size mismatch: ")
        );
        wait_for_release(&part(directory.path())).await;
        assert_eq!(std::fs::read(&target).unwrap(), INSTALLED);
    }

    #[tokio::test]
    async fn a_failing_status_is_reported_with_its_reason() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let registry = PullRegistry::new();
        let _subscription = registry.start(body_pull(
            directory.path(),
            format!("{}/file.bin", server.uri()),
        ));
        assert_eq!(
            assert_error(&registry, "tiny").await,
            "Could not download image weights: HTTP 404 Not Found"
        );
    }

    #[tokio::test]
    async fn cancelling_mid_body_keeps_the_partial_file() {
        let origin = serve(Router::new().route(
            "/file.bin",
            get(|| async {
                weight(
                    StatusCode::OK,
                    &[
                        (header::ETAG, ENTITY_TAG.to_string()),
                        (header::CONTENT_LENGTH, BODY.len().to_string()),
                    ],
                    head_then(futures::stream::pending()),
                )
            }),
        ))
        .await;
        let directory = tempfile::tempdir().unwrap();
        let registry = PullRegistry::new();
        let _subscription =
            registry.start(body_pull(directory.path(), format!("{origin}/file.bin")));
        wait_for_progress(&registry, "tiny", HEAD_LENGTH).await;

        assert!(registry.cancel("tiny"));

        assert_eq!(assert_error(&registry, "tiny").await, CANCELLED);
        wait_for_release(&part(directory.path())).await;
        assert_eq!(
            std::fs::metadata(part(directory.path())).unwrap().len(),
            HEAD_LENGTH as u64
        );
        assert!(!directory.path().join(RELATIVE_PATH).exists());
    }

    #[tokio::test]
    async fn a_second_transfer_to_the_same_weight_is_refused() {
        let origin = serve(Router::new().route(
            "/file.bin",
            get(|| async {
                weight(
                    StatusCode::OK,
                    &[(header::CONTENT_LENGTH, BODY.len().to_string())],
                    head_then(futures::stream::pending()),
                )
            }),
        ))
        .await;
        let directory = tempfile::tempdir().unwrap();
        let registry = PullRegistry::new();
        let url = format!("{origin}/file.bin");
        let _first = registry.start(body_pull(directory.path(), url.clone()));
        wait_for_progress(&registry, "tiny", HEAD_LENGTH).await;

        let _second = registry.start(PullStart {
            model: "tiny-again".into(),
            ..body_pull(directory.path(), url)
        });

        assert_eq!(
            assert_error(&registry, "tiny-again").await,
            ALREADY_DOWNLOADING
        );
        assert!(registry.cancel("tiny"));
    }

    #[tokio::test]
    async fn a_transfer_waits_for_a_cancelled_one_to_release_the_weight() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let draining = hold_transfer_lock(directory.path());
        let registry = PullRegistry::new();
        let _subscription = registry.start(body_pull(
            directory.path(),
            format!("{}/file.bin", server.uri()),
        ));
        wait_for_step(&registry, "tiny", WAITING_FOR_RELEASE).await;
        assert!(
            registry
                .status("tiny")
                .is_some_and(|view| view.terminal.is_none()),
            "a held weight ended the pull before it was released"
        );

        drop(draining);

        assert_complete(&registry, "tiny").await;
        assert_eq!(
            std::fs::read(directory.path().join(RELATIVE_PATH)).unwrap(),
            BODY
        );
    }

    #[tokio::test]
    async fn cancelling_while_waiting_for_a_held_weight_ends_the_pull() {
        let directory = tempfile::tempdir().unwrap();
        let _held = hold_transfer_lock(directory.path());
        let registry = PullRegistry::new();
        let _subscription = registry.start(body_pull(
            directory.path(),
            "http://127.0.0.1:1/file.bin".into(),
        ));
        wait_for_step(&registry, "tiny", WAITING_FOR_RELEASE).await;

        assert!(registry.cancel("tiny"));

        assert_eq!(assert_error(&registry, "tiny").await, CANCELLED);
    }

    #[tokio::test]
    async fn progress_reports_completed_and_total() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let registry = PullRegistry::new();
        let mut subscription = registry.start(body_pull(
            directory.path(),
            format!("{}/file.bin", server.uri()),
        ));
        let mut events = Vec::new();
        while let Some(event) = subscription.next().await {
            let finished = event.is_terminal();
            events.push(event);
            if finished {
                break;
            }
        }
        let total = BODY.len() as u64;
        let progress: Vec<(Option<u64>, Option<u64>)> = events
            .iter()
            .filter_map(|event| match event {
                Event::Progress {
                    completed, total, ..
                } => Some((*completed, *total)),
                _ => None,
            })
            .collect();
        assert_eq!(
            progress.last(),
            Some(&(Some(total), Some(total))),
            "{events:?}"
        );
        assert!(matches!(
            events.last(),
            Some(Event::Complete { success: true, .. })
        ));
    }

    #[tokio::test]
    async fn a_redirect_is_followed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/moved.bin"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/file.bin", server.uri()).as_str()),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/file.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let registry = PullRegistry::new();
        let _subscription = registry.start(body_pull(
            directory.path(),
            format!("{}/moved.bin", server.uri()),
        ));
        assert_complete(&registry, "tiny").await;
        assert_eq!(
            std::fs::read(directory.path().join(RELATIVE_PATH)).unwrap(),
            BODY
        );
    }

    #[tokio::test]
    async fn a_partial_file_without_a_validator_restarts() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let partial = part(directory.path());
        std::fs::create_dir_all(partial.parent().unwrap()).unwrap();
        std::fs::write(&partial, b"bytes from before the switch").unwrap();
        let registry = PullRegistry::new();
        let _subscription = registry.start(body_pull(
            directory.path(),
            format!("{}/file.bin", server.uri()),
        ));
        assert_complete(&registry, "tiny").await;
        assert_eq!(
            std::fs::read(directory.path().join(RELATIVE_PATH)).unwrap(),
            BODY
        );
        let requests = requests_to(&server).await;
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].headers.contains_key(header::RANGE));
    }

    #[tokio::test]
    async fn resumes_a_download_interrupted_before_staging() {
        let server = ranged_origin().await;
        let url = format!("{}/file.bin", server.uri());
        let directory = tempfile::tempdir().unwrap();
        seed_partial(
            &legacy_part(directory.path()),
            &BODY[..HEAD_LENGTH],
            Some(&url),
        );
        let registry = PullRegistry::new();

        let _subscription = registry.start(body_pull(directory.path(), url));

        assert_complete(&registry, "tiny").await;
        assert_eq!(
            std::fs::read(directory.path().join(RELATIVE_PATH)).unwrap(),
            BODY
        );
        let requests = requests_to(&server).await;
        assert_eq!(requests.len(), 1, "the resume fetched only the tail");
        assert_eq!(
            requests[0].headers[header::RANGE],
            format!("bytes={HEAD_LENGTH}-")
        );
        assert_eq!(requests[0].headers[header::IF_RANGE], ENTITY_TAG);
        assert_no_legacy_partial(directory.path());
        assert!(!part(directory.path()).exists());
    }

    #[tokio::test]
    async fn an_unresumable_download_from_before_staging_is_removed() {
        let server = ranged_origin().await;
        let url = format!("{}/file.bin", server.uri());
        let directory = tempfile::tempdir().unwrap();
        let legacy = legacy_part(directory.path());
        seed_partial(&legacy, &BODY[..HEAD_LENGTH], None);
        std::fs::write(transfer_lock(&legacy), b"").unwrap();
        let registry = PullRegistry::new();

        let _subscription = registry.start(body_pull(directory.path(), url));

        assert_complete(&registry, "tiny").await;
        assert_eq!(
            std::fs::read(directory.path().join(RELATIVE_PATH)).unwrap(),
            BODY
        );
        let requests = requests_to(&server).await;
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].headers.contains_key(header::RANGE));
        assert_no_legacy_partial(directory.path());
    }

    #[tokio::test]
    async fn a_download_from_before_staging_yields_to_a_staged_one() {
        let server = ranged_origin().await;
        let url = format!("{}/file.bin", server.uri());
        let directory = tempfile::tempdir().unwrap();
        seed_partial(&legacy_part(directory.path()), b"stale", Some(&url));
        seed_partial(&part(directory.path()), &BODY[..HEAD_LENGTH], Some(&url));
        let registry = PullRegistry::new();

        let _subscription = registry.start(body_pull(directory.path(), url));

        assert_complete(&registry, "tiny").await;
        assert_eq!(
            std::fs::read(directory.path().join(RELATIVE_PATH)).unwrap(),
            BODY
        );
        let requests = requests_to(&server).await;
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].headers[header::RANGE],
            format!("bytes={HEAD_LENGTH}-")
        );
        assert_no_legacy_partial(directory.path());
    }

    #[tokio::test]
    async fn listing_ignores_an_unfinished_transfer() {
        let directory = tempfile::tempdir().unwrap();
        let checkpoints = directory.path().join("checkpoints");
        std::fs::create_dir_all(&checkpoints).unwrap();
        let weight = "flux1-schnell-fp8.safetensors";
        for suffix in [
            ".part".to_string(),
            ".part.validator".to_string(),
            format!(".part{TRANSFER_LOCK_SUFFIX}"),
            STAGED_SUFFIX.to_string(),
            format!("{STAGED_SUFFIX}.part"),
        ] {
            std::fs::write(checkpoints.join(format!("{weight}{suffix}")), b"partial").unwrap();
        }
        let mut config = crate::state::test_config();
        config.comfyui.models_directory = directory.path().to_path_buf();
        let state = crate::state::AppState::new(
            config,
            sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap(),
            None,
        );
        state.disable_mcp();

        assert!(crate::routes::models::list_comfy_models(&state).is_empty());

        std::fs::write(checkpoints.join(weight), b"weight").unwrap();
        let listed: Vec<String> = crate::routes::models::list_comfy_models(&state)
            .into_iter()
            .map(|model| model.entry.name)
            .collect();
        assert_eq!(listed, [weight]);
    }
}
