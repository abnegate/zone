use super::TrainRegistry;
use crate::services::comfy::adoption;
use abnegate_comfy::Config;
use abnegate_comfy::host;
use abnegate_comfy::host::Job;
use abnegate_comfy::host::JobDirectory;
use abnegate_comfy::host::Status;
use abnegate_comfy::lora::TrainError;
use std::fs;
use std::path::PathBuf;
use tokio::sync::watch;
use uuid::Uuid;

/// Adopts the weights zone trained before abnegate-comfy, so their names
/// train again, then picks up the host jobs a restart left behind. Every job
/// that finished while the server was down is published, and the one still
/// running is followed in `registry` until it ends, then published too. A job
/// with no base, such as a chat model, is the host's to publish and is left
/// alone.
pub async fn recover(config: Config, registry: TrainRegistry) {
    let scan = config.clone();
    let finished = match tokio::task::spawn_blocking(move || {
        adoption::adopt(&scan);
        finished_jobs(&scan)
    })
    .await
    {
        Ok(finished) => finished,
        Err(error) => {
            tracing::warn!(%error, "Could not look for training jobs a restart left behind");
            return;
        }
    };
    for id in finished {
        report(host::resume(&config, id).await);
    }
    let scan = config.clone();
    let (published, running) = match tokio::task::spawn_blocking(move || scan_jobs(&scan)).await {
        Ok(found) => found,
        Err(error) => {
            tracing::warn!(%error, "Could not look for training jobs a restart left behind");
            return;
        }
    };
    for result in published {
        report(result);
    }
    let Some(running) = running else {
        return;
    };
    let Some(job) = registry.resume(&running) else {
        return;
    };
    let (sender, receiver) = watch::channel(None);
    match job
        .follow(
            receiver,
            host::resume_reporting(&config, running.id, sender),
        )
        .await
    {
        Ok(path) => job.succeed_resumed(&path),
        Err(error) => {
            tracing::warn!(%error, id = %running.id, "A resumed training job was not published");
            job.fail(error);
        }
    }
}

fn report(result: Result<PathBuf, TrainError>) {
    match result {
        Ok(path) => tracing::info!(
            path = %path.display(),
            "Published a training job that finished while the server was down"
        ),
        Err(error) => tracing::warn!(
            %error,
            "Could not publish a training job that finished while the server was down"
        ),
    }
}

/// The jobs that succeeded while the server was down and whose weight still
/// waits in their directory, which [`host::resume`] publishes once it holds
/// the job's lock. [`host::recover`] only tries that lock, and a child that
/// another thread spawns while it is held inherits the lock's descriptor
/// until it execs, so a job nobody publishes can read as held and be skipped.
fn finished_jobs(config: &Config) -> Vec<Uuid> {
    if config.host_trainer.is_none() {
        return Vec::new();
    }
    host::jobs(config)
        .into_iter()
        .filter(|job| {
            job.status == Status::Succeeded
                && !job.base.is_empty()
                && job.published.is_none()
                && job.filename.as_deref().is_some_and(|filename| {
                    fs::symlink_metadata(JobDirectory::new(config, job.id).path().join(filename))
                        .is_ok_and(|metadata| metadata.is_file())
                })
        })
        .map(|job| job.id)
        .collect()
}

