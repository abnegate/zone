//! Where a chat that belongs to a project starts: the project's source, on this machine.
//!
//! A folder source is that folder. A repository is a checkout of it in one of the folders its
//! workspace mounted, used as it stands, and otherwise a clone of Zone's own. The directory is
//! where relative paths resolve and nothing more: a chat reaches every other folder it could
//! reach before by its absolute path.

mod missing;
mod origin;
mod scan;

pub use missing::Missing;
pub use origin::Origin;

use std::path::{Path, PathBuf};

use abnegate_secret::SecretValue;
use abnegate_vcs::{GitService, RepositoryUrl};
use serde_json::Value;
use uuid::Uuid;

use crate::config::Config;
use crate::db::projects::{self, ProjectRow};
use crate::db::sources::{self, SourceRow};
use crate::db::{chats, workspace_host_directories};
use crate::host_mounts::{HostMounts, normalize};
use crate::state::AppState;

const FILESYSTEM: &str = "filesystem";
const GITHUB: &str = "github";
const GITLAB: &str = "gitlab";
const GITLAB_HOST: &str = "https://gitlab.com";

/// Where a chat's turn starts, as far as can be told without touching the network.
pub enum Start {
    /// The chat has no project, or a source with no local form: it starts where any chat does.
    Elsewhere,
    Directory(PathBuf),
    /// A repository to clone first. Only a turn that is about to run does.
    Missing(Missing),
    /// The project names a place this machine cannot offer, and why, which its chat is told.
    Unavailable(String),
}

/// The directory a turn of `chat` starts in. Nothing here fails a turn: a project that cannot be
/// read leaves the chat [`Start::Elsewhere`].
pub async fn locate(state: &AppState, chat: Uuid, workspace: Uuid) -> Start {
    let Some(project) = project(state, chat, workspace).await else {
        return Start::Elsewhere;
    };
    let mut unavailable = None;
    for candidate in candidates(state, &project, workspace).await {
        match candidate.start(state, workspace).await {
            Start::Elsewhere => {}
            Start::Unavailable(reason) => unavailable = unavailable.or(Some(reason)),
            start => return start,
        }
    }
    unavailable.map_or(Start::Elsewhere, Start::Unavailable)
}

async fn project(state: &AppState, chat: Uuid, workspace: Uuid) -> Option<ProjectRow> {
    let link = match chats::link(state.db(), chat).await {
        Ok(link) => link?,
        Err(error) => {
            tracing::warn!(%chat, %error, "Could not read the chat's project");
            return None;
        }
    };
    match projects::get_project_in_workspace(state.db(), link.project_id?, workspace).await {
        Ok(project) => project,
        Err(error) => {
            tracing::warn!(%chat, %error, "Could not read the chat's project");
            None
        }
    }
}

/// What a project names, first to last: its source, then the GitHub repository linked to it.
async fn candidates(state: &AppState, project: &ProjectRow, workspace: Uuid) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    if let Some(id) = project.source_id {
        match sources::get_source(state.db(), id, workspace).await {
            Ok(Some(source)) if source.is_active.unwrap_or(true) => {
                candidates.extend(Candidate::of_source(state.encryption_key(), &source));
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(project = %project.id, %error, "Could not read the project's source");
            }
        }
    }
    if let Some(url) = &project.github_repo_url {
        candidates.extend(Candidate::of_project(
            state.encryption_key(),
            url,
            project.github_access_token.as_deref(),
        ));
    }
    candidates
}

enum Candidate {
    Folder(String),
    Repository {
        origin: Origin,
        /// How to clone it, which only a GitHub repository offers: nothing else has a way to
        /// present a token that keeps it out of the URL.
        clone: Option<(RepositoryUrl, Option<SecretValue>)>,
    },
}

impl Candidate {
    fn of_source(key: &[u8], source: &SourceRow) -> Option<Self> {
        match source.source_type.as_str() {
            FILESYSTEM => text(&source.config, &["base_path", "path"])
                .map(|path| Self::Folder(path.to_string())),
            GITHUB => {
                let owner = text(&source.config, &["owner"])?;
                let repository = text(&source.config, &["repo"])?;
                let url =
                    GitService::repository_url(&format!("https://github.com/{owner}/{repository}"))
                        .ok()?;
                Some(Self::Repository {
                    origin: Origin::parse(url.as_str())?,
                    clone: Some((url, credential(key, source))),
                })
            }
            GITLAB => {
                let project = text(&source.config, &["project", "project_id"])?;
                let host = text(&source.config, &["base_url", "host"]).unwrap_or(GITLAB_HOST);
                let host = host.trim_end_matches('/');
                let remote = match host.contains("://") {
                    true => format!("{host}/{project}"),
                    false => format!("https://{host}/{project}"),
                };
                Some(Self::Repository {
                    origin: Origin::parse(&remote)?,
                    clone: None,
                })
            }
            _ => None,
        }
    }

    fn of_project(key: &[u8], url: &str, token: Option<&str>) -> Option<Self> {
        let url = GitService::repository_url(url).ok()?;
        Some(Self::Repository {
            origin: Origin::parse(url.as_str())?,
            clone: Some((
                url,
                token.map(|stored| SecretValue::new(crate::crypto::open(key, stored))),
            )),
        })
    }

