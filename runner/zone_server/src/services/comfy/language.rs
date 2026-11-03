mod dataset;
mod example;
mod format;
mod outcome;
mod request;

pub use outcome::Outcome;
pub use request::Request;

use super::policy;
use abnegate_comfy::Config;
use abnegate_comfy::host;
use abnegate_comfy::host::Compute;
use abnegate_comfy::host::DATA_DIRECTORY;
use abnegate_comfy::host::DATASET_DIRECTORY;
use abnegate_comfy::host::Job;
use abnegate_comfy::host::JobDirectory;
use abnegate_comfy::lora::TrainError;
use abnegate_comfy::lora::TrainImage;
use abnegate_comfy::lora::TrainMethod;
use abnegate_comfy::train::TrainProgress;
use abnegate_secret::SecretValue;
use base64::Engine;
use dataset::Dataset;
use std::fs;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use tokio::sync::watch;

pub const SUBJECT: &str = "language";
pub const RECIPE: &str = "chat";
pub const MINIMUM_STEPS: u32 = 200;
pub const MAXIMUM_STEPS: u32 = 1000;
pub const INVALID_NAME: &str = "invalid chat model name";
pub const UNAVAILABLE_METHOD: &str = "not available for a chat model";
pub const UNKNOWN_BASE: &str = "unknown training base";
pub const INVALID_BASE64: &str = "document is not valid base64";
pub const UNCONFINED: &str = "a staged file is one plain name inside its job";
const WEIGHT_EXTENSION: &str = ".safetensors";
const DOCUMENT: &str = "document";

pub fn validate(config: &Config, request: &Request) -> Result<(), TrainError> {
    prepare(config, request).map(|_| ())
}

pub async fn train(
    config: &Config,
    request: Request,
    compute: Compute,
    key: Option<SecretValue>,
    progress: watch::Sender<Option<TrainProgress>>,
) -> Result<Outcome, TrainError> {
    let (name, dataset) = prepare(config, &request)?;
    let key = policy::key(compute, key)?;
    let job = job(&request, &name, dataset.len(), compute);
    let directory = JobDirectory::new(config, job.id);
    if let Err(error) = queue(&directory, &job, &dataset, &request.documents, key.as_ref()) {
        let _ = host::remove(config, job.id);
        return Err(error);
    }
    let finished = host::wait_reporting(config, &directory, progress).await?;
    let published = finished
        .filename
        .filter(|filename| !filename.is_empty())
        .unwrap_or(name);
    Ok(Outcome {
        path: directory.path().join(published),
        examples: dataset.len(),
    })
}

pub fn model_name(name: &str) -> Result<String, TrainError> {
    let name = name.trim();
    let plain = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.ends_with(WEIGHT_EXTENSION)
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ':' | '-')
        })
        && Path::new(name).file_name().and_then(|value| value.to_str()) == Some(name)
        && Path::new(name).components().count() == 1;
    if !plain {
        return Err(TrainError::Invalid(INVALID_NAME));
    }
    Ok(name.to_string())
}

pub fn steps_for(examples: usize) -> u32 {
    u32::try_from(examples)
        .unwrap_or(u32::MAX)
        .clamp(MINIMUM_STEPS, MAXIMUM_STEPS)
}

pub fn document_bytes(document: &TrainImage) -> Result<Vec<u8>, TrainError> {
    if let Some(bytes) = &document.bytes {
        return Ok(bytes.clone());
    }
    let encoded = document.bytes_base64.trim();
    if encoded.is_empty() {
        return Ok(Vec::new());
    }
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| TrainError::Invalid(INVALID_BASE64))
}