fn scan_jobs(config: &Config) -> (Vec<Result<PathBuf, TrainError>>, Option<Job>) {
    let published = host::recover(config);
    let running = host::current(config).filter(|job| job.busy() && !job.base.is_empty());
    (published, running)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::comfy::policy::PEOPLE_BASE;
    use crate::services::comfy::policy::PEOPLE_CHECKPOINT;
    use crate::services::comfy::settings;
    use crate::train_jobs::TrainJobStatus;
    use abnegate_comfy::host::LOCK_FILE;
    use abnegate_comfy::host::Trainer;
    use abnegate_comfy::train::TrainProgress;
    use std::fs::OpenOptions;
    use std::process::{Command, Stdio};
    use std::time::Duration;
    use tempfile::TempDir;

    const WEIGHT: &str = "jerry.safetensors";
    const PUBLISHED: &str = "loras/jerry.safetensors";

    fn hosted() -> (TempDir, Config) {
        let models = tempfile::tempdir().expect("models directory");
        let mut config = settings::development().with_host_trainer(
            Trainer::new()
                .with_poll_interval(Duration::from_millis(5))
                .with_pickup(Duration::from_secs(5)),
        );
        config.models_directory = models.path().to_path_buf();
        let checkpoint = config.models_directory.join(PEOPLE_CHECKPOINT);
        fs::create_dir_all(checkpoint.parent().expect("checkpoints")).expect("checkpoints");
        fs::write(checkpoint, b"weights").expect("people checkpoint");
        (models, config)
    }

    fn person(status: Status) -> Job {
        let mut job = Job::new("jerry", "lora", PEOPLE_CHECKPOINT)
            .with_subject("person")
            .with_trigger("ohwx")
            .with_base(PEOPLE_BASE)
            .with_filename(WEIGHT);
        job.status = status;
        job
    }

    fn write(config: &Config, job: &Job) -> JobDirectory {
        let directory = JobDirectory::new(config, job.id);
        directory.write_job(job).expect("job.json");
        directory
    }

    fn finish(directory: &JobDirectory) {
        fs::write(directory.path().join(WEIGHT), b"adapter").expect("weight");
        let mut job = directory.read_job().expect("job.json");
        job.status = Status::Succeeded;
        directory.write_job(&job).expect("job.json");
    }

    #[tokio::test]
    async fn a_job_that_finished_while_the_server_was_down_is_published_at_startup() {
        let (_models, config) = hosted();
        finish(&write(&config, &person(Status::Running)));
        let registry = TrainRegistry::new();

        recover(config.clone(), registry.clone()).await;

        let published = config.models_directory.join(PUBLISHED);
        assert_eq!(fs::read(&published).expect("published weight"), b"adapter");
        assert!(
            registry.current().is_none(),
            "a job that already ended is not followed"
        );
    }

    #[tokio::test]
    async fn a_job_that_finished_over_a_weight_trained_before_the_upgrade_is_published() {
        let (_models, config) = hosted();
        let weight = config.models_directory.join(PUBLISHED);
        fs::create_dir_all(weight.parent().expect("loras")).expect("loras");
        fs::write(&weight, b"previous").expect("previous weight");
        let sidecar = weight.with_file_name(format!("{WEIGHT}{}", config.contract.sidecar_suffix));
        fs::write(
            &sidecar,
            br#"{"recipe_id": "sdxl-adapter", "hf_base": "stabilityai/stable-diffusion-xl-base-1.0", "trigger": "ohwx"}"#,
        )
        .expect("a sidecar written before abnegate-comfy");
        finish(&write(&config, &person(Status::Running)));

        recover(config.clone(), TrainRegistry::new()).await;

        assert_eq!(fs::read(&weight).expect("published weight"), b"adapter");
        let published: serde_json::Value =
            serde_json::from_slice(&fs::read(&sidecar).expect("sidecar")).expect("sidecar");
        assert!(
            published["generation"]
                .as_str()
                .is_some_and(|generation| uuid::Uuid::parse_str(generation).is_ok()),
            "{published}"
        );
    }

    #[tokio::test]
    async fn a_job_still_running_shows_its_progress_again_and_is_published_when_it_ends() {
        let (_models, config) = hosted();
        let job = person(Status::Running);
        let directory = write(&config, &job);
        directory
            .write_progress(&TrainProgress::new(3, 10))
            .expect("progress.json");
        let registry = TrainRegistry::new();

        let recovery = tokio::spawn(recover(config.clone(), registry.clone()));
        let mut followed = None;
        for _ in 0..1000 {
            followed = registry.current().filter(|view| view.step == Some(3));
            if followed.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let followed = followed.expect("the registry follows the running job");
        assert_eq!(followed.id, job.id);
        assert_eq!(followed.name, "jerry");
        assert_eq!(followed.status, TrainJobStatus::Running);
        assert_eq!(followed.total, Some(10));
        assert_eq!(followed.method.as_deref(), Some("lora"));
        assert_eq!(followed.provider.as_deref(), Some("local"));
        assert_eq!(
            followed.started_at,
            directory.read_job().expect("job.json").started_at
        );

        finish(&directory);
        recovery.await.expect("recovery");

        let view = registry.current().expect("the finished job");
        assert_eq!(view.status, TrainJobStatus::Succeeded, "{:?}", view.error);
        assert_eq!(view.filename.as_deref(), Some(WEIGHT));
        assert_eq!(
            fs::read(config.models_directory.join(PUBLISHED)).expect("published weight"),
            b"adapter"
        );
    }

    #[tokio::test]
    async fn a_running_job_with_no_base_is_left_to_the_host() {
        let (_models, config) = hosted();
        let mut language = Job::new("support-bot", "lora", "qwen2.5:7b")
            .with_subject("language")
            .with_filename("support-bot");
        language.status = Status::Running;
        write(&config, &language);
        let registry = TrainRegistry::new();

        recover(config, registry.clone()).await;

        assert!(registry.current().is_none());
    }

    /// Hands the job's lock to a child process, the way a child spawned while
    /// the lock is held inherits it, and drops this process's own descriptor.
    /// The child holds the lock until it exits.
    fn held_by_a_child(directory: &JobDirectory) -> std::process::Child {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.path().join(LOCK_FILE))
            .expect("publish.lock");
        lock.try_lock().expect("nobody holds the job yet");
        Command::new("/bin/sleep")
            .arg("1")
            .stdin(Stdio::from(lock))
            .spawn()
            .expect("a child holding the lock")
    }

    #[tokio::test]
    async fn a_finished_job_whose_lock_a_spawned_child_inherited_is_still_published() {
        let (_models, config) = hosted();
        let directory = write(&config, &person(Status::Running));
        finish(&directory);
        let mut child = held_by_a_child(&directory);

        recover(config.clone(), TrainRegistry::new()).await;

        child.wait().expect("the child exits");
        assert_eq!(
            fs::read(config.models_directory.join(PUBLISHED)).expect("published weight"),
            b"adapter"
        );
    }
}
