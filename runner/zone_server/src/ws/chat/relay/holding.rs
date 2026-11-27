use sqlx::{PgPool, Row};
use uuid::Uuid;

/// A chat's lease as the database has it now, whoever holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Holding {
    pub fence: i64,
    pub live: bool,
}

impl Holding {
    /// None for a chat that never ran a turn.
    pub async fn read(pool: &PgPool, chat: Uuid) -> Result<Option<Self>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT fence, expires_at > clock_timestamp() AS live FROM chat_leases WHERE chat_id = $1",
        )
        .bind(chat)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(|row| Self {
            fence: row.get("fence"),
            live: row.get("live"),
        }))
    }

    /// The live lease, if any. A read that fails counts as none, so the
    /// caller falls back on what a single server does.
    pub async fn live(pool: &PgPool, chat: Uuid) -> Option<Self> {
        match Self::read(pool, chat).await {
            Ok(holding) => holding.filter(|holding| holding.live),
            Err(error) => {
                tracing::warn!(%chat, %error, "Could not read the chat's lease");
                None
            }
        }
    }
}
