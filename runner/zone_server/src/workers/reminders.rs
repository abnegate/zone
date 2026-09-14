//! Persistent reminder dispatch. Database locks coordinate all server instances.
use crate::{
    db::reminders::{self, Delivered},
    state::AppState,
};
use serde_json::json;
use std::time::Duration;

pub fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            loop {
                match reminders::deliver_next(state.db()).await {
                    Ok(Delivered::Settled) => {}
                    Ok(Delivered::Nothing) => break,
                    Ok(Delivered::Turn {
                        chat_id,
                        workspace_id,
                        user_id,
                        reminder_id,
                        prompt,
                    }) => {
                        // Spawned rather than awaited: a generation takes as
                        // long as a model takes, and the dispatch tick is what
                        // every other due reminder is waiting on. The chat's
                        // own semaphore already serialises this turn against
                        // anything the person is typing there.
                        let turn = state.clone();
                        tokio::spawn(async move {
                            crate::ws::chat::run_turn(
                                &turn,
                                chat_id,
                                workspace_id,
                                user_id,
                                &prompt,
                                // Marked, so a reader can tell a turn the
                                // schedule opened from one the person typed.
                                Some(json!({
                                    "source": "reminder",
                                    "reminder_id": reminder_id,
                                    "actor_id": user_id,
                                })),
                            )
                            .await;
                        });
                    }
                    Err(error) => {
                        tracing::warn!(%error, "Reminder dispatch failed; retrying next interval");
                        break;
                    }
                }
            }
        }
    })
}
