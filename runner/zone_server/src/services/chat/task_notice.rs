//! What a run started from a chat says in that chat when it ends.
//!
//! A task run is started and then nobody is told anything. The reader who
//! asked for it watches the Tasks box, and whoever started it — an agent in
//! the chat, usually — has already finished its turn and never learns the
//! outcome. Auto-fix has said how its runs ended since it was built
//! ([`super::pull_request::auto_fix`]); this says the same for a run that a
//! chat started on purpose.
//!
//! One notice per run, written when the run has actually settled. A run whose
//! completion could not be written is left unannounced: it wants its owner's
//! lease, and one that lost its heartbeat matches no row, so its status
//! column is not evidence that it stopped.

use uuid::Uuid;

use crate::db::{chats, tasks};
use crate::state::AppState;

/// How a notice about a task run is marked, so the chat can draw it as
/// something an automation wrote rather than a turn somebody typed.
pub const TASK_RUN: &str = "task_run";

/// What one started run needs in order to say how it ended, kept so the chat
/// can be told after the run itself is over.
#[derive(Clone, Debug)]
pub struct Finished {
    pub chat_id: Uuid,
    pub task_id: Uuid,
    pub title: String,
}

impl Finished {
    /// Say in the chat how the run ended, reading its settled row rather than
    /// assuming it got that far.
    ///
    /// Never fails the run: a notice that cannot be stored is logged and
    /// dropped, because the run's own result is already recorded on the task.
    pub async fn say(&self, state: &AppState, run_id: Uuid) {
        let run = match tasks::get_task_run(state.db(), run_id).await {
            Ok(Some(run)) => run,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(%run_id, %error, "could not read how a task run ended");
                return;
            }
        };
        if run.completed_at.is_none() {
            return;
        }
        let content = self.content(&run.status, run.error_message.as_deref());
        let metadata = serde_json::json!({
            "source": TASK_RUN,
            "task_id": self.task_id,
            "run_id": run_id,
            "status": run.status,
        });
        match chats::announce(state.db(), self.chat_id, &content, metadata).await {
            Ok(stored) => crate::db::actions::publish(self.chat_id, stored),
            Err(error) => {
                tracing::warn!(%run_id, %error, "could not say in the chat how a task run ended");
            }
        }
    }

    /// What the chat is told. A run that ended any way but completed says so
    /// and why, because that is the outcome somebody has to act on.
    fn content(&self, status: &str, error: Option<&str>) -> String {
        let title = self.title.trim();
        match status {
            tasks::COMPLETED => format!("The task “{title}” finished."),
            status => {
                let reason = error
                    .map(str::trim)
                    .filter(|reason| !reason.is_empty())
                    .map(|reason| format!(": {reason}"))
                    .unwrap_or_default();
                format!("The task “{title}” ended as {status}{reason}.")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finished() -> Finished {
        Finished {
            chat_id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            title: "Review abnegate/zone #276".to_string(),
        }
    }

    #[test]
    fn a_completed_run_is_announced_by_its_task() {
        assert_eq!(
            finished().content(tasks::COMPLETED, None),
            "The task “Review abnegate/zone #276” finished."
        );
    }

    #[test]
    fn a_run_that_ended_any_other_way_says_so_and_why() {
        assert_eq!(
            finished().content("failed", Some("the branch was held")),
            "The task “Review abnegate/zone #276” ended as failed: the branch was held."
        );
        assert_eq!(
            finished().content("cancelled", None),
            "The task “Review abnegate/zone #276” ended as cancelled."
        );
    }

    #[test]
    fn an_error_of_nothing_but_space_adds_no_empty_reason() {
        assert_eq!(
            finished().content("failed", Some("   ")),
            "The task “Review abnegate/zone #276” ended as failed."
        );
    }
}
