//! Where a chat that belongs to a project starts: its source on this machine, a checkout of its
//! repository in a mounted folder, or a clone of Zone's own. A chat that cannot have that starts
//! where any chat does, and nothing here confines it to the directory it starts in.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use abnegate_secret::SecretValue;
use abnegate_vcs::RepositoryUrl;
use nix::fcntl::{Flock, FlockArg};
use serde_json::{Value, json};
use sqlx::PgPool;
use tempfile::TempDir;
use uuid::Uuid;
use zone_core::llm::LlmBackend;
use zone_core::tools::ToolResult;
use zone_server::db::projects::{self, NewProject};
use zone_server::db::{chats, organizations, sources, workspace_host_directories, workspaces};
use zone_server::services::chat::session::{self, Mode, Preparation};
use zone_server::services::project_directory::{Missing, Start, locate};
use zone_server::services::route::Route;
use zone_server::state::AppState;

use crate::common::{context_database_url, create_test_state, test_config};

const REMOTE: &str = "git@github.com:owner/repository.git";
const GITHUB: &str = "https://github.com/owner/repository";
const TIMEOUT: Duration = Duration::from_secs(120);
const UNREACHABLE: &str = "http://127.0.0.1:9";
const TOKEN: &str = "ghp_a-token-nothing-may-repeat";

struct Fixture {
    pool: PgPool,
    state: AppState,
    organization: Uuid,
    workspace: Uuid,
    clones: TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let pool = PgPool::connect(&context_database_url())
            .await
            .expect("the test database");
        let suffix = Uuid::new_v4().simple().to_string();
        let organization = organizations::create_organization(&pool, "Directory", &suffix, None)
            .await
            .expect("an organization");
        let workspace =
            workspaces::create_workspace(&pool, organization.id, "Directory", &suffix, None)
                .await
                .expect("a workspace");
        let clones = TempDir::new().expect("a clone root");
        let mut config = test_config();
        config.litellm_host = UNREACHABLE.to_string();
        config.ollama_host = UNREACHABLE.to_string();
        config.clones = clones.path().to_path_buf();
        Self {
            state: create_test_state(config, pool.clone()),
            pool,
            organization: organization.id,
            workspace: workspace.id,
            clones,
        }
    }

    async fn remove(&self) {
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(self.organization)
            .execute(&self.pool)
            .await
            .expect("the organization to be removed");
    }

    async fn mount(&self, folders: &[&Path]) {
        let folders: Vec<String> = folders
            .iter()
            .map(|folder| folder.to_string_lossy().into_owned())
            .collect();
        workspace_host_directories::upsert(&self.pool, self.workspace, &folders)
            .await
            .expect("the workspace's folders");
    }

    async fn source(&self, kind: &str, config: Value, token: Option<&str>) -> Uuid {
        let sealed = token.map(|token| {
            zone_server::crypto::encrypt(self.state.encryption_key(), token).expect("a token")
        });
        sources::create_source(
            &self.pool,
            self.workspace,
            &Uuid::new_v4().to_string(),
            kind,
            config,
            None,
            None,
            sealed.as_deref(),
        )
        .await
        .expect("a source")
        .id
    }

    async fn project(&self, source: Option<Uuid>) -> Uuid {
        projects::create(
            &self.pool,
            NewProject {
                name: "Project",
                description: None,
                workspace_id: Some(self.workspace),
                source_id: source,
                status: None,
            },
        )
        .await
        .expect("a project")
        .id
    }

    async fn chat(&self, project: Option<Uuid>) -> chats::ChatRow {
        chats::create_purposed_chat(
            &self.pool,
            self.workspace,
            "Chat",
            "sonnet",
            chats::ChatPurpose::Assistant,
            project,
            true,
        )
        .await
        .expect("a chat")
    }

    async fn start(&self, chat: &chats::ChatRow) -> Start {
        locate(&self.state, chat.id, self.workspace).await
    }

    async fn preview(&self, chat: &chats::ChatRow) -> Preparation {
        let endpoint = Route::for_workspace(&self.state, self.workspace)
            .await
            .into_endpoint()
            .expect("a usable route");
        session::build(
            &self.state,
            chat,
            Uuid::new_v4(),
            None,
            Mode::Preview,
            endpoint,
        )
        .await
        .expect("a preview builds")
    }

    /// The chat of a project whose source is `kind` with `config`.
    async fn sourced(&self, kind: &str, config: Value) -> chats::ChatRow {
        let source = self.source(kind, config, None).await;
        let project = self.project(Some(source)).await;
        self.chat(Some(project)).await
    }

    async fn on_github(&self) -> chats::ChatRow {
        self.sourced("github", json!({"owner": "owner", "repo": "repository"}))
            .await
    }

    fn missing(&self, url: RepositoryUrl, token: Option<&str>) -> Missing {
        Missing::new(
            self.clones.path(),
            self.workspace,
            url,
            token.map(SecretValue::new),
            TIMEOUT,
        )
    }
}

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.test")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.test")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A checkout at `path`, with a commit, a remote and work nobody committed.
fn checkout(path: &Path, remote: &str) -> PathBuf {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "--quiet"]);
    fs::write(path.join("committed"), "committed").unwrap();
    git(path, &["add", "committed"]);
    git(path, &["commit", "--quiet", "-m", "first"]);
    git(path, &["remote", "add", "origin", remote]);
    fs::write(path.join("committed"), "changed, not committed").unwrap();
    fs::write(path.join("untracked"), "untracked").unwrap();
    path.to_path_buf()
}