fn prepare(config: &Config, request: &Request) -> Result<(String, Dataset), TrainError> {
    if config.host_trainer.is_none() {
        return Err(TrainError::Disabled);
    }
    if request.documents.is_empty() {
        return Err(TrainError::Invalid(dataset::EMPTY));
    }
    if !matches!(request.method, TrainMethod::Lora | TrainMethod::Finetune) {
        return Err(TrainError::Invalid(UNAVAILABLE_METHOD));
    }
    if request.base.trim().is_empty() {
        return Err(TrainError::Invalid(UNKNOWN_BASE));
    }
    let name = model_name(&request.name)?;
    let dataset = Dataset::from_documents(&request.documents)?;
    Ok((name, dataset))
}

fn job(request: &Request, name: &str, examples: usize, compute: Compute) -> Job {
    Job::new(
        request.name.as_str(),
        request.method.as_str(),
        request.base.trim(),
    )
    .with_subject(SUBJECT)
    .with_recipe_id(RECIPE)
    .with_filename(name)
    .with_image_count(examples)
    .with_compute(compute)
}

fn queue(
    directory: &JobDirectory,
    job: &Job,
    dataset: &Dataset,
    documents: &[TrainImage],
    key: Option<&SecretValue>,
) -> Result<(), TrainError> {
    directory.write_queued_progress(steps_for(job.image_count))?;
    dataset.write(&confined(directory.path(), DATA_DIRECTORY)?)?;
    stage(&confined(directory.path(), DATASET_DIRECTORY)?, documents)?;
    if let Some(key) = key {
        directory.write_compute_key(job.compute, key)?;
    }
    directory.write_job(job)
}

fn stage(directory: &Path, documents: &[TrainImage]) -> Result<(), TrainError> {
    fs::create_dir_all(directory).map_err(failed)?;
    for (index, document) in documents.iter().enumerate() {
        fs::write(
            confined(directory, &staged_name(index, &document.filename))?,
            document_bytes(document)?,
        )
        .map_err(failed)?;
    }
    Ok(())
}

fn confined(directory: &Path, name: &str) -> Result<PathBuf, TrainError> {
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(directory.join(name)),
        _ => Err(TrainError::Invalid(UNCONFINED)),
    }
}

fn staged_name(index: usize, filename: &str) -> String {
    let base = Path::new(filename)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(DOCUMENT);
    let safe: String = base
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let safe = if safe.is_empty() || safe == "." || safe == ".." {
        DOCUMENT.to_string()
    } else {
        safe
    };
    format!("{index:04}-{safe}")
}

