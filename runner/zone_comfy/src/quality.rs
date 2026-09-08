//! Scores a trained adapter against its own base and promotes the best checkpoint.

use crate::config::Config;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::Instant;
use uuid::Uuid;

const PACKAGED_TRAIN_CONFIG: &str =
    include_str!("../../../comfyui/custom_nodes/zone_lora/train_config.json");

const RANK_PERCENT: &str = "0.5";
const RANK_IMAGES: usize = 4;
const MEASURE_PERCENTS: &str = "0.2,0.6,0.9";
const PROBE_SEED: u64 = 1234;
const MIN_WEIGHT_BYTES: usize = 10_000;
const FINAL: &str = "final";

#[derive(Debug, Deserialize)]
struct Settings {
    resolution: u32,
    max_steps: u32,
    #[serde(default)]
    checkpoint_every: u32,
    #[serde(default = "default_checkpoints_per_run")]
    checkpoints_per_run: u32,
}

fn default_checkpoints_per_run() -> u32 {
    8
}

/// How much better an adapter fits its own training images than its base does.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Quality {
    pub improvement: f32,
    pub checkpoint: String,
    pub measured: bool,
}

struct Candidate {
    label: String,
    lora: String,
    staged: Option<PathBuf>,
    mean: f64,
}

/// Ranks every checkpoint the run left behind, promotes the winner over
/// `output`, and deletes the rest. `None` whenever ComfyUI could not be asked,
/// which leaves the trained adapter exactly where the trainer wrote it.
pub async fn select(
    config: &Config,
    folder: &str,
    output: &Path,
    captions: &HashMap<String, String>,
) -> Option<Quality> {
    let selection = Selection::new(config, folder, output, captions)?;
    let deadline = Instant::now() + Duration::from_secs(config.train_timeout_secs);
    let sample = subsample(config, folder, RANK_IMAGES);
    let quality = selection.choose(sample.as_deref(), deadline).await;
    discard(config, sample.as_deref());
    selection.sweep();
    quality
}

struct Selection<'a> {
    probe: Probe<'a>,
    settings: Settings,
    folder: String,
    output: PathBuf,
    adapter: String,
    stem: String,
    loras: PathBuf,
    images: usize,
}

impl<'a> Selection<'a> {
    fn new(
        config: &'a Config,
        folder: &str,
        output: &Path,
        captions: &HashMap<String, String>,
    ) -> Option<Self> {
        let settings: Settings = serde_json::from_str(PACKAGED_TRAIN_CONFIG).ok()?;
        let adapter = output.file_name()?.to_str()?.to_string();
        Some(Self {
            probe: Probe::new(config, captions, settings.resolution)?,
            settings,
            folder: folder.to_string(),
            output: output.to_path_buf(),
            stem: adapter.trim_end_matches(".safetensors").to_string(),
            adapter,
            loras: output.parent()?.to_path_buf(),
            images: captions.len(),
        })
    }

    async fn choose(&self, sample: Option<&str>, deadline: Instant) -> Option<Quality> {
        let ranking = sample.unwrap_or(&self.folder);
        let base = self
            .probe
            .mean(ranking, None, RANK_PERCENT, deadline)
            .await?;
        if base <= 0.0 {
            return None;
        }
        let mut best = Candidate {
            label: FINAL.to_string(),
            lora: self.adapter.clone(),
            staged: None,
            mean: self
                .probe
                .mean(ranking, Some(&self.adapter), RANK_PERCENT, deadline)
                .await?,
        };
        for step in self.candidates() {
            if Instant::now() >= deadline {
                break;
            }
            let name = format!("{}-step{step}.safetensors", self.stem);
            let staged = self.loras.join(&name);
            if !self.probe.stage(&name, &staged).await {
                continue;
            }
            match self
                .probe
                .mean(ranking, Some(&name), RANK_PERCENT, deadline)
                .await
            {
                Some(mean) if mean < best.mean => {
                    if let Some(beaten) = best.staged.take() {
                        let _ = fs::remove_file(beaten);
                    }
                    best = Candidate {
                        label: format!("step{step}"),
                        lora: name,
                        staged: Some(staged),
                        mean,
                    };
                }
                _ => {
                    let _ = fs::remove_file(&staged);
                }
            }
        }
        let (base, winner, measured) = match self.measure(&best, deadline).await {
            Some((base, winner)) if base > 0.0 => (base, winner, true),
            _ => (base, best.mean, false),
        };
        if let Some(staged) = &best.staged
            && fs::rename(staged, &self.output).is_err()
        {
            return None;
        }
        Some(Quality {
            improvement: ((base - winner) / base) as f32,
            checkpoint: best.label,
            measured,
        })
    }