/// A bare repository to clone, on the local disk, with one commit.
fn remote() -> (TempDir, RepositoryUrl) {
    let directory = TempDir::new().unwrap();
    let bare = directory.path().join("remote.git");
    let seed = directory.path().join("seed");
    fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "--quiet"]);
    fs::write(seed.join("cloned"), "cloned").unwrap();
    git(&seed, &["add", "cloned"]);
    git(&seed, &["commit", "--quiet", "-m", "first"]);
    git(
        directory.path(),
        &["clone", "--quiet", "--bare", "seed", "remote.git"],
    );
    let url = RepositoryUrl::local(&bare).expect("a local repository");
    (directory, url)
}

fn started_in(start: Start) -> PathBuf {
    match start {
        Start::Directory(directory) => directory,
        Start::Elsewhere => panic!("the chat starts where any chat does"),
        Start::Missing(_) => panic!("the chat has a repository to clone"),
        Start::Unavailable(reason) => panic!("the chat is displaced: {reason}"),
    }
}

/// Directory a preview, the prompt and the tools agree on.
fn agreed(preparation: &Preparation) -> PathBuf {
    let tools = preparation.tools.working_directory().to_path_buf();
    assert_eq!(
        preparation.environment.directory, tools,
        "the prompt and the tools name different directories"
    );
    tools
}

#[tokio::test]
async fn a_folder_source_starts_the_chat_in_that_folder() {
    let fixture = Fixture::new().await;
    let (project, other) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fixture.mount(&[other.path()]).await;
    let chat = fixture
        .sourced("filesystem", json!({"base_path": project.path()}))
        .await;

    let start = fixture.start(&chat).await;
    let preview = fixture.preview(&chat).await;
    fixture.remove().await;

    assert_eq!(started_in(start), project.path());
    assert_eq!(agreed(&preview), project.path());
}

#[tokio::test]
async fn a_folder_that_is_gone_leaves_the_chat_in_the_workspaces_folder_and_says_why() {
    let fixture = Fixture::new().await;
    let (mounted, removed) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let gone = removed.path().join("gone");
    fixture.mount(&[mounted.path()]).await;
    let chat = fixture
        .sourced("filesystem", json!({"base_path": gone}))
        .await;

    let start = fixture.start(&chat).await;
    let preview = fixture.preview(&chat).await;
    fixture.remove().await;

    let Start::Unavailable(reason) = start else {
        panic!("a missing folder must not be started in");
    };
    assert!(reason.contains(gone.to_str().unwrap()), "{reason}");
    assert_eq!(agreed(&preview), mounted.path());
}

#[tokio::test]
async fn a_chat_with_no_project_starts_where_it_always_did() {
    let fixture = Fixture::new().await;
    let mounted = TempDir::new().unwrap();
    fixture.mount(&[mounted.path()]).await;
    let chat = fixture.chat(None).await;

    let start = fixture.start(&chat).await;
    let preview = fixture.preview(&chat).await;
    fixture.remove().await;

    assert!(matches!(start, Start::Elsewhere));
    assert_eq!(agreed(&preview), mounted.path());
}

#[tokio::test]
async fn a_source_with_no_local_form_starts_the_chat_where_it_always_did() {
    let fixture = Fixture::new().await;
    let mounted = TempDir::new().unwrap();
    fixture.mount(&[mounted.path()]).await;
    let chat = fixture
        .sourced("web", json!({"url": "https://example.test"}))
        .await;

    let start = fixture.start(&chat).await;
    fixture.remove().await;

    assert!(matches!(start, Start::Elsewhere));
}