fn failed(error: std::io::Error) -> TrainError {
    TrainError::Failed(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use abnegate_comfy::host::FORMAT_FILE;
    use abnegate_comfy::host::Status;
    use abnegate_comfy::host::TRAIN_DATA_FILE;
    use abnegate_comfy::host::Trainer;
    use abnegate_comfy::train::Phase;
    use serde_json::Value;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::task::JoinHandle;

    const HOST_DIRECTORY: &str = ".zone-train";
    const CHAT: &str =
        r#"{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}"#;
    const RUNPOD_KEY: &str = "runpod.key";

    fn hosted() -> (TempDir, Config) {
        let root = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.models_directory = root.path().to_path_buf();
        config.contract.host_directory = HOST_DIRECTORY.to_string();
        config.host_trainer = Some(
            Trainer::new()
                .with_poll_interval(Duration::from_millis(5))
                .with_pickup(Duration::from_secs(5)),
        );
        (root, config)
    }

    fn document(filename: &str, body: &str) -> TrainImage {
        TrainImage::from_bytes(filename, "", body.as_bytes())
    }

    fn chat(name: &str) -> Request {
        Request::new(name, "qwen2.5:7b", vec![document("chat.json", CHAT)])
    }

    fn silent() -> watch::Sender<Option<TrainProgress>> {
        watch::Sender::new(None)
    }

    fn host_entries(config: &Config) -> usize {
        fs::read_dir(host::root(config)).map_or(0, |entries| entries.count())
    }

    fn succeed_first_job(config: &Config) -> JoinHandle<Option<(String, u32)>> {
        let config = config.clone();
        tokio::spawn(async move {
            for _ in 0..1000 {
                if let Some(job) = host::current(&config) {
                    let directory = JobDirectory::new(&config, job.id);
                    let key = directory.path().join(RUNPOD_KEY);
                    let secret = fs::read_to_string(&key).ok().map(|value| {
                        (
                            value,
                            fs::metadata(&key).unwrap().permissions().mode() & 0o777,
                        )
                    });
                    let mut finished = directory.read_job().unwrap();
                    finished.status = Status::Succeeded;
                    directory.write_job(&finished).unwrap();
                    return secret;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            None
        })
    }

    #[test]
    fn validation_refuses_what_a_chat_job_cannot_train() {
        let (_root, config) = hosted();
        let mut request = Request::new("support-bot", "qwen2.5:7b", Vec::new());
        assert!(matches!(
            validate(&config, &request),
            Err(TrainError::Invalid(dataset::EMPTY))
        ));
        request.documents = vec![document("chat.json", CHAT)];
        for method in [TrainMethod::Video, TrainMethod::Pivotal] {
            request.method = method;
            assert!(matches!(
                validate(&config, &request),
                Err(TrainError::Invalid(UNAVAILABLE_METHOD))
            ));
        }
        request.method = TrainMethod::Lora;
        request.base = "  ".into();
        assert!(matches!(
            validate(&config, &request),
            Err(TrainError::Invalid(UNKNOWN_BASE))
        ));
        request.base = "qwen2.5:7b".into();
        request.name = "support.safetensors".into();
        assert!(matches!(
            validate(&config, &request),
            Err(TrainError::Invalid(INVALID_NAME))
        ));
        request.name = "support-bot".into();
        validate(&config, &request)
            .expect("a chat model needs no people checkpoint and no .safetensors name");
        let request = request.with_method(TrainMethod::Finetune);
        validate(&config, &request).expect("a chat fine-tune is a host chat job");
    }

    #[test]
    fn validation_needs_a_host_trainer() {
        let (_root, mut config) = hosted();
        config.host_trainer = None;
        assert!(matches!(
            validate(&config, &chat("support-bot")),
            Err(TrainError::Disabled)
        ));
    }

    #[test]
    fn validation_rejects_mixed_document_formats() {
        let (_root, config) = hosted();
        let request = Request::new(
            "support-bot",
            "qwen2.5:7b",
            vec![document("chat.jsonl", CHAT), document("notes.txt", "loose")],
        );
        assert!(matches!(
            validate(&config, &request),
            Err(TrainError::Invalid(format::MIXED))
        ));
    }

    #[test]
    fn model_names_are_one_plain_path_component() {
        assert_eq!(model_name(" support-bot ").unwrap(), "support-bot");
        assert_eq!(model_name("qwen2.5:7b-tuned").unwrap(), "qwen2.5:7b-tuned");
        for name in [
            "",
            "  ",
            ".",
            "..",
            "support/bot",
            "../bot",
            "support bot",
            "bot.safetensors",
            "bot\u{7}",
        ] {
            assert!(
                matches!(model_name(name), Err(TrainError::Invalid(INVALID_NAME))),
                "{name:?} is not a chat model name"
            );
        }
    }

    #[test]
    fn step_budget_clamps_to_the_chat_range() {
        assert_eq!(steps_for(1), 200);
        assert_eq!(steps_for(200), 200);
        assert_eq!(steps_for(1000), 1000);
        assert_eq!(steps_for(5000), 1000);
    }

    #[test]
    fn staged_names_are_indexed_plain_file_names() {
        assert_eq!(staged_name(0, "../etc/passwd"), "0000-passwd");
        assert_eq!(staged_name(3, "my notes.txt"), "0003-my_notes.txt");
        assert_eq!(staged_name(1, ".."), "0001-document");
        assert_eq!(staged_name(2, ""), "0002-document");
    }

    #[test]
    fn a_staged_path_is_one_plain_name_inside_its_directory() {
        let directory = Path::new("/models/.zone-train/job");
        assert_eq!(
            confined(directory, "0000-chat.json").unwrap(),
            directory.join("0000-chat.json")
        );
        for name in ["", ".", "..", "../escape", "nested/file", "/etc/passwd"] {
            assert!(
                matches!(
                    confined(directory, name),
                    Err(TrainError::Invalid(UNCONFINED))
                ),
                "{name:?} leaves its directory"
            );
        }
    }

    #[test]
    fn a_language_job_json_zone_writes_today_reads_through_the_host_contract() {
        let (_root, config) = hosted();
        let id = "5b0f3c1e-8d7a-4f3e-9a51-0c2d6e7f8a9b";
        let directory = host::root(&config).join(id);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("job.json"),
            r#"{
  "schema_version": 1,
  "id": "5b0f3c1e-8d7a-4f3e-9a51-0c2d6e7f8a9b",
  "name": "support-bot",
  "method": "lora",
  "subject": "language",
  "trigger": "",
  "checkpoint": "qwen2.5:7b",
  "provider": "runpod",
  "gpu": "A40",
  "pod_id": "pod-1",
  "status": "running",
  "filename": "support-bot",
  "recipe_id": "chat",
  "hf_base": "",
  "image_count": 12,
  "pid": 4242,
  "started_at": "2026-11-03T09:30:00Z"
}"#,
        )
        .unwrap();

        let job = host::current(&config).expect("zone's language job.json reads");

        assert_eq!(job.id.to_string(), id);
        assert_eq!(job.name, "support-bot");
        assert_eq!(job.method, TrainMethod::Lora.as_str());
        assert_eq!(job.subject, SUBJECT);
        assert_eq!(job.trigger, "");
        assert_eq!(job.checkpoint, "qwen2.5:7b");
        assert_eq!(job.compute, Compute::Runpod);
        assert_eq!(job.gpu.as_deref(), Some("A40"));
        assert_eq!(job.pod_id.as_deref(), Some("pod-1"));
        assert_eq!(job.status, Status::Running);
        assert_eq!(job.filename.as_deref(), Some("support-bot"));
        assert_eq!(job.recipe_id, RECIPE);
        assert_eq!(job.image_count, 12);
        assert_eq!(job.process_id, Some(4242));
        assert_eq!(job.base, "", "a language job is the host's to publish");
        assert!(job.busy());
    }

    #[test]
    fn the_queued_job_keeps_every_key_zone_wrote_before() {
        let job = job(&chat("support-bot"), "support-bot", 3, Compute::Runpod);
        let written = serde_json::to_value(&job).unwrap();
        let expected = json!({
            "schema_version": 1,
            "name": "support-bot",
            "method": "lora",
            "subject": "language",
            "trigger": "",
            "checkpoint": "qwen2.5:7b",
            "provider": "runpod",
            "status": "queued",
            "filename": "support-bot",
            "recipe_id": "chat",
            "hf_base": "",
            "image_count": 3,
        });
        for (key, value) in expected.as_object().unwrap() {
            assert_eq!(&written[key], value, "{key} changed on the wire");
        }
        assert_eq!(written["id"], json!(job.id.to_string()));
        assert!(written["started_at"].is_string());
        assert!(written.get(RUNPOD_KEY).is_none());
        assert!(written.get("runpod_api_key").is_none());
    }

    #[tokio::test]
    async fn training_queues_a_host_job_and_returns_the_chat_model_name() {
        let (_root, config) = hosted();
        let trainer = succeed_first_job(&config);
        let (progress, receiver) = watch::channel(None);

        let outcome = train(&config, chat("support-bot"), Compute::Local, None, progress)
            .await
            .unwrap();

        assert!(trainer.await.unwrap().is_none(), "a local job has no key");
        assert_eq!(outcome.path.file_name().unwrap(), "support-bot");
        assert_eq!(outcome.examples, 1);
        let job = host::current(&config).unwrap();
        assert_eq!(job.subject, SUBJECT);
        assert_eq!(job.recipe_id, RECIPE);
        assert_eq!(job.checkpoint, "qwen2.5:7b");
        assert_eq!(job.filename.as_deref(), Some("support-bot"));
        assert_eq!(job.trigger, "");
        assert_eq!(job.method, "lora");
        assert_eq!(job.image_count, 1);
        assert_eq!(job.compute, Compute::Local);
        assert_eq!(job.base, "");
        assert!(job.secrets.is_empty());
        let directory = JobDirectory::new(&config, job.id);
        assert_eq!(outcome.path, directory.path().join("support-bot"));
        assert!(
            directory
                .path()
                .join(DATA_DIRECTORY)
                .join(TRAIN_DATA_FILE)
                .is_file()
        );
        assert!(
            directory
                .path()
                .join(DATA_DIRECTORY)
                .join(FORMAT_FILE)
                .is_file()
        );
        assert_eq!(
            fs::read_to_string(
                directory
                    .path()
                    .join(DATASET_DIRECTORY)
                    .join("0000-chat.json")
            )
            .unwrap(),
            CHAT
        );
        let queued = receiver
            .borrow()
            .clone()
            .expect("the queued progress is reported");
        assert_eq!(queued.total, MINIMUM_STEPS);
        assert_eq!(queued.phase, Some(Phase::Queued));
        for weights in ["loras", "checkpoints"] {
            assert!(
                !config
                    .models_directory
                    .join(weights)
                    .join("support-bot.safetensors")
                    .exists()
            );
        }
    }

    #[tokio::test]
    async fn a_remote_job_carries_its_key_beside_the_job_until_it_ends() {
        let (_root, config) = hosted();
        let trainer = succeed_first_job(&config);

        let outcome = train(
            &config,
            chat("support-bot"),
            Compute::Runpod,
            Some(SecretValue::new(" rp-test-key ")),
            silent(),
        )
        .await
        .unwrap();

        let (key, mode) = trainer.await.unwrap().expect("the trainer read the key");
        assert_eq!(key, "rp-test-key\n");
        assert_eq!(mode, 0o600);
        assert_eq!(outcome.path.file_name().unwrap(), "support-bot");
        let job = host::current(&config).unwrap();
        assert_eq!(job.compute, Compute::Runpod);
        assert_eq!(job.secrets, vec![RUNPOD_KEY.to_string()]);
        let directory = JobDirectory::new(&config, job.id);
        assert!(
            directory
                .path()
                .join(DATA_DIRECTORY)
                .join(TRAIN_DATA_FILE)
                .is_file()
        );
        assert!(
            !directory.path().join(RUNPOD_KEY).exists(),
            "the key is removed once the job ends"
        );
        let written: Value =
            serde_json::from_slice(&fs::read(directory.path().join("job.json")).unwrap()).unwrap();
        assert!(!written.to_string().contains("rp-test-key"));
    }

    #[tokio::test]
    async fn a_remote_job_without_a_key_queues_nothing() {
        let (_root, config) = hosted();
        for key in [None, Some(SecretValue::new("  "))] {
            let error = train(&config, chat("support-bot"), Compute::Runpod, key, silent())
                .await
                .unwrap_err();
            assert!(
                matches!(error, TrainError::Invalid(policy::MISSING_RUNPOD_KEY)),
                "{error}"
            );
        }
        assert_eq!(host_entries(&config), 0);
    }

    #[tokio::test]
    async fn a_job_that_cannot_be_queued_leaves_no_directory_behind() {
        let (_root, config) = hosted();
        let filename = format!("{}.txt", "n".repeat(300));
        let request = Request::new(
            "support-bot",
            "qwen2.5:7b",
            vec![document(&filename, "a short dump")],
        );

        let error = train(&config, request, Compute::Local, None, silent())
            .await
            .unwrap_err();

        assert!(matches!(error, TrainError::Failed(_)), "{error}");
        assert_eq!(host_entries(&config), 0);
    }
}
