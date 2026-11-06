use std::path::{Path, PathBuf};

use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use abnegate_vcs::{GitService, RepositoryUrl};
use serde_json::Value;
use uuid::Uuid;

use super::search::Search;
use super::{Missing, Origin, Start, scan};
use crate::config::Config;
use crate::db::sources::SourceRow;
use crate::db::workspace_host_directories;
use crate::host_mounts::{CONTAINER_ROOT, HostMounts, normalize};
use crate::secret::Unseal;
use crate::state::AppState;

const FILESYSTEM: &str = "filesystem";
const GITHUB: &str = "github";
const GITLAB: &str = "gitlab";
const GITLAB_HOST: &str = "https://gitlab.com";

/// A place a project names that a chat could start in.
pub(super) enum Candidate {
    Folder(String),
    Repository {
        origin: Origin,
        /// How to clone it, which only a GitHub repository offers: nothing else has a way to
        /// present a token that keeps it out of the URL.
        clone: Option<(RepositoryUrl, Option<SecretValue>)>,
    },
}

impl Candidate {
    pub(super) fn of_source(key: &MasterKey, source: &SourceRow) -> Option<Self> {
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

    pub(super) fn of_project(key: &MasterKey, url: &str, token: Option<&str>) -> Option<Self> {
        let url = GitService::repository_url(url).ok()?;
        Some(Self::Repository {
            origin: Origin::parse(url.as_str())?,
            clone: Some((url, token.map(|stored| key.open_lenient(stored)))),
        })
    }

    /// Where this candidate starts a chat. All of it that touches a disk runs off the runtime,
    /// where a stalled mount holds up one thread and not every workspace's turns, and the caller's
    /// Stop still works.
    pub(super) async fn start(self, state: &AppState, workspace: Uuid, search: Search) -> Start {
        let directories = match &self {
            Self::Folder(_) => Vec::new(),
            Self::Repository { .. } => folders(state, workspace).await,
        };
        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            self.resolve(state.config(), workspace, &directories, search)
        })
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%workspace, %error, "Could not look for the project's directory");
            Start::Elsewhere
        })
    }

    fn resolve(
        self,
        config: &Config,
        workspace: Uuid,
        directories: &[String],
        search: Search,
    ) -> Start {
        match self {
            Self::Folder(path) => match folder(&config.host_mounts, &path) {
                Some(directory) if within(&directory, &config.clones) => {
                    Start::Unavailable(format!(
                        "The project's folder {path} is inside Zone's clones, which belong to \
                         the workspaces they were made for."
                    ))
                }
                Some(directory) => usable(config, directory),
                None => Start::Unavailable(format!(
                    "The project's folder {path} isn't available on this machine."
                )),
            },
            Self::Repository { origin, clone } => {
                let mounted: Vec<PathBuf> = directories
                    .iter()
                    .filter_map(|host| config.host_mounts.mapped_directory(host).ok())
                    .collect();
                if let Some(directory) =
                    scan::find(workspace, &mounted, &config.clones, &origin, search)
                {
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
fn credential(key: &MasterKey, source: &SourceRow) -> Option<SecretValue> {
    let encrypted = source.credentials_encrypted.as_deref()?;
    match key.open_strict(encrypted) {
        Ok(token) => Some(token),
        Err(error) => {
            tracing::warn!(source = %source.id, %error, "Could not open the source's credentials");
            None
        }
    }
}

/// The folder a source names: a host path through the mounts, or a path already under the
/// container's mount of them, which is how a source indexed from inside the container names it.
fn folder(mounts: &HostMounts, path: &str) -> Option<PathBuf> {
    mounts.mapped_directory(path).ok().or_else(|| {
        let path = normalize(path).ok()?;
        (mounts.in_container && path.starts_with(CONTAINER_ROOT) && path.is_dir()).then_some(path)
    })
}

/// The host paths of the workspace's folders.
pub(super) async fn folders(state: &AppState, workspace: Uuid) -> Vec<String> {
    workspace_host_directories::list(state.db(), workspace)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%workspace, %error, "Could not read the workspace's folders");
            Vec::new()
        })
}

/// `directory`, unless it lies in the agents' state, which every chat's tools are denied, or
/// this process can't list and enter it: a CLI can't even be started in a folder it can't enter,
/// which would fail the turn. Reading a folder's `.` is what takes entering it.
fn usable(config: &Config, directory: PathBuf) -> Start {
    if within(&directory, &config.agents.state) {
        return Start::Unavailable(format!(
            "{} is inside Zone's agent state, which a chat can't work in.",
            directory.display()
        ));
    }
    if std::fs::read_dir(&directory).is_err() || std::fs::metadata(directory.join(".")).is_err() {
        return Start::Unavailable(format!(
            "Zone doesn't have permission to open {}.",
            directory.display()
        ));
    }
    Start::Directory(directory)
}

pub(super) fn within(directory: &Path, root: &Path) -> bool {
    let real = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    real(directory).starts_with(real(root))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::secret::fixture;

    const TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyz";
    const REPOSITORY_URL: &str = "https://github.com/owner/repository";

    fn key() -> MasterKey {
        MasterKey::generate().expect("a key")
    }

    fn token(candidate: Option<Candidate>) -> Option<String> {
        match candidate {
            Some(Candidate::Repository {
                clone: Some((_, token)),
                ..
            }) => token.map(|token| token.expose().to_string()),
            _ => panic!("a candidate that clones"),
        }
    }

    fn github_source(credential: &str) -> SourceRow {
        SourceRow {
            credentials_encrypted: Some(credential.to_string()),
            ..source(GITHUB, json!({"owner": "owner", "repo": "repository"}))
        }
    }

    #[test]
    fn a_project_token_opens_whether_sealed_sealed_before_envelopes_or_plain() {
        let key = key();
        for stored in [
            fixture::seal(TOKEN, &key),
            fixture::seal_legacy(TOKEN, &key),
            TOKEN.to_string(),
        ] {
            assert_eq!(
                token(Candidate::of_project(&key, REPOSITORY_URL, Some(&stored))).as_deref(),
                Some(TOKEN),
                "{stored} did not open to the token"
            );
        }
    }

    #[test]
    fn a_source_credential_opens_only_when_sealed() {
        let key = key();
        for sealed in [
            fixture::seal(TOKEN, &key),
            fixture::seal_legacy(TOKEN, &key),
        ] {
            assert_eq!(
                token(Candidate::of_source(&key, &github_source(&sealed))).as_deref(),
                Some(TOKEN),
                "{sealed} did not open to the token"
            );
        }
        assert_eq!(
            token(Candidate::of_source(&key, &github_source(TOKEN))),
            None,
            "a plaintext credential is never presented"
        );
    }

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
    fn a_folder_is_read_through_the_mounts_and_nothing_else_inside_a_container() {
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
            None,
            "a path that happens to exist in the container is not a mounted folder"
        );
        assert_eq!(folder(&container, "relative/path"), None);
        assert_eq!(folder(&container, "/a/../b"), None);
    }

    #[test]
    fn a_source_is_a_folder_or_a_repository_and_anything_else_has_no_directory() {
        for config in [json!({"base_path": "/srv"}), json!({"path": "/srv"})] {
            assert!(matches!(
                Candidate::of_source(&key(), &source(FILESYSTEM, config)),
                Some(Candidate::Folder(path)) if path == "/srv"
            ));
        }
        assert!(matches!(
            Candidate::of_source(&key(), &source(GITHUB, json!({"owner": "o", "repo": "r"}))),
            Some(Candidate::Repository { clone: Some(_), .. })
        ));
        assert!(matches!(
            Candidate::of_source(
                &key(),
                &source(GITLAB, json!({"project_id": "g/p", "host": "https://git.example"}))
            ),
            Some(Candidate::Repository { clone: None, origin })
                if Some(origin.clone()) == Origin::parse("git@git.example:g/p.git")
        ));
        assert!(
            Candidate::of_source(
                &key(),
                &source(GITHUB, json!({"owner": "--upload-pack=evil", "repo": "r"}))
            )
            .is_none(),
            "an owner GitHub would not allow is never given to git"
        );
        for kind in ["web", "text", "notion"] {
            assert!(Candidate::of_source(&key(), &source(kind, json!({}))).is_none());
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