#[tokio::test]
async fn a_repository_checked_out_in_a_mount_is_used_and_left_exactly_as_it_was() {
    let fixture = Fixture::new().await;
    let mounted = TempDir::new().unwrap();
    let path = checkout(&mounted.path().join("code/renamed"), REMOTE);
    fixture.mount(&[mounted.path()]).await;
    let chat = fixture.on_github().await;
    let before = (
        git(&path, &["status", "--porcelain=v2", "--branch"]),
        git(&path, &["rev-parse", "HEAD"]),
        git(&path, &["remote", "-v"]),
        fs::read(path.join(".git/config")).unwrap(),
        fs::read_to_string(path.join("committed")).unwrap(),
    );

    let start = fixture.start(&chat).await;
    let preview = fixture.preview(&chat).await;
    fixture.remove().await;

    assert_eq!(started_in(start), path);
    assert_eq!(agreed(&preview), path);
    let after = (
        git(&path, &["status", "--porcelain=v2", "--branch"]),
        git(&path, &["rev-parse", "HEAD"]),
        git(&path, &["remote", "-v"]),
        fs::read(path.join(".git/config")).unwrap(),
        fs::read_to_string(path.join("committed")).unwrap(),
    );
    assert_eq!(after, before, "finding a checkout must not change it");
    assert!(
        !fixture
            .clones
            .path()
            .join(fixture.workspace.to_string())
            .exists(),
        "a repository already in a mount must not be cloned"
    );
}

#[tokio::test]
async fn a_folder_named_for_the_repository_with_another_origin_is_not_its_checkout() {
    let fixture = Fixture::new().await;
    let mounted = TempDir::new().unwrap();
    checkout(
        &mounted.path().join("repository"),
        "git@github.com:someone-else/repository.git",
    );
    fixture.mount(&[mounted.path()]).await;
    let chat = fixture.on_github().await;

    let start = fixture.start(&chat).await;
    fixture.remove().await;

    assert!(matches!(start, Start::Missing(_)));
}

#[tokio::test]
async fn a_repository_nobody_has_cloned_is_cloned_for_its_workspace_and_previews_never_clone() {
    let fixture = Fixture::new().await;
    let mounted = TempDir::new().unwrap();
    fixture.mount(&[mounted.path()]).await;
    let chat = fixture.on_github().await;

    let start = fixture.start(&chat).await;
    let preview = fixture.preview(&chat).await;
    fixture.remove().await;

    let Start::Missing(missing) = start else {
        panic!("an uncloned repository must be cloned");
    };
    assert_eq!(missing.url().as_str(), format!("{GITHUB}.git"));
    assert!(
        missing
            .destination()
            .starts_with(fixture.clones.path().join(fixture.workspace.to_string())),
        "a clone belongs to its workspace: {}",
        missing.destination().display()
    );
    assert!(!missing.present());
    assert_eq!(agreed(&preview), mounted.path());
    assert!(
        !fixture
            .clones
            .path()
            .join(fixture.workspace.to_string())
            .exists(),
        "a preview must not clone"
    );
}

#[tokio::test]
async fn a_project_linked_to_a_repository_with_no_source_starts_in_its_checkout() {
    let fixture = Fixture::new().await;
    let mounted = TempDir::new().unwrap();
    let path = checkout(&mounted.path().join("repository"), REMOTE);
    fixture.mount(&[mounted.path()]).await;
    let project = fixture.project(None).await;
    projects::link_github(&fixture.pool, project, GITHUB, None)
        .await
        .expect("a linked repository");
    let chat = fixture.chat(Some(project)).await;

    let start = fixture.start(&chat).await;
    fixture.remove().await;

    assert_eq!(started_in(start), path);
}

#[tokio::test]
async fn a_source_comes_before_the_repository_linked_to_its_project() {
    let fixture = Fixture::new().await;
    let (folder, mounted) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    checkout(&mounted.path().join("repository"), REMOTE);
    fixture.mount(&[mounted.path()]).await;
    let source = fixture
        .source("filesystem", json!({"path": folder.path()}), None)
        .await;
    let project = fixture.project(Some(source)).await;
    projects::link_github(&fixture.pool, project, GITHUB, None)
        .await
        .expect("a linked repository");
    let chat = fixture.chat(Some(project)).await;

    let start = fixture.start(&chat).await;
    fixture.remove().await;

    assert_eq!(started_in(start), folder.path());
}

