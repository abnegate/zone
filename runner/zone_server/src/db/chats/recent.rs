//! The settings the chat a workspace used last runs with, which a new chat in
//! that workspace starts from.

use abnegate_llm::ReasoningEffort;
use serde::Serialize;
use sqlx::{Executor, FromRow, Postgres};
use uuid::Uuid;

use super::effort;
use crate::db::DbResult;
use crate::services::provider::Provider;

/// What the workspace's most recently used assistant chat runs with. A
/// provider is `None` while that chat follows its workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recent {
    pub model_name: String,
    pub provider: Option<Provider>,
    pub provider_model: Option<String>,
    pub reasoning_effort: ReasoningEffort,
    pub agent_enabled: bool,
    pub auto_approve: bool,
    pub offline: bool,
    pub context_tokens: Option<i64>,
}

#[derive(FromRow)]
struct Row {
    model_name: String,
    provider: Option<String>,
    provider_model: Option<String>,
    reasoning_effort: String,
    agent_enabled: bool,
    auto_approve: bool,
    offline: bool,
    context_tokens: Option<i64>,
}

impl From<Row> for Recent {
    fn from(row: Row) -> Self {
        let provider = row.provider.as_deref().and_then(Provider::named);
        Self {
            model_name: row.model_name,
            provider_model: provider.and(row.provider_model),
            provider,
            reasoning_effort: effort(&row.reasoning_effort),
            agent_enabled: row.agent_enabled,
            auto_approve: row.auto_approve,
            offline: row.offline,
            context_tokens: row.context_tokens,
        }
    }
}

/// The settings of the unarchived assistant chat in `workspace` touched last,
/// or `None` when it has none. Planner and update chats are a project's, not
/// the member's, so they never count.
pub async fn get<'e, E>(executor: E, workspace: Uuid) -> DbResult<Option<Recent>>
where
    E: Executor<'e, Database = Postgres>,
{
    let row: Option<Row> = sqlx::query_as(
        "SELECT model_name, provider, provider_model, reasoning_effort, agent_enabled, \
         auto_approve, offline, context_tokens \
         FROM chats \
         WHERE workspace_id = $1 AND purpose = 'assistant' AND archived IS NOT TRUE \
         ORDER BY updated_at DESC NULLS LAST, created_at DESC NULLS LAST \
         LIMIT 1",
    )
    .bind(workspace)
    .fetch_optional(executor)
    .await?;
    Ok(row.map(Recent::from))
}

#[cfg(test)]
mod tests {
    use sqlx::PgPool;

    use super::*;
    use crate::db::chats::create_chat_with_title;
    use crate::db::chats::provider::Choice;
    use crate::services::stages::AUTO;

    struct Fixture {
        pool: PgPool,
        organization: Uuid,
        workspace: Uuid,
    }

    impl Fixture {
        async fn new() -> Self {
            let pool = PgPool::connect(
                &std::env::var("TEST_DATABASE_URL").expect("isolated test database"),
            )
            .await
            .expect("the test database accepts connections");
            let organization = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO organizations (id, name, slug) VALUES ($1, 'Recent', $1::text)",
            )
            .bind(organization)
            .execute(&pool)
            .await
            .expect("an organization");
            let workspace = Self::workspace(&pool, organization).await;
            Self {
                pool,
                organization,
                workspace,
            }
        }

        async fn workspace(pool: &PgPool, organization: Uuid) -> Uuid {
            let workspace = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO workspaces (id, organization_id, name, slug) VALUES ($1, $2, 'Recent', $1::text)",
            )
            .bind(workspace)
            .bind(organization)
            .execute(pool)
            .await
            .expect("a workspace");
            workspace
        }

        async fn chat(&self, workspace: Uuid, model: &str, choice: Option<&Choice>) -> Uuid {
            let chat = create_chat_with_title(
                &self.pool,
                Some(workspace),
                "Recent",
                model,
                (true, true),
                false,
                true,
                ReasoningEffort::High,
                None,
                true,
                Some(32_768),
                choice,
            )
            .await
            .expect("a chat")
            .id;
            self.age(chat, 1).await;
            chat
        }

        async fn age(&self, chat: Uuid, days: i32) {
            sqlx::query(
                "UPDATE chats SET updated_at = NOW() - make_interval(days => $2) WHERE id = $1",
            )
            .bind(chat)
            .bind(days)
            .execute(&self.pool)
            .await
            .expect("the chat ages");
        }

        async fn touch(&self, chat: Uuid) {
            sqlx::query("UPDATE chats SET updated_at = NOW() WHERE id = $1")
                .bind(chat)
                .execute(&self.pool)
                .await
                .expect("the chat is touched");
        }

        async fn cleanup(self) {
            sqlx::query("DELETE FROM organizations WHERE id = $1")
                .bind(self.organization)
                .execute(&self.pool)
                .await
                .expect("the organization can be deleted");
        }
    }

    #[tokio::test]
    async fn a_workspace_without_chats_has_nothing_recent() {
        let fixture = Fixture::new().await;
        assert_eq!(get(&fixture.pool, fixture.workspace).await.unwrap(), None);
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn the_chat_touched_last_is_the_recent_one() {
        let fixture = Fixture::new().await;
        let codex = Choice {
            provider: Provider::Codex,
            model: Some("gpt-6-sol".to_string()),
        };
        fixture.chat(fixture.workspace, AUTO, Some(&codex)).await;
        let following = fixture.chat(fixture.workspace, "qwen3:8b", None).await;
        fixture.age(following, 2).await;

        let recent = get(&fixture.pool, fixture.workspace)
            .await
            .unwrap()
            .expect("a recent chat");
        assert_eq!(
            recent,
            Recent {
                model_name: AUTO.to_string(),
                provider: Some(Provider::Codex),
                provider_model: Some("gpt-6-sol".to_string()),
                reasoning_effort: ReasoningEffort::High,
                agent_enabled: true,
                auto_approve: true,
                offline: true,
                context_tokens: Some(32_768),
            }
        );

        fixture.touch(following).await;
        let recent = get(&fixture.pool, fixture.workspace)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (recent.model_name.as_str(), recent.provider),
            ("qwen3:8b", None),
            "using an older chat makes it the one a new chat starts from"
        );
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn archived_project_and_other_workspace_chats_never_count() {
        let fixture = Fixture::new().await;
        let kept = fixture.chat(fixture.workspace, "kept", None).await;
        fixture.age(kept, 5).await;

        let archived = fixture.chat(fixture.workspace, "archived", None).await;
        sqlx::query("UPDATE chats SET archived = TRUE, updated_at = NOW() WHERE id = $1")
            .bind(archived)
            .execute(&fixture.pool)
            .await
            .unwrap();
        let planner = fixture.chat(fixture.workspace, "planner", None).await;
        sqlx::query(
            "UPDATE chats SET purpose = 'project_planner', updated_at = NOW() WHERE id = $1",
        )
        .bind(planner)
        .execute(&fixture.pool)
        .await
        .unwrap();
        let elsewhere = Fixture::workspace(&fixture.pool, fixture.organization).await;
        let other = fixture.chat(elsewhere, "elsewhere", None).await;
        fixture.touch(other).await;

        let recent = get(&fixture.pool, fixture.workspace)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(recent.model_name, "kept");
        let recent = get(&fixture.pool, elsewhere).await.unwrap().unwrap();
        assert_eq!(
            recent.model_name, "elsewhere",
            "each workspace keeps its own"
        );
        fixture.cleanup().await;
    }
}