    /// The names the run actually wrote, read from ComfyUI's output folder when
    /// that folder is on this machine. Only a remote server falls back to
    /// deriving them, where a policy change in the node would go unseen.
    fn candidates(&self) -> Vec<u32> {
        if let Some(written) = self.written()
            && !written.is_empty()
        {
            return written;
        }
        let Some(steps) = steps(self.images) else {
            return Vec::new();
        };
        let interval = interval(&self.settings, steps);
        if interval == 0 {
            return Vec::new();
        }
        (1..)
            .map(|multiple| multiple * interval)
            .take_while(|step| *step < self.settings.max_steps)
            .collect()
    }

    fn written(&self) -> Option<Vec<u32>> {
        let prefix = format!("{}-step", self.stem);
        let mut steps: Vec<u32> = fs::read_dir(produced(self.probe.config)?)
            .ok()?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter_map(|name| {
                name.strip_prefix(&prefix)?
                    .strip_suffix(".safetensors")?
                    .parse()
                    .ok()
            })
            .collect();
        steps.sort_unstable();
        Some(steps)
    }

    /// Runs before promotion so the winner is still measured under its own
    /// filename: ComfyUI keeps one loader instance per node id and remembers the
    /// last weights it read from a path, so a name whose bytes just changed can
    /// come back scored as the file it replaced.
    async fn measure(&self, best: &Candidate, deadline: Instant) -> Option<(f64, f64)> {
        let base = self
            .probe
            .mean(&self.folder, None, MEASURE_PERCENTS, deadline)
            .await?;
        let winner = self
            .probe
            .mean(&self.folder, Some(&best.lora), MEASURE_PERCENTS, deadline)
            .await?;
        Some((base, winner))
    }

    fn sweep(&self) {
        let prefix = format!("{}-step", self.stem);
        let directories = [Some(self.loras.clone()), produced(self.probe.config)];
        for directory in directories.into_iter().flatten() {
            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if name.starts_with(&prefix) && name.ends_with(".safetensors") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
}

struct Probe<'a> {
    config: &'a Config,
    client: reqwest::Client,
    captions: String,
    resolution: u32,
}

impl<'a> Probe<'a> {
    fn new(
        config: &'a Config,
        captions: &HashMap<String, String>,
        resolution: u32,
    ) -> Option<Self> {
        Some(Self {
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(config.train_timeout_secs))
                .build()
                .ok()?,
            config,
            captions: serde_json::to_string(captions).ok()?,
            resolution,
        })
    }

    async fn mean(
        &self,
        folder: &str,
        lora: Option<&str>,
        percents: &str,
        deadline: Instant,
    ) -> Option<f64> {
        let graph = graph(
            &self.config.checkpoint,
            folder,
            &self.captions,
            self.resolution,
            lora,
            percents,
        );
        let queued: Value = self
            .authorize(self.client.post(format!("{}/prompt", self.config.base_url)))
            .json(&json!({ "prompt": graph }))
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()?;
        if queued.get("error").is_some_and(|error| !error.is_null()) {
            return None;
        }
        let prompt = queued.get("prompt_id")?.as_str()?.to_string();
        self.wait(&prompt, deadline).await
    }

