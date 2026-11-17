mod ledger;
mod share;
mod spent;

pub use ledger::Ledger;
pub use share::Share;
pub use spent::Spent;

use sqlx::PgPool;

use crate::db::chat_token_usage;
use crate::metrics;

/// Record `spent` off the turn's path, so the end of a turn never waits on it.
pub fn spawn(pool: PgPool, spent: Spent) {
    tokio::spawn(async move { record(&pool, &spent).await });
}

/// Count `spent` into Prometheus and store it. A failed write is logged, never
/// raised: the answer it describes is already delivered.
pub async fn record(pool: &PgPool, spent: &Spent) {
    metrics::record_chat_turn(
        spent.billed.rounds() as u64,
        spent.billed.tool_calls() as u64,
    );
    for share in spent.billed.shares() {
        if let Some(billed) = share.billed {
            metrics::record_chat_tokens(&share.model, spent::billed(billed));
        }
    }
    metrics::record_chat_context(&spent.categories());
    let turn = spent.turn();
    let mut stored = chat_token_usage::record(pool, &turn).await;
    // The message or chat was deleted while the row was being written; the
    // second write finds it gone and stores the row without it.
    if stored.as_ref().is_err_and(vanished) {
        stored = chat_token_usage::record(pool, &turn).await;
    }
    if let Err(error) = stored {
        tracing::warn!(
            chat = %spent.chat,
            message = %spent.message,
            %error,
            "Could not store what this turn spent; the answer itself is stored"
        );
    }
}

fn vanished(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database) if database.is_foreign_key_violation())
}