    async fn start(self, state: &AppState, workspace: Uuid) -> Start {
        let config = state.config();
        match self {
            Self::Folder(path) => match folder(&config.host_mounts, &path) {
                Some(directory) => usable(config, directory),
                None => Start::Unavailable(format!(
                    "The project's folder {path} isn't available on this machine."
                )),
            },
            Self::Repository { origin, clone } => {
                let mounted = mounted(state, workspace).await;
                let found = {
                    let origin = origin.clone();
                    tokio::task::spawn_blocking(move || scan::checkout(&mounted, &origin)).await
                };
                if let Ok(Some(directory)) = found {
                    return usable(config, directory);
                }
                let Some((url, token)) = clone else {
                    return Start::Unavailable(format!(
                        "No folder of this workspace holds a checkout of {origin}, and Zone \
                         clones only GitHub repositories. Add a folder that holds one."
                    ));
                };
                let missing =
                    Missing::new(&config.clones, workspace, url, token, config.chat.timeout);
                match missing.present() {
                    true => usable(config, missing.destination().to_path_buf()),
                    false => Start::Missing(missing),
                }
            }
        }
    }
}

fn text<'a>(config: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| config.get(key)?.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// The token a source's repository is cloned with. A credential that cannot be opened is not
/// presented, and the clone then fails or succeeds on what the repository allows anyone.
fn credential(key: &[u8], source: &SourceRow) -> Option<SecretValue> {
    let encrypted = source.credentials_encrypted.as_deref()?;
    match crate::crypto::decrypt(key, encrypted) {
        Ok(token) => Some(SecretValue::new(token)),
        Err(error) => {
            tracing::warn!(source = %source.id, %error, "Could not open the source's credentials");
            None
        }
    }
}

/// The folder a source names: a host path through the mounts, or a path this process opens as it
/// is, which is how the source's own indexing reads it.
fn folder(mounts: &HostMounts, path: &str) -> Option<PathBuf> {
    mounts.mapped_directory(path).ok().or_else(|| {
        let path = normalize(path).ok()?;
        path.is_dir().then_some(path)
    })
}

/// The workspace's folders as this process opens them, those that exist.
async fn mounted(state: &AppState, workspace: Uuid) -> Vec<PathBuf> {
    let directories = match workspace_host_directories::list(state.db(), workspace).await {
        Ok(directories) => directories,
        Err(error) => {
            tracing::warn!(%workspace, %error, "Could not read the workspace's folders");
            return Vec::new();
        }
    };
    let mounts = &state.config().host_mounts;
    directories
        .iter()
        .filter_map(|host| mounts.mapped_directory(host).ok())
        .collect()
}

/// `directory`, unless it lies in the agents' state: every chat's tools are denied that, and
/// a turn must not start among sign-ins it can't read.
fn usable(config: &Config, directory: PathBuf) -> Start {
    if within(&directory, &config.agents.state) {
        return Start::Unavailable(format!(
            "{} is inside Zone's agent state, which a chat can't work in.",
            directory.display()
        ));
    }
    Start::Directory(directory)
}

fn within(directory: &Path, root: &Path) -> bool {
    let real = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    real(directory).starts_with(real(root))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;

    fn source(kind: &str, config: Value) -> SourceRow {
        SourceRow {
            id: Uuid::nil(),
            name: "source".into(),
            source_type: kind.into(),
            config,
            credentials_encrypted: None,
            description: None,
            url: None,
            is_active: Some(true),
            last_verified_at: None,
            last_error: None,
            created_at: None,
            updated_at: None,
            workspace_id: None,
        }
    }

    #[test]
    fn a_folder_is_read_through_the_mounts_then_as_this_process_opens_it() {
        let root = TempDir::new().unwrap();
        let inside = root.path().join("project");
        std::fs::create_dir_all(&inside).unwrap();
        let native = HostMounts::default();
        assert_eq!(
            folder(&native, inside.to_str().unwrap()),
            Some(inside.clone())
        );
        assert_eq!(
            folder(&native, root.path().join("gone").to_str().unwrap()),
            None
        );

        let container = HostMounts {
            host_root: Some(PathBuf::from("/Users/nobody")),
            in_container: true,
        };
        assert_eq!(
            folder(&container, inside.to_str().unwrap()),
            Some(inside),
            "a path the indexer opens as it is opens here too"
        );
        assert_eq!(folder(&container, "relative/path"), None);
        assert_eq!(folder(&container, "/a/../b"), None);
    }

    #[test]
    fn a_source_is_a_folder_or_a_repository_and_anything_else_has_no_directory() {
        let key = [0_u8; 32];
        for config in [json!({"base_path": "/srv"}), json!({"path": "/srv"})] {
            assert!(matches!(
                Candidate::of_source(&key, &source(FILESYSTEM, config)),
                Some(Candidate::Folder(path)) if path == "/srv"
            ));
        }
        assert!(matches!(
            Candidate::of_source(&key, &source(GITHUB, json!({"owner": "o", "repo": "r"}))),
            Some(Candidate::Repository { clone: Some(_), .. })
        ));
        assert!(matches!(
            Candidate::of_source(
                &key,
                &source(GITLAB, json!({"project_id": "g/p", "host": "https://git.example"}))
            ),
            Some(Candidate::Repository { clone: None, origin })
                if Some(origin.clone()) == Origin::parse("git@git.example:g/p.git")
        ));
        assert!(
            Candidate::of_source(
                &key,
                &source(GITHUB, json!({"owner": "--upload-pack=evil", "repo": "r"}))
            )
            .is_none(),
            "an owner GitHub would not allow is never given to git"
        );
        for kind in ["web", "text", "notion"] {
            assert!(Candidate::of_source(&key, &source(kind, json!({}))).is_none());
        }
    }

    #[test]
    fn the_agents_state_is_not_a_place_to_start() {
        let state = TempDir::new().unwrap();
        let inside = state.path().join("organization");
        std::fs::create_dir_all(&inside).unwrap();
        assert!(within(&inside, state.path()));
        assert!(within(state.path(), state.path()));
        assert!(!within(TempDir::new().unwrap().path(), state.path()));
    }
}