    async fn wait(&self, prompt: &str, deadline: Instant) -> Option<f64> {
        loop {
            if Instant::now() >= deadline {
                return None;
            }
            let history: Value = self
                .authorize(
                    self.client
                        .get(format!("{}/history/{prompt}", self.config.base_url)),
                )
                .send()
                .await
                .ok()?
                .error_for_status()
                .ok()?
                .json()
                .await
                .ok()?;
            if let Some(entry) = history.get(prompt) {
                let status = entry.get("status");
                let state = status
                    .and_then(|status| status.get("status_str"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if state.eq_ignore_ascii_case("error") {
                    return None;
                }
                let completed = status
                    .and_then(|status| status.get("completed"))
                    .and_then(Value::as_bool);
                if completed == Some(true) || state.eq_ignore_ascii_case("success") {
                    return report_mean(entry);
                }
            }
            tokio::time::sleep(Duration::from_millis(self.config.poll_interval_ms)).await;
        }
    }

    /// Moves a checkpoint out of ComfyUI's output folder when that folder is on
    /// this machine, so ranking neither copies 220 MB per candidate nor leaves
    /// the copy behind; otherwise pulls it over the same view endpoint the
    /// trainer already uses.
    async fn stage(&self, name: &str, destination: &Path) -> bool {
        let source = produced(self.config).map(|directory| directory.join(name));
        if let Some(source) = &source
            && source.is_file()
            && fs::rename(source, destination).is_ok()
        {
            return true;
        }
        let Some(bytes) = self.fetch(name).await else {
            return false;
        };
        if fs::write(destination, bytes).is_err() {
            return false;
        }
        if let Some(source) = &source {
            let _ = fs::remove_file(source);
        }
        true
    }

    async fn fetch(&self, name: &str) -> Option<Vec<u8>> {
        let bytes = self
            .authorize(self.client.get(format!(
                "{}/view?filename={}&subfolder=loras&type=output",
                self.config.base_url,
                urlencoding::encode(name)
            )))
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .bytes()
            .await
            .ok()?;
        (bytes.len() >= MIN_WEIGHT_BYTES).then(|| bytes.to_vec())
    }

    fn authorize(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.config.api_token {
            Some(token) => request.header("X-Zone-ComfyUI-Token", token),
            None => request,
        }
    }
}

/// The adapter loader carries a fresh node id on every probe because ComfyUI
/// caches one node instance per id along with the weights it last read, and a
/// reused id would score two different checkpoints with the same tensors.
fn graph(
    checkpoint: &str,
    folder: &str,
    captions: &str,
    resolution: u32,
    lora: Option<&str>,
    percents: &str,
) -> Value {
    let mut nodes = json!({
        "1": {
            "class_type": "CheckpointLoaderSimple",
            "inputs": { "ckpt_name": checkpoint }
        },
        "2": {
            "class_type": "ZoneLoadTrainFolder",
            "inputs": {
                "folder": folder,
                "captions_json": captions,
                "resolution": resolution
            }
        },
        "3": {
            "class_type": "MakeTrainingDataset",
            "inputs": {
                "images": ["2", 0],
                "texts": ["2", 1],
                "vae": ["1", 2],
                "clip": ["1", 1]
            }
        },
        "4": {
            "class_type": "ZoneProbeLoss",
            "inputs": {
                "model": ["1", 0],
                "latents": ["3", 0],
                "positive": ["3", 1],
                "percents": percents,
                "seed": PROBE_SEED
            }
        },
        "6": {
            "class_type": "PreviewAny",
            "inputs": { "source": ["4", 0] }
        }
    });
    if let Some(lora) = lora {
        let loader = format!("5-{}", Uuid::new_v4());
        nodes[loader.as_str()] = json!({
            "class_type": "LoraLoaderModelOnly",
            "inputs": {
                "model": ["1", 0],
                "lora_name": lora,
                "strength_model": 1.0
            }
        });
        nodes["4"]["inputs"]["model"] = json!([loader, 0]);
    }
    nodes
}

fn report_mean(entry: &Value) -> Option<f64> {
    entry
        .get("outputs")?
        .as_object()?
        .values()
        .filter_map(Value::as_object)
        .flat_map(|node| node.values())
        .filter_map(|value| match value {
            Value::Array(items) => items.first().and_then(Value::as_str),
            Value::String(text) => Some(text.as_str()),
            _ => None,
        })
        .filter_map(|text| serde_json::from_str::<Value>(text).ok())
        .find_map(|report| report.get("mean").and_then(Value::as_f64))
}

fn steps(images: usize) -> Option<u32> {
    Some(crate::train::packaged_config().ok()?.steps(images))
}

/// Mirrors `checkpoint_interval` in `train_config.py`. The node bounds how many
/// intermediates a run writes rather than the gap between them, so the gap is
/// derived from the step count and is not the configured `checkpoint_every`.
fn interval(settings: &Settings, steps: u32) -> u32 {
    if settings.checkpoint_every == 0 {
        return 0;
    }
    if settings.checkpoints_per_run < 1 {
        return settings.checkpoint_every;
    }
    settings
        .checkpoint_every
        .max(steps.div_ceil(settings.checkpoints_per_run))
}

fn produced(config: &Config) -> Option<PathBuf> {
    Some(config.models_dir.parent()?.join("output").join("loras"))
}

fn input(config: &Config) -> Option<PathBuf> {
    Some(config.models_dir.parent()?.join("input"))
}

/// Ranking only needs an ordering, and every extra image is another forward
/// pass per candidate, so the cheap probes read a trimmed copy of the folder.
fn subsample(config: &Config, folder: &str, limit: usize) -> Option<String> {
    let input = input(config)?;
    let mut images: Vec<PathBuf> = fs::read_dir(input.join(folder))
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("png"))
        .collect();
    if images.len() <= limit {
        return None;
    }
    images.sort();
    let name = format!("{folder}-rank");
    let destination = input.join(&name);
    fs::create_dir_all(&destination).ok()?;
    let mut copied = 0;
    for image in images.into_iter().take(limit) {
        let Some(filename) = image.file_name() else {
            continue;
        };
        if fs::copy(&image, destination.join(filename)).is_err() {
            continue;
        }
        copied += 1;
        let text = image.with_extension("txt");
        if let Some(filename) = text.file_name().filter(|_| text.is_file()) {
            let _ = fs::copy(&text, destination.join(filename));
        }
    }
    if copied == 0 {
        let _ = fs::remove_dir_all(&destination);
        return None;
    }
    Some(name)
}

fn discard(config: &Config, sample: Option<&str>) {
    if let Some(sample) = sample
        && let Some(input) = input(config)
    {
        let _ = fs::remove_dir_all(input.join(sample));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, Request, Respond, ResponseTemplate,
        matchers::{method, path, path_regex},
    };

    const ADAPTER: &str = "identity.safetensors";

    fn settings() -> Settings {
        serde_json::from_str(PACKAGED_TRAIN_CONFIG).expect("the packaged train config")
    }

    /// The gap the node uses for a one-image run, which is what the harness trains.
    fn gap() -> u32 {
        interval(&settings(), steps(1).expect("the packaged step count"))
    }

    fn weights(marker: u8) -> Vec<u8> {
        vec![marker; MIN_WEIGHT_BYTES + 2_000]
    }

    fn node<'a>(graph: &'a Value, class: &str) -> Option<&'a Value> {
        graph
            .get("prompt")?
            .as_object()?
            .values()
            .find(|node| node["class_type"] == class)
    }

    #[derive(Clone)]
    struct Reports {
        means: HashMap<String, f64>,
        measure: bool,
    }

    struct Queue(Reports);

    impl Respond for Queue {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let graph: Value = serde_json::from_slice(&request.body).unwrap_or_default();
            let percents = node(&graph, "ZoneProbeLoss")
                .and_then(|probe| probe["inputs"]["percents"].as_str())
                .unwrap_or_default()
                .to_string();
            if percents == MEASURE_PERCENTS && !self.0.measure {
                return ResponseTemplate::new(500);
            }
            let lora = node(&graph, "LoraLoaderModelOnly")
                .and_then(|loader| loader["inputs"]["lora_name"].as_str())
                .unwrap_or("base")
                .to_string();
            ResponseTemplate::new(200).set_body_json(json!({ "prompt_id": lora }))
        }
    }

    struct History(Reports);

    impl Respond for History {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let prompt = request
                .url
                .path()
                .trim_start_matches("/history/")
                .to_string();
            let Some(mean) = self.0.means.get(&prompt) else {
                return ResponseTemplate::new(200).set_body_json(json!({}));
            };
            let report = json!({ "images": 4, "mean": mean, "by_percent": {} }).to_string();
            let mut body = serde_json::Map::new();
            body.insert(
                prompt,
                json!({
                    "status": { "status_str": "success" },
                    "outputs": { "6": { "text": [report] } }
                }),
            );
            ResponseTemplate::new(200).set_body_json(Value::Object(body))
        }
    }

    struct View(HashMap<String, Vec<u8>>);

    impl Respond for View {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let name = request
                .url
                .query_pairs()
                .find(|(key, _)| key == "filename")
                .map(|(_, value)| value.to_string())
                .unwrap_or_default();
            match self.0.get(&name) {
                Some(bytes) => ResponseTemplate::new(200).set_body_bytes(bytes.clone()),
                None => ResponseTemplate::new(404),
            }
        }
    }

    struct Harness {
        _root: tempfile::TempDir,
        config: Config,
        output: PathBuf,
        loras: PathBuf,
    }

    impl Harness {
        fn new(server: &MockServer) -> Self {
            let root = tempfile::tempdir().expect("a temporary ComfyUI root");
            let models = root.path().join("models");
            let loras = models.join("loras");
            fs::create_dir_all(&loras).expect("a loras directory");
            let output = loras.join(ADAPTER);
            fs::write(&output, weights(b'f')).expect("the trained adapter");
            Self {
                config: Config {
                    enabled: true,
                    base_url: server.uri(),
                    models_dir: models,
                    poll_interval_ms: 50,
                    ..Default::default()
                },
                output,
                loras,
                _root: root,
            }
        }

        fn checkpoints(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(&self.loras)
                .expect("the loras directory")
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| name.contains("-step"))
                .collect();
            names.sort();
            names
        }

        async fn select(&self) -> Option<Quality> {
            super::select(
                &self.config,
                "zone-train-identity",
                &self.output,
                &HashMap::from([("0000.png".to_string(), "ohwx, a portrait".to_string())]),
            )
            .await
        }
    }

    async fn serve(server: &MockServer, reports: Reports, files: HashMap<String, Vec<u8>>) {
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .respond_with(Queue(reports.clone()))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/history/.+$"))
            .respond_with(History(reports))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/view"))
            .respond_with(View(files))
            .mount(server)
            .await;
    }

    #[test]
    fn checkpoint_names_follow_the_gap_the_node_uses_not_the_one_configured() {
        let settings = settings();
        let longest = interval(&settings, settings.max_steps);
        assert!(
            longest >= settings.checkpoint_every,
            "the node never writes more often than the configured gap"
        );
        assert!(
            settings.max_steps / longest <= settings.checkpoints_per_run,
            "an intermediate is 220 MB, so a long run has to stay bounded"
        );
        assert_eq!(
            interval(
                &Settings {
                    checkpoint_every: 0,
                    ..settings
                },
                6_000
            ),
            0,
            "checkpointing turned off leaves nothing to rank"
        );
    }

    /// Values taken from `checkpoint_interval` in `train_config.py`, which is the
    /// side that decides what the node writes.
    #[test]
    fn the_derived_gap_matches_the_node_that_writes_the_files() {
        let cases = [
            (300_u32, 50_u32, 8_u32, 50_u32),
            (400, 50, 8, 50),
            (401, 50, 8, 51),
            (800, 50, 8, 100),
            (1_900, 50, 8, 238),
            (6_000, 50, 8, 750),
            (6_000, 50, 0, 50),
            (6_000, 0, 8, 0),
            (800, 50, 1, 800),
            (50, 7, 3, 17),
        ];
        for (steps, every, most, expected) in cases {
            assert_eq!(
                interval(
                    &Settings {
                        resolution: 512,
                        max_steps: 800,
                        checkpoint_every: every,
                        checkpoints_per_run: most,
                    },
                    steps
                ),
                expected,
                "{steps} steps at every={every} per_run={most}"
            );
        }
    }

    #[tokio::test]
    async fn checkpoints_are_found_by_name_when_comfyui_writes_beside_us() {
        let server = MockServer::start().await;
        serve(
            &server,
            Reports {
                means: HashMap::from([
                    ("base".to_string(), 1.0),
                    (ADAPTER.to_string(), 0.9),
                    ("identity-step137.safetensors".to_string(), 0.4),
                ]),
                measure: true,
            },
            HashMap::new(),
        )
        .await;
        let harness = Harness::new(&server);
        let produced = produced(&harness.config).unwrap();
        fs::create_dir_all(&produced).unwrap();
        fs::write(produced.join("identity-step137.safetensors"), weights(b'c')).unwrap();
        fs::write(
            produced.join("unrelated-step137.safetensors"),
            weights(b'z'),
        )
        .unwrap();

        let quality = harness.select().await.expect("a measured verdict");

        assert_eq!(
            quality.checkpoint, "step137",
            "a step number no formula would guess still has to be ranked"
        );
        assert_eq!(fs::read(&harness.output).unwrap(), weights(b'c'));
        assert!(
            produced.join("unrelated-step137.safetensors").is_file(),
            "another adapter's checkpoints are not this run's to delete"
        );
        assert!(
            !produced.join("identity-step137.safetensors").exists(),
            "a promoted checkpoint leaves no 220 MB copy behind"
        );
    }

    #[test]
    fn a_verdict_serialises_to_the_shape_callers_read() {
        let verdict = serde_json::to_value(Quality {
            improvement: 0.34,
            checkpoint: "step400".to_string(),
            measured: true,
        })
        .unwrap();
        let fields: Vec<&String> = verdict.as_object().unwrap().keys().collect();
        assert_eq!(fields, ["checkpoint", "improvement", "measured"]);
        assert_eq!(verdict["checkpoint"], "step400");
        assert_eq!(verdict["measured"], true);
        assert!(
            (verdict["improvement"].as_f64().unwrap() - 0.34).abs() < 1e-6,
            "improvement is a fraction, not a percent: {verdict}"
        );
        assert_eq!(
            json!({ "quality": Option::<Quality>::None }).get("quality"),
            Some(&Value::Null),
            "an unmeasured run reports a null verdict, never a missing key"
        );
    }

    #[test]
    fn probe_graph_scores_the_named_adapter_against_the_same_base() {
        let base = graph("flux.safetensors", "set", "{}", 512, None, RANK_PERCENT);
        assert_eq!(base["4"]["inputs"]["model"], json!(["1", 0]));
        assert_eq!(base["4"]["inputs"]["percents"], RANK_PERCENT);
        assert!(
            !base.to_string().contains("LoraLoaderModelOnly"),
            "a baseline must load no adapter at all, or it is not a baseline"
        );

        let adapter = graph(
            "flux.safetensors",
            "set",
            "{}",
            512,
            Some(ADAPTER),
            MEASURE_PERCENTS,
        );
        let loader = adapter["4"]["inputs"]["model"][0]
            .as_str()
            .expect("the probe reads its model from the adapter loader")
            .to_string();
        assert_eq!(adapter[&loader]["inputs"]["lora_name"], ADAPTER);
        assert_eq!(adapter[&loader]["inputs"]["model"], json!(["1", 0]));
        assert_eq!(adapter["1"]["inputs"]["ckpt_name"], "flux.safetensors");
        let again = graph(
            "flux.safetensors",
            "set",
            "{}",
            512,
            Some(ADAPTER),
            MEASURE_PERCENTS,
        );
        assert_ne!(
            again["4"]["inputs"]["model"][0].as_str(),
            Some(loader.as_str()),
            "a reused loader id lets ComfyUI score new weights with the ones it cached"
        );
    }

    #[tokio::test]
    async fn promotes_the_best_checkpoint_over_the_last_one() {
        let every = gap();
        let first = format!("identity-step{every}.safetensors");
        let second = format!("identity-step{}.safetensors", every * 2);
        let server = MockServer::start().await;
        serve(
            &server,
            Reports {
                means: HashMap::from([
                    ("base".to_string(), 1.0),
                    (ADAPTER.to_string(), 0.9),
                    (first.clone(), 0.8),
                    (second.clone(), 0.6),
                ]),
                measure: true,
            },
            HashMap::from([(first, weights(b'a')), (second, weights(b'b'))]),
        )
        .await;
        let harness = Harness::new(&server);

        let quality = harness.select().await.expect("a measured verdict");

        assert_eq!(quality.checkpoint, format!("step{}", every * 2));
        assert!(quality.measured);
        assert!(
            (quality.improvement - 0.4).abs() < 1e-6,
            "improvement is the fraction of base loss removed, got {}",
            quality.improvement
        );
        assert_eq!(
            fs::read(&harness.output).unwrap(),
            weights(b'b'),
            "the best checkpoint has to reach the adapter's filename, not merely win the ranking"
        );
        assert!(
            harness.checkpoints().is_empty(),
            "losing checkpoints are 220 MB each and must not survive the run: {:?}",
            harness.checkpoints()
        );
    }

    #[tokio::test]
    async fn keeps_the_final_adapter_when_no_checkpoint_beats_it() {
        let first = format!("identity-step{}.safetensors", gap());
        let server = MockServer::start().await;
        serve(
            &server,
            Reports {
                means: HashMap::from([
                    ("base".to_string(), 1.0),
                    (ADAPTER.to_string(), 0.5),
                    (first.clone(), 0.7),
                ]),
                measure: true,
            },
            HashMap::from([(first, weights(b'a'))]),
        )
        .await;
        let harness = Harness::new(&server);

        let quality = harness.select().await.expect("a measured verdict");

        assert_eq!(quality.checkpoint, FINAL);
        assert!((quality.improvement - 0.5).abs() < 1e-6);
        assert_eq!(fs::read(&harness.output).unwrap(), weights(b'f'));
        assert!(harness.checkpoints().is_empty());
    }

    #[tokio::test]
    async fn a_failed_probe_leaves_the_trained_adapter_in_place() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let harness = Harness::new(&server);

        assert!(
            harness.select().await.is_none(),
            "a probe that cannot run reports no score rather than failing the training run"
        );
        assert_eq!(fs::read(&harness.output).unwrap(), weights(b'f'));
    }

    #[tokio::test]
    async fn an_adapter_that_loses_to_its_base_is_reported_honestly() {
        let server = MockServer::start().await;
        serve(
            &server,
            Reports {
                means: HashMap::from([("base".to_string(), 1.0), (ADAPTER.to_string(), 1.25)]),
                measure: true,
            },
            HashMap::new(),
        )
        .await;
        let harness = Harness::new(&server);

        let quality = harness.select().await.expect("a verdict, even a bad one");

        assert_eq!(quality.checkpoint, FINAL);
        assert!(
            (quality.improvement + 0.25).abs() < 1e-6,
            "an adapter worse than its base has to read as worse, got {}",
            quality.improvement
        );
        assert_eq!(fs::read(&harness.output).unwrap(), weights(b'f'));
    }

    #[tokio::test]
    async fn a_ranking_that_cannot_be_measured_properly_says_so() {
        let server = MockServer::start().await;
        serve(
            &server,
            Reports {
                means: HashMap::from([("base".to_string(), 1.0), (ADAPTER.to_string(), 0.8)]),
                measure: false,
            },
            HashMap::new(),
        )
        .await;
        let harness = Harness::new(&server);

        let quality = harness.select().await.expect("the ranking still stands");

        assert!(!quality.measured);
        assert!((quality.improvement - 0.2).abs() < 1e-6);
    }

    #[tokio::test]
    async fn ranking_reads_a_capped_sample_of_the_training_images() {
        let server = MockServer::start().await;
        serve(
            &server,
            Reports {
                means: HashMap::from([("base".to_string(), 1.0), (ADAPTER.to_string(), 0.8)]),
                measure: true,
            },
            HashMap::new(),
        )
        .await;
        let harness = Harness::new(&server);
        let folder = "zone-train-identity";
        let staged = input(&harness.config).unwrap().join(folder);
        fs::create_dir_all(&staged).unwrap();
        for index in 0..RANK_IMAGES + 3 {
            fs::write(staged.join(format!("{index:04}.png")), b"png").unwrap();
            fs::write(staged.join(format!("{index:04}.txt")), b"ohwx").unwrap();
        }

        harness.select().await.expect("a measured verdict");

        let folders: Vec<String> = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path() == "/prompt")
            .filter_map(|request| {
                let graph: Value = serde_json::from_slice(&request.body).ok()?;
                Some((
                    node(&graph, "ZoneLoadTrainFolder")?["inputs"]["folder"]
                        .as_str()?
                        .to_string(),
                    node(&graph, "ZoneProbeLoss")?["inputs"]["percents"]
                        .as_str()?
                        .to_string(),
                ))
            })
            .filter(|(_, percents)| percents == RANK_PERCENT)
            .map(|(folder, _)| folder)
            .collect();
        assert!(
            folders.iter().all(|name| name == &format!("{folder}-rank")),
            "ranking must not pay for the whole dataset on every candidate: {folders:?}"
        );
        assert!(
            !input(&harness.config)
                .unwrap()
                .join(format!("{folder}-rank"))
                .exists(),
            "the sampled folder is scratch space and must not outlive the run"
        );
    }
}
