//! Where a chat that belongs to a project starts: the project's source, on this machine.
//!
//! A folder source is that folder. A repository is a checkout of it in one of the folders its
//! workspace mounted, used as it stands, and otherwise a clone of Zone's own. The directory is
//! where relative paths resolve and nothing more: a chat reaches every other folder it could
//! reach before by its absolute path.

mod candidate;
mod missing;
mod origin;
mod scan;
mod search;
mod start;

pub use missing::Missing;
pub use origin::Origin;
pub use start::Start;

use uuid::Uuid;

use crate::db::chats;
use crate::db::projects::{self, ProjectRow};
use crate::db::sources;
use crate::services::github_app::Credential;
use crate::state::AppState;
use candidate::Candidate;
use search::Search;

/// The directory a turn of `chat` starts in. Nothing here fails a turn: a project that cannot be
/// read leaves the chat [`Start::Elsewhere`].
pub async fn locate(state: &AppState, chat: Uuid, workspace: Uuid) -> Start {
    find(state, chat, workspace, Search::Walk).await
}

/// [`locate`] for a preview, which runs on every paused keystroke and every chat opened: it uses
/// a checkout a turn already found rather than walking the mounts for one.
pub async fn peek(state: &AppState, chat: Uuid, workspace: Uuid) -> Start {
    find(state, chat, workspace, Search::Known).await
}

/// Whether `directory` is one of `workspace`'s: under a folder it mounted, or under the clones
/// Zone made for it. A chat may only name a checkout of its own workspace for Zone to act on.
pub async fn owns(state: &AppState, workspace: Uuid, directory: &std::path::Path) -> bool {
    let config = state.config();
    let clones = config.clones.join(workspace.as_hyphenated().to_string());
    if candidate::within(directory, &clones) {
        return true;
    }
    candidate::folders(state, workspace)
        .await
        .iter()
        .filter_map(|host| config.host_mounts.mapped_directory(host).ok())
        .any(|root| candidate::within(directory, &root))
}

async fn find(state: &AppState, chat: Uuid, workspace: Uuid, search: Search) -> Start {
    let Some(project) = project(state, chat, workspace).await else {
        return Start::Elsewhere;
    };
    let mut unavailable = None;
    for candidate in candidates(state, &project, workspace).await {
        match candidate.start(state, workspace, search).await {
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
/// A credential that cannot be resolved is not presented, and a clone then fails or succeeds on
/// what the repository allows anyone.
async fn candidates(state: &AppState, project: &ProjectRow, workspace: Uuid) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    if let Some(id) = project.source_id {
        match sources::get_source(state.db(), id, workspace).await {
            Ok(Some(source)) if source.is_active.unwrap_or(true) => {
                let token = match state.github().source(state.db(), &source).await {
                    Ok(credential) => credential.map(Credential::into_secret),
                    Err(error) => {
                        tracing::warn!(source = %source.id, %error, "Could not resolve the source's credential");
                        None
                    }
                };
                candidates.extend(Candidate::of_source(&source, token));
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(project = %project.id, %error, "Could not read the project's source");
            }
        }
    }
    if let Some(url) = &project.github_repo_url {
        let token = match state.github().project(state.db(), project).await {
            Ok(credential) => credential.map(Credential::into_secret),
            Err(error) => {
                tracing::warn!(project = %project.id, %error, "Could not resolve the project's credential");
                None
            }
        };
        candidates.extend(Candidate::of_project(url, token));
    }
    candidates
}