#[tokio::test]
async fn a_source_that_is_gone_gives_way_to_the_repository_linked_to_its_project() {
    let fixture = Fixture::new().await;
    let (removed, mounted) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let path = checkout(&mounted.path().join("repository"), REMOTE);
    fixture.mount(&[mounted.path()]).await;
    let source = fixture
        .source(
            "filesystem",
            json!({"path": removed.path().join("gone")}),
            None,
        )
        .await;
    let project = fixture.project(Some(source)).await;
    projects::link_github(&fixture.pool, project, GITHUB, None)
        .await
        .expect("a linked repository");
    let chat = fixture.chat(Some(project)).await;

    let start = fixture.start(&chat).await;
    fixture.remove().await;

    assert_eq!(started_in(start), path);
}

#[tokio::test]
async fn a_gitlab_repository_is_found_in_a_mount_and_otherwise_not_cloned() {
    let fixture = Fixture::new().await;
    let mounted = TempDir::new().unwrap();
    fixture.mount(&[mounted.path()]).await;
    let chat = fixture
        .sourced(
            "gitlab",
            json!({"project_id": "group/project", "host": "https://git.example"}),
        )
        .await;

    let absent = fixture.start(&chat).await;
    let path = checkout(
        &mounted.path().join("project"),
        "git@git.example:group/project.git",
    );
    let present = fixture.start(&chat).await;
    fixture.remove().await;

    assert!(matches!(absent, Start::Unavailable(_)));
    assert_eq!(started_in(present), path);
}

#[tokio::test]
async fn an_inactive_source_gives_the_chat_nothing() {
    let fixture = Fixture::new().await;
    let folder = TempDir::new().unwrap();
    let source = fixture
        .source("filesystem", json!({"path": folder.path()}), None)
        .await;
    sqlx::query("UPDATE sources SET is_active = false WHERE id = $1")
        .bind(source)
        .execute(&fixture.pool)
        .await
        .expect("the source deactivated");
    let project = fixture.project(Some(source)).await;
    let chat = fixture.chat(Some(project)).await;

    let start = fixture.start(&chat).await;
    fixture.remove().await;

    assert!(matches!(start, Start::Elsewhere));
}

#[tokio::test]
async fn a_folder_inside_the_agents_state_is_refused() {
    let fixture = Fixture::new().await;
    let state = fixture.state.config().agents.state.clone();
    fs::create_dir_all(&state).unwrap();
    let chat = fixture.sourced("filesystem", json!({"path": state})).await;

    let start = fixture.start(&chat).await;
    fixture.remove().await;

    assert!(matches!(start, Start::Unavailable(_)));
}

#[tokio::test]
async fn a_repository_is_cloned_once_and_reused_by_later_chats() {
    let fixture = Fixture::new().await;
    let (holder, url) = remote();

    let cloned = fixture
        .missing(url.clone(), None)
        .clone_repository()
        .await
        .expect("a clone");
    assert_eq!(fs::read_to_string(cloned.join("cloned")).unwrap(), "cloned");
    assert_eq!(
        git(&cloned, &["remote", "get-url", "origin"]).trim(),
        url.as_str()
    );
    fs::write(cloned.join("work"), "the first chat's work").unwrap();
    drop(holder);

    let later = fixture.missing(url.clone(), None);
    assert!(later.present(), "a finished clone is found again");
    let reused = later
        .clone_repository()
        .await
        .expect("a clone needs no remote once it is there");
    fixture.remove().await;

    assert_eq!(reused, cloned);
    assert_eq!(
        fs::read_to_string(reused.join("work")).unwrap(),
        "the first chat's work"
    );
}

#[tokio::test]
async fn a_later_chat_starts_in_the_clone_a_former_one_made() {
    let fixture = Fixture::new().await;
    let mounted = TempDir::new().unwrap();
    fixture.mount(&[mounted.path()]).await;
    let chat = fixture.on_github().await;
    let Start::Missing(missing) = fixture.start(&chat).await else {
        panic!("an uncloned repository must be cloned");
    };
    let destination = missing.destination().to_path_buf();
    checkout(&destination, REMOTE);

    let start = fixture.start(&chat).await;
    let preview = fixture.preview(&chat).await;
    fixture.remove().await;

    assert_eq!(started_in(start), destination);
    assert_eq!(agreed(&preview), destination);
}

