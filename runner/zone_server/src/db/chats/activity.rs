//! What a workspace's chats are doing, read for the whole list at once.
//!
//! Two readings the chat list needs and a per-chat query would make N of: the
//! chats whose turn is still running, and the chats whose last word was a
//! question nobody has answered.

use sqlx::{Executor, Postgres, Row};
use std::collections::HashSet;
use uuid::Uuid;

use crate::db::DbResult;

/// The workspace's chats holding a live generation lease.
///
/// A lease is taken for the length of one turn and renewed while it runs, so an
/// unexpired lease is a turn still being written. A turn whose process died
/// leaves the row behind with a lease already expired, and that is not writing:
/// the clock comparison is the database's own, never the server's.
pub async fn writing<'e, E>(executor: E, workspace_id: Uuid) -> DbResult<HashSet<Uuid>>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows = sqlx::query(
        "SELECT l.chat_id FROM chat_leases l
         JOIN chats c ON c.id = l.chat_id
         WHERE c.workspace_id = $1 AND l.expires_at > clock_timestamp()",
    )
    .bind(workspace_id)
    .fetch_all(executor)
    .await?;
    Ok(rows.into_iter().map(|row| row.get("chat_id")).collect())
}

/// The workspace's chats whose latest message is an agent question with no
/// answer after it.
///
/// An answer travels as an ordinary user message rather than a frame of its
/// own, so a user message after the question settles it; that is the same rule
/// the console draws the card by. Which means the question is outstanding
/// exactly when the newest message in the chat is the assistant one carrying
/// it.
pub async fn awaiting_answer<'e, E>(executor: E, workspace_id: Uuid) -> DbResult<HashSet<Uuid>>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows = sqlx::query(
        "SELECT last.chat_id FROM (
             SELECT DISTINCT ON (m.chat_id) m.chat_id, m.role, m.metadata
             FROM messages m
             JOIN chats c ON c.id = m.chat_id
             WHERE c.workspace_id = $1
             ORDER BY m.chat_id, m.created_at DESC, m.id DESC
         ) last
         WHERE last.role = 'assistant' AND EXISTS (
             SELECT 1
             FROM jsonb_array_elements(
                 CASE WHEN jsonb_typeof(last.metadata -> 'tool_calls') = 'array'
                      THEN last.metadata -> 'tool_calls'
                      ELSE '[]'::jsonb END
             ) AS call
             WHERE jsonb_typeof(call -> 'questions') = 'array'
               AND jsonb_array_length(call -> 'questions') > 0
         )",
    )
    .bind(workspace_id)
    .fetch_all(executor)
    .await?;
    Ok(rows.into_iter().map(|row| row.get("chat_id")).collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use sqlx::PgPool;
    use std::time::Duration;
    use uuid::Uuid;

    use super::*;
    use crate::db::chats::{create_chat, create_message};
    use crate::db::context::Store;
    use crate::db::{organizations, workspaces};

    async fn pool() -> PgPool {
        PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
            .await
            .expect("the test database accepts connections")
    }

    async fn workspace(pool: &PgPool) -> Uuid {
        let organization = organizations::create_organization(
            pool,
            &format!("Indicators {}", Uuid::new_v4()),
            &format!("indicators-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("an organization")
        .id;
        workspaces::create_workspace(
            pool,
            organization,
            &format!("Chats {}", Uuid::new_v4()),
            &format!("chats-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("a workspace")
        .id
    }

    async fn chat(pool: &PgPool, workspace_id: Uuid) -> Uuid {
        create_chat(pool, Some(workspace_id), "Changes", "qwen3:8b", false, true)
            .await
            .expect("a chat")
            .id
    }

    fn asked() -> serde_json::Value {
        json!({
            "tool_calls": [{
                "id": "call_1",
                "name": "ask_question",
                "arguments": "{}",
                "success": true,
                "detail": "Waiting for your answer…",
                "duration_ms": 1,
                "questions": [{
                    "header": "Scope",
                    "question": "How far?",
                    "choices": [],
                    "multi_select": false,
                    "required": true
                }]
            }]
        })
    }

    #[tokio::test]
    async fn only_an_unexpired_lease_reads_as_writing() {
        let pool = pool().await;
        let workspace_id = workspace(&pool).await;
        let live = chat(&pool, workspace_id).await;
        let lapsed = chat(&pool, workspace_id).await;

        let held = Store::new(pool.clone(), live, Some(workspace_id));
        let _lease = held
            .acquire(Uuid::new_v4(), Duration::from_secs(30))
            .await
            .expect("a lease");
        // Releasing a lease expires it where it stands, which is the row a
        // turn that has already ended leaves behind.
        let dropped = Store::new(pool.clone(), lapsed, Some(workspace_id));
        let expired = dropped
            .acquire(Uuid::new_v4(), Duration::from_secs(30))
            .await
            .expect("a lease");
        dropped.release(&expired).await.expect("releasing it");

        let writing = writing(&pool, workspace_id).await.expect("the reading");
        assert!(writing.contains(&live));
        assert!(!writing.contains(&lapsed));
    }

    #[tokio::test]
    async fn a_question_answered_by_the_next_message_is_no_longer_waiting() {
        let pool = pool().await;
        let workspace_id = workspace(&pool).await;
        let outstanding = chat(&pool, workspace_id).await;
        let answered = chat(&pool, workspace_id).await;
        let quiet = chat(&pool, workspace_id).await;

        for chat_id in [outstanding, answered] {
            create_message(&pool, chat_id, "assistant", "Which way?", Some(asked()))
                .await
                .expect("the question");
        }
        create_message(&pool, answered, "user", "Scope: Backfill", None)
            .await
            .expect("the answer");
        create_message(&pool, quiet, "assistant", "Done", None)
            .await
            .expect("an ordinary reply");

        let waiting = awaiting_answer(&pool, workspace_id)
            .await
            .expect("the reading");
        assert!(waiting.contains(&outstanding));
        assert!(!waiting.contains(&answered));
        assert!(!waiting.contains(&quiet));
    }
}
