//! Reading the pull requests chats watch, without waiting to be asked.
//!
//! Auto-fix acts on a status read, and until this sweep existed the only
//! thing that read a chat's pull requests was the chat panel asking for them.
//! A check that started failing while nobody was looking, or a conflict that
//! appeared after a merge to the base branch, reached zone only when somebody
//! next opened that chat.
//!
//! GitHub would rather push than be polled, and it cannot here: the App is
//! registered without `hook_attributes` (see `github_app::manifest`) so that a
//! deployment needs no address GitHub can reach, and most do not have one.
//! Reading on a schedule is what is left.

use futures::{StreamExt, stream};

use crate::db::{DbResult, chat_pull_requests};
use crate::services::chat::pull_request;
use crate::state::AppState;

/// How often every watched pull request is read again.
///
/// A pass costs a few GitHub calls for each one, against the installation's
/// hourly limit, so this is spaced rather than eager. Auto-fix is not made
/// slower in any way that matters by it: the run a pass starts takes far
/// longer than the wait for the pass that starts it.
pub const WATCH_INTERVAL_SECONDS: u64 = 5 * 60;

/// How many are read from GitHub at once.
const CONCURRENT_READS: usize = 4;

/// Reads every watched pull request again, so that auto-fix considers one
/// that broke while nobody was looking at its chat. Answers with how many
/// were read.
///
/// One whose status is still fresh costs nothing: the read returns the cached
/// status, and a cached status has already been through auto-fix, so there is
/// nothing this pass would add to it.
///
/// A pull request that could not be read leaves the others alone. The
/// complaint is still there on the next pass if it was real.
pub async fn run_cycle(state: &AppState) -> DbResult<usize> {
    let watching = chat_pull_requests::watching(state.db()).await?;
    let read = stream::iter(watching)
        .map(|watching| {
            let id = watching.row.id;
            async move {
                if let Err(error) =
                    pull_request::track(state, watching.workspace_id, watching.row).await
                {
                    tracing::warn!(pull_request = %id, %error, "could not read a watched pull request");
                }
            }
        })
        .buffer_unordered(CONCURRENT_READS)
        .count()
        .await;
    Ok(read)
}
