//! Persistent reminder dispatch. Database locks coordinate all server instances.
use crate::{
    db::reminders::{self, Delivered},
    state::AppState,
};
use serde_json::json;
use std::time::Duration;

/// How long a claimed turn is left alone before it is taken to have died with
/// whatever was running it.
///
/// The generation timeout bounds a turn once it starts; this adds the time one
/// can spend queued behind the chat's own semaphore, waiting out whatever the
/// person is saying there. A turn still running when the lease expires is
/// dispatched a second time, which is a repeated question — the failure this
/// whole path exists to avoid is the other one, a firing nobody ever receives.
const QUEUED: Duration = Duration::from_secs(30 * 60);

pub fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let lease = state.config().chat.timeout.saturating_add(QUEUED);
        loop {
            interval.tick().await;
            // Every reminder whose time has come is claimed first. A claim
            // stores a message, or writes down a turn that is still owed, and
            // either way the schedule moves on within the same transaction.
            loop {
                match reminders::deliver_next(state.db()).await {
                    Ok(Delivered::Settled) => {}
                    Ok(Delivered::Nothing) => break,
                    Err(error) => {
                        tracing::warn!(%error, "Reminder dispatch failed; retrying next interval");
                        break;
                    }
                }
            }
            // Then whatever the claims left owing, which is also where a turn
            // abandoned by a restart is picked back up.
            loop {
                match reminders::claim_turn(state.db(), lease).await {
                    Ok(None) => break,
                    Ok(Some(turn)) => {
                        // Spawned rather than awaited: a generation takes as
                        // long as a model takes, and the dispatch tick is what
                        // every other due reminder is waiting on. The chat's
                        // own semaphore already serialises this turn against
                        // anything the person is typing there.
                        let state = state.clone();
                        tokio::spawn(async move {
                            crate::ws::chat::run_turn(
                                &state,
                                turn.chat_id,
                                turn.workspace_id,
                                turn.user_id,
                                &turn.prompt,
                                // Marked, so a reader can tell a turn the
                                // schedule opened from one the person typed.
                                Some(json!({
                                    "source": "reminder",
                                    "reminder_id": turn.reminder_id,
                                    "actor_id": turn.user_id,
                                })),
                            )
                            .await;
                            // Last, and only after the turn has finished: this
                            // is what stops it being offered again, so it is
                            // not owed until there is nothing left to run.
                            if let Err(error) = reminders::finish_turn(state.db(), turn.id).await {
                                tracing::warn!(
                                    %error,
                                    reminder_id = %turn.reminder_id,
                                    "Reminder turn ran but could not be marked done; it will be \
                                     offered again once its claim expires"
                                );
                            }
                        });
                    }
                    Err(error) => {
                        tracing::warn!(%error, "Reminder turn claim failed; retrying next interval");
                        break;
                    }
                }
            }
        }
    })
}
