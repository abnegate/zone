//! The pull requests each chat opened.

use chrono::{DateTime, Utc};
use sqlx::{Executor, Postgres};
use uuid::Uuid;

use super::DbResult;

pub const GITHUB: &str = "github";

macro_rules! columns {
    () => {
        "id, chat_id, provider, owner, repo, number, url, title, source_id, dismissed_at, created_at"
    };
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PullRequestRow {
    pub id: Uuid,
    pub chat_id: Uuid,
    pub provider: String,
    pub owner: String,
    pub repo: String,
    pub number: i64,
    pub url: String,
    pub title: Option<String>,
    pub source_id: Option<Uuid>,
    pub dismissed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPullRequest<'a> {
    pub chat_id: Uuid,
    pub owner: &'a str,
    pub repo: &'a str,
    pub number: i64,
    pub url: &'a str,
    pub title: Option<&'a str>,
    pub source_id: Option<Uuid>,
}

/// Records that `opened.chat_id` opened this pull request. Recording one the
/// chat already owns keeps the row, filling in what the new sighting knows and
/// the old one did not.
pub async fn record<'e, E>(executor: E, opened: &NewPullRequest<'_>) -> DbResult<PullRequestRow>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "INSERT INTO chat_pull_requests (chat_id, provider, owner, repo, number, url, title, source_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (chat_id, provider, lower(owner), lower(repo), number) DO UPDATE SET
             title = COALESCE(EXCLUDED.title, chat_pull_requests.title),
             source_id = COALESCE(EXCLUDED.source_id, chat_pull_requests.source_id)
         RETURNING ",
        columns!()
    ))
    .bind(opened.chat_id)
    .bind(GITHUB)
    .bind(opened.owner)
    .bind(opened.repo)
    .bind(opened.number)
    .bind(opened.url)
    .bind(opened.title)
    .bind(opened.source_id)
    .fetch_one(executor)
    .await
}

/// The pull requests `chat_id` opened and has not dismissed, oldest first.
pub async fn list<'e, E>(executor: E, chat_id: Uuid) -> DbResult<Vec<PullRequestRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM chat_pull_requests
         WHERE chat_id = $1 AND dismissed_at IS NULL
         ORDER BY created_at, id"
    ))
    .bind(chat_id)
    .fetch_all(executor)
    .await
}

/// One pull request `chat_id` opened, dismissed or not.
pub async fn get<'e, E>(executor: E, chat_id: Uuid, id: Uuid) -> DbResult<Option<PullRequestRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM chat_pull_requests WHERE chat_id = $1 AND id = $2"
    ))
    .bind(chat_id)
    .bind(id)
    .fetch_optional(executor)
    .await
}

/// Hides the pull request from its chat, or with `false` shows it again.
pub async fn dismiss<'e, E>(
    executor: E,
    chat_id: Uuid,
    id: Uuid,
    dismissed: bool,
) -> DbResult<Option<PullRequestRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "UPDATE chat_pull_requests
         SET dismissed_at = CASE WHEN $3 THEN COALESCE(dismissed_at, NOW()) END
         WHERE chat_id = $1 AND id = $2
         RETURNING ",
        columns!()
    ))
    .bind(chat_id)
    .bind(id)
    .bind(dismissed)
    .fetch_optional(executor)
    .await
}

pub async fn retitle<'e, E>(executor: E, id: Uuid, title: &str) -> DbResult<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        "UPDATE chat_pull_requests SET title = $2 WHERE id = $1 AND title IS DISTINCT FROM $2",
    )
    .bind(id)
    .bind(title)
    .execute(executor)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::PgPool;

    use super::*;
    use crate::db::chats::create_chat;

    async fn pool() -> PgPool {
        PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
            .await
            .expect("the test database accepts connections")
    }

    async fn chat(pool: &PgPool) -> Uuid {
        create_chat(pool, None, "Changes", "qwen3:8b", false, true)
            .await
            .expect("a chat")
            .id
    }

    fn opened(
        chat_id: Uuid,
        owner: &'static str,
        title: Option<&'static str>,
    ) -> NewPullRequest<'static> {
        NewPullRequest {
            chat_id,
            owner,
            repo: "widgets",
            number: 7,
            url: "https://github.com/acme/widgets/pull/7",
            title,
            source_id: None,
        }
    }

    #[tokio::test]
    async fn recording_a_pull_request_twice_keeps_one_row_and_what_either_knew() {
        let pool = pool().await;
        let chat_id = chat(&pool).await;

        let first = record(&pool, &opened(chat_id, "acme", Some("Add widgets")))
            .await
            .expect("the first sighting");
        let second = record(&pool, &opened(chat_id, "ACME", None))
            .await
            .expect("the second sighting");

        assert_eq!(first.id, second.id);
        assert_eq!(second.title.as_deref(), Some("Add widgets"));
        assert_eq!(list(&pool, chat_id).await.expect("the list"), [second]);
    }

    #[tokio::test]
    async fn a_dismissed_pull_request_leaves_the_list_until_it_is_restored() {
        let pool = pool().await;
        let chat_id = chat(&pool).await;
        let row = record(&pool, &opened(chat_id, "acme", None))
            .await
            .expect("a pull request");

        let dismissed = dismiss(&pool, chat_id, row.id, true)
            .await
            .expect("dismissing")
            .expect("the row");
        assert!(dismissed.dismissed_at.is_some());
        assert!(list(&pool, chat_id).await.expect("the list").is_empty());

        let restored = dismiss(&pool, chat_id, row.id, false)
            .await
            .expect("restoring")
            .expect("the row");
        assert_eq!(restored.dismissed_at, None);
        assert_eq!(list(&pool, chat_id).await.expect("the list"), [restored]);
    }

    #[tokio::test]
    async fn another_chats_pull_request_is_out_of_reach() {
        let pool = pool().await;
        let owner = chat(&pool).await;
        let other = chat(&pool).await;
        let row = record(&pool, &opened(owner, "acme", None))
            .await
            .expect("a pull request");

        assert_eq!(get(&pool, other, row.id).await.expect("the lookup"), None);
        assert_eq!(
            dismiss(&pool, other, row.id, true)
                .await
                .expect("the update"),
            None
        );
    }
}
