//! Per-run workspaces. The guard owns the directory until publication finishes
//! and removes it when execution completes, fails, or is cancelled.

use sqlx::PgPool;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use zone_vcs::git::GitService;

use crate::db::{projects, tasks::TaskRow};

pub struct Repository {
    pub url: String,
    pub token: Option<String>,
}

impl Repository {
    pub async fn resolve(pool: &PgPool, task: &TaskRow) -> Result<Option<Self>, String> {
        let requested = task
            .github_repo_url
            .as_deref()
            .map(GitService::repository_url)
            .transpose()
            .map_err(|error| error.to_string())?;
        let mut repositories = Vec::new();
        for id in &task.project_ids {
            let project = projects::get_project(pool, *id)
                .await
                .map_err(|_| "Cannot load task repository".to_string())?
                .ok_or("Task project is missing")?;
            if project.workspace_id != Some(task.workspace_id) {
                return Err("Task project belongs to another workspace".to_string());
            }
            if let Some(url) = project.github_repo_url {
                let url = GitService::repository_url(&url).map_err(|error| error.to_string())?;
                repositories.push(Self {
                    url,
                    token: project.github_access_token,
                });
            }
        }
        Self::select(requested, repositories)
    }

    fn select(requested: Option<String>, repositories: Vec<Self>) -> Result<Option<Self>, String> {
        if let Some(url) = requested {
            let token = repositories
                .into_iter()
                .find(|repository| repository.url == url)
                .and_then(|repository| repository.token);
            return Ok(Some(Self { url, token }));
        }
        let mut repositories = repositories.into_iter();
        let selected = repositories.next();
        if let Some(selected) = &selected
            && repositories.any(|repository| repository.url != selected.url)
        {
            return Err("Task has multiple repositories; select one explicitly".to_string());
        }
        Ok(selected)
    }
}

pub struct Checkout {
    path: PathBuf,
}

impl Checkout {
    pub async fn prepare(pool: &PgPool, task: &TaskRow, run: Uuid) -> Result<Self, String> {
        let repository = Repository::resolve(pool, task).await?;
        let checkout = Self::create(run).map_err(|_| "Cannot create task checkout".to_string())?;
        if let Some(repository) = repository {
            GitService::new()
                .clone_repository(
                    &repository.url,
                    checkout.path(),
                    repository.token.as_deref(),
                )
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(checkout)
    }

    fn create(run: Uuid) -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!("zone-run-{run}-{}", Uuid::new_v4()));
        let mut directory = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            directory.mode(0o700);
        }
        directory.create(&path)?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::error!(path = %self.path.display(), "Failed to remove task checkout");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_checkout_exists_and_is_isolated_for_each_run() {
        let run = Uuid::new_v4();
        let first = Checkout::create(run).unwrap();
        let second = Checkout::create(run).unwrap();
        assert!(first.path().is_dir());
        assert!(second.path().is_dir());
        assert_ne!(first.path(), second.path());
        std::fs::write(first.path().join("sentinel"), "first").unwrap();
        assert!(!second.path().join("sentinel").exists());
        let path = first.path().to_path_buf();
        drop(first);
        assert!(!path.exists());
        assert!(second.path().exists());
    }

    #[test]
    fn credentials_are_never_used_for_another_repository() {
        let repository = Repository::select(
            Some("https://github.com/owner/requested.git".to_string()),
            vec![Repository {
                url: "https://github.com/owner/other.git".to_string(),
                token: Some("secret".to_string()),
            }],
        )
        .unwrap()
        .unwrap();
        assert!(repository.token.is_none());
    }

    #[test]
    fn multiple_project_repositories_require_an_explicit_selection() {
        assert!(
            Repository::select(
                None,
                vec![
                    Repository {
                        url: "https://github.com/owner/first.git".to_string(),
                        token: None
                    },
                    Repository {
                        url: "https://github.com/owner/second.git".to_string(),
                        token: None
                    }
                ]
            )
            .is_err()
        );
    }
}