#[tokio::test]
async fn one_workspaces_clone_is_not_anothers() {
    let (fixture, other) = (Fixture::new().await, Fixture::new().await);
    let (_holder, url) = remote();
    let root = fixture.clones.path();

    let one = Missing::new(root, fixture.workspace, url.clone(), None, TIMEOUT);
    let two = Missing::new(root, other.workspace, url, None, TIMEOUT);
    fixture.remove().await;
    other.remove().await;

    assert_ne!(one.destination(), two.destination());
}

#[tokio::test]
async fn a_clone_that_fails_says_why_without_its_token_and_a_later_one_can_succeed() {
    let fixture = Fixture::new().await;
    let (holder, url) = remote();
    let absent = RepositoryUrl::local(&holder.path().join("absent.git")).expect("a local path");

    let failed = fixture
        .missing(absent.clone(), Some(TOKEN))
        .clone_repository()
        .await;
    let retried = fixture.missing(absent, Some(TOKEN));
    let present = retried.present();
    let working = fixture.missing(url, Some(TOKEN)).clone_repository().await;
    fixture.remove().await;

    let reason = failed.expect_err("a repository that isn't there cannot be cloned");
    assert!(reason.starts_with("Zone couldn't clone "), "{reason}");
    assert!(!reason.contains(TOKEN), "{reason}");
    assert!(!present, "a failed clone must not count as present");
    let working = working.expect("a token over a local path is no obstacle");
    assert!(
        !fs::read_to_string(working.join(".git/config"))
            .unwrap()
            .contains(TOKEN),
        "a token must not reach the clone's configuration"
    );
}

#[tokio::test]
async fn chats_starting_together_all_get_one_clone() {
    let fixture = Fixture::new().await;
    let (_holder, url) = remote();

    let mut starting = Vec::new();
    for _ in 0..8 {
        let missing = fixture.missing(url.clone(), None);
        starting.push(tokio::spawn(missing.clone_repository()));
    }
    let mut cloned = Vec::new();
    for start in starting {
        cloned.push(start.await.unwrap());
    }
    fixture.remove().await;

    for clone in cloned {
        assert_eq!(
            clone.as_deref(),
            Ok(fixture.missing(url.clone(), None).destination())
        );
    }
    let entries: Vec<_> = fs::read_dir(fixture.clones.path().join(fixture.workspace.to_string()))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| !name.ends_with(".lock"))
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "one clone and nothing left over: {entries:?}"
    );
}

/// A chat that finds the repository being cloned waits, and then uses the clone it was waiting
/// for: with the remote gone by then, any clone of its own would fail.
#[tokio::test]
async fn a_chat_waiting_on_another_chats_clone_uses_it_and_never_clones() {
    let fixture = Fixture::new().await;
    let (holder, url) = remote();
    let missing = fixture.missing(url.clone(), None);
    let destination = missing.destination().to_path_buf();
    let lock = locked(&destination);
    let waiting = tokio::spawn(missing.clone_repository());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!waiting.is_finished(), "the lock is held by another chat");

    git(
        holder.path(),
        &[
            "clone",
            "--quiet",
            "remote.git",
            destination.to_str().unwrap(),
        ],
    );
    fs::remove_dir_all(holder.path().join("remote.git")).unwrap();
    drop(lock);
    let cloned = waiting.await.unwrap();
    fixture.remove().await;

    assert_eq!(cloned.as_deref(), Ok(destination.as_path()));
}

/// What Stop does to a turn: it drops the future. A clone waiting for the lock gives way at once
/// and takes nothing with it.
#[tokio::test]
async fn stopping_a_chat_that_is_waiting_on_a_clone_ends_it_at_once() {
    let fixture = Fixture::new().await;
    let (_holder, url) = remote();
    let missing = fixture.missing(url.clone(), None);
    let destination = missing.destination().to_path_buf();
    let lock = locked(&destination);

    let started = Instant::now();
    let stopped =
        tokio::time::timeout(Duration::from_millis(100), missing.clone_repository()).await;
    assert!(stopped.is_err(), "the lock is held by another chat");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "Stop waited on the lock"
    );
    drop(lock);

    let cloned = fixture.missing(url, None).clone_repository().await;
    fixture.remove().await;
    assert_eq!(cloned.as_deref(), Ok(destination.as_path()));
}

