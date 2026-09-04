//! Persistent reminder dispatch. Database locks coordinate all server instances.
use crate::{db::reminders, state::AppState};
use std::time::Duration;

pub fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            loop {
                match reminders::deliver_next(state.db()).await {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(error) => {
                        tracing::warn!(%error, "Reminder dispatch failed; retrying next interval");
                        break;
                    }
                }
            }
        }
    })
}
