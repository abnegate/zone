mod ledger;
mod pending;
mod share;
mod spent;

pub use ledger::Ledger;
pub use pending::Pending;
pub use share::Share;
pub use spent::Spent;

use sqlx::PgPool;

use crate::db::chat_token_usage;
use crate::metrics;

/// A failed write is logged rather than raised, because the answer it describes
/// is already delivered.
pub async fn record(pool: &PgPool, spent: &Spent) {
    metrics::record_chat_turn(
        spent.ledger.rounds() as u64,
        spent.ledger.tool_calls() as u64,
    );
    for share in spent.ledger.shares() {
        if let Some(reported) = share.reported {
            metrics::record_chat_tokens(&share.model, spent::counted(reported));
        }
    }
    metrics::record_chat_context(&spent.categories());
    if let Err(error) = chat_token_usage::record(pool, &spent.turn()).await {
        tracing::warn!(
            chat = %spent.chat,
            message = %spent.message,
            %error,
            "Could not store what this turn spent; the answer itself is stored"
        );
    }
}
