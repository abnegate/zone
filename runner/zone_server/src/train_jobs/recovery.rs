use super::TrainRegistry;
use abnegate_comfy::Config;
use abnegate_comfy::host;
use abnegate_comfy::host::Job;
use abnegate_comfy::lora::TrainError;
use std::path::PathBuf;
use tokio::sync::watch;

/// Picks up the host jobs a restart left behind. Every job that finished
/// while the server was down is published, and the one still running is
/// followed in `registry` until it ends, then published too. A job with no
/// base, such as a chat model, is the host's to publish and is left alone.
pub async fn recover(config: Config, registry: TrainRegistry) {
    let scan = config.clone();
    let (published, running) = match tokio::task::spawn_blocking(move || scan_jobs(&scan)).await {
        Ok(found) => found,
        Err(error) => {
            tracing::warn!(%error, "Could not look for training jobs a restart left behind");
            return;
        }
    };
    for result in published {
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
    use abnegate_comfy::host::JobDirectory;
    use abnegate_comfy::host::Status;
    use abnegate_comfy::host::Trainer;
    use abnegate_comfy::train::TrainProgress;
    use std::fs;
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
}
