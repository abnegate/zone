use abnegate_agent::context::ContextUsage;
use sqlx::PgPool;
use tokio::runtime::Handle;

use super::spent::Spent;

/// A turn's spend, recorded when this is dropped, so every way out of the
/// generation records it exactly once, whether the answer was stored, the
/// store failed, or no answer was kept. A turn that spent nothing records
/// nothing unless its answer was stored.
pub struct Pending {
    pool: PgPool,
    spent: Spent,
    stored: bool,
}

impl Pending {
    pub fn new(pool: PgPool, spent: Spent) -> Self {
        Self {
            pool,
            spent,
            stored: false,
        }
    }

    pub fn context(&mut self, context: ContextUsage) {
        self.spent.context = Some(context);
    }

    pub fn stored(&mut self) {
        self.stored = true;
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if self.spent.ledger.is_empty() && !self.stored {
            return;
        }
        let Ok(runtime) = Handle::try_current() else {
            tracing::warn!(
                chat = %self.spent.chat,
                message = %self.spent.message,
                "No runtime to record what this turn spent"
            );
            return;
        };
        let pool = self.pool.clone();
        let spent = self.spent.clone();
        runtime.spawn(async move { super::record(&pool, &spent).await });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::chat::tokens::Ledger;
    use abnegate_llm::{ReasoningEffort, Usage};
    use uuid::Uuid;

    async fn pool() -> PgPool {
        PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
            .await
            .expect("the test database accepts connections")
    }

    async fn workspace(pool: &PgPool) -> Uuid {
        let organization = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Pending', $1::text)")
            .bind(organization)
            .execute(pool)
            .await
            .expect("an organization");
        sqlx::query(
            "INSERT INTO workspaces (id, organization_id, name, slug) VALUES ($1, $2, 'Pending', $1::text)",
        )
        .bind(workspace)
        .bind(organization)
        .execute(pool)
        .await
        .expect("a workspace");
        workspace
    }

    fn spent(workspace: Uuid, ledger: Ledger) -> Spent {
        Spent {
            message: Uuid::new_v4(),
            chat: Uuid::new_v4(),
            workspace,
            model: "llama3".to_string(),
            effort: ReasoningEffort::Medium,
            ledger,
            context: None,
        }
    }

    fn ledger() -> Ledger {
        let mut ledger = Ledger::default();
        ledger.round("llama3");
        ledger.add("llama3", Usage::new(10, 5));
        ledger
    }

    async fn rows(pool: &PgPool, turn: Uuid) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM chat_token_usage WHERE turn_id = $1")
            .bind(turn)
            .fetch_one(pool)
            .await
            .expect("the turn's rows")
    }

    async fn settled(pool: &PgPool, turn: Uuid) -> bool {
        for _ in 0..100 {
            if rows(pool, turn).await > 0 {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        false
    }

    #[tokio::test]
    async fn a_turn_that_ends_early_is_recorded_once_when_it_is_dropped() {
        let pool = pool().await;
        let workspace = workspace(&pool).await;
        let spent = spent(workspace, ledger());
        let turn = spent.message;

        let pending = Pending::new(pool.clone(), spent);
        drop(pending);

        assert!(
            settled(&pool, turn).await,
            "a turn dropped before its answer was stored is still recorded"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(rows(&pool, turn).await, 1);
    }

    #[tokio::test]
    async fn a_turn_that_spent_nothing_and_stored_nothing_records_nothing() {
        let pool = pool().await;
        let workspace = workspace(&pool).await;
        let spent = spent(workspace, Ledger::default());
        let turn = spent.message;

        drop(Pending::new(pool.clone(), spent));

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_eq!(rows(&pool, turn).await, 0);
    }
}