#[tokio::test]
async fn a_partial_clone_left_behind_does_not_count_and_is_replaced() {
    let fixture = Fixture::new().await;
    let (_holder, url) = remote();
    let missing = fixture.missing(url.clone(), None);
    let staging = missing.destination().with_extension("staging");
    fs::create_dir_all(staging.join(".git")).unwrap();
    fs::write(staging.join("partial"), "partial").unwrap();
    fs::set_permissions(
        staging.parent().unwrap(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();

    assert!(!missing.present(), "a stopped clone must not count");
    let cloned = missing.clone_repository().await.expect("a clone");
    fixture.remove().await;

    assert!(cloned.join("cloned").is_file());
    assert!(!cloned.join("partial").exists());
    assert!(!staging.exists());
}

#[tokio::test]
async fn something_else_where_the_clone_goes_is_never_replaced() {
    let fixture = Fixture::new().await;
    let (_holder, url) = remote();
    let missing = fixture.missing(url, None);
    let destination = missing.destination().to_path_buf();
    fs::create_dir_all(&destination).unwrap();
    fs::set_permissions(
        destination.parent().unwrap(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    fs::write(destination.join("work"), "someone's work").unwrap();

    let failed = missing.clone_repository().await;
    fixture.remove().await;

    assert!(failed.unwrap_err().contains("move it aside"));
    assert_eq!(
        fs::read_to_string(destination.join("work")).unwrap(),
        "someone's work"
    );
}

/// Starting somewhere is not being confined there: relative paths resolve from the project, and
/// another mounted project is still reached by its absolute path, by the file tools and the shell.
#[tokio::test]
async fn other_mounted_folders_stay_reachable_from_a_project_chat() {
    let fixture = Fixture::new().await;
    let (project, other) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::write(project.path().join("here"), "in the project").unwrap();
    fs::write(other.path().join("there"), "in another project").unwrap();
    fixture.mount(&[other.path(), project.path()]).await;
    let chat = fixture
        .sourced("filesystem", json!({"path": project.path()}))
        .await;
    let endpoint = Route::for_workspace(&fixture.state, fixture.workspace)
        .await
        .into_endpoint()
        .expect("a usable route");
    let mut generation = session::Generation::unrouted(LlmBackend::Http);
    generation.directory = Some(project.path().to_path_buf());
    let preparation = session::build(
        &fixture.state,
        &chat,
        Uuid::new_v4(),
        None,
        Mode::Generation(generation),
        endpoint,
    )
    .await
    .expect("a turn is prepared");
    let tools = &preparation.tools;

    let relative = tools
        .execute("read_file", &json!({"path": "here"}).to_string())
        .await;
    let absolute = tools
        .execute(
            "read_file",
            &json!({"path": other.path().join("there")}).to_string(),
        )
        .await;
    let written = tools
        .execute(
            "write_file",
            &json!({"path": other.path().join("written"), "content": "from the project's chat"})
                .to_string(),
        )
        .await;
    let shell = tools
        .execute(
            "run_shell",
            &json!({"command": format!("pwd -P; cat '{}'", other.path().join("there").display())})
                .to_string(),
        )
        .await;
    fixture.remove().await;

    assert!(relative.success, "{:?}", relative.error);
    assert!(
        output(&relative).contains("in the project"),
        "{:?}",
        relative.output
    );
    assert!(absolute.success, "{:?}", absolute.error);
    assert!(
        output(&absolute).contains("in another project"),
        "{:?}",
        absolute.output
    );
    assert!(written.success, "{:?}", written.error);
    assert_eq!(
        fs::read_to_string(other.path().join("written")).unwrap(),
        "from the project's chat"
    );
    assert!(shell.success, "{:?}", shell.error);
    let physical = fs::canonicalize(project.path()).unwrap();
    assert!(
        output(&shell).contains(physical.to_str().unwrap()),
        "{:?}",
        shell.output
    );
    assert!(
        output(&shell).contains("in another project"),
        "{:?}",
        shell.output
    );
}

fn output(result: &ToolResult) -> &str {
    result.output.as_deref().unwrap_or_default()
}

/// Another chat's hold on a repository's lock, as the lock file beside its clone.
fn locked(destination: &Path) -> Flock<fs::File> {
    let workspace = destination.parent().unwrap();
    fs::create_dir_all(workspace).unwrap();
    fs::set_permissions(
        workspace,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(destination.with_extension("lock"))
        .unwrap();
    Flock::lock(file, FlockArg::LockExclusive)
        .map_err(|(_, errno)| errno)
        .expect("the lock")
}
