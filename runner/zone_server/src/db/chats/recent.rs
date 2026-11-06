use abnegate_llm::ReasoningEffort;
use serde::Serialize;
use sqlx::{Executor, FromRow, Postgres};
use uuid::Uuid;

use super::effort;
use crate::db::DbResult;
use crate::services::provider::Provider;

/// What a new chat in a workspace starts from. A `None` provider follows the
/// workspace's. Auto-approve is never carried: chats have no owner, so the
/// chat used last may be another member's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recent {
    pub model_name: String,
    pub provider: Option<Provider>,
    pub provider_model: Option<String>,
    pub reasoning_effort: ReasoningEffort,
    pub agent_enabled: bool,
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
            offline: row.offline,
            context_tokens: row.context_tokens,
        }
    }
}

/// The settings of the unarchived assistant chat in `workspace` a member used
/// last: created, reconfigured or typed in. Planner and update chats belong to
/// a project, so they never count.
pub async fn get<'e, E>(executor: E, workspace: Uuid) -> DbResult<Option<Recent>>
where
    E: Executor<'e, Database = Postgres>,
{
    let row: Option<Row> = sqlx::query_as(
        "SELECT model_name, provider, provider_model, reasoning_effort, agent_enabled, \
         offline, context_tokens \
         FROM chats \
         WHERE workspace_id = $1 AND purpose = 'assistant' AND archived IS NOT TRUE \
         ORDER BY used_at DESC \
         LIMIT 1",
    )
    .bind(workspace)
    .fetch_optional(executor)
    .await?;
    Ok(row.map(Recent::from))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use abnegate_llm::Message;
    use serde_json::{Value, json};
    use sqlx::PgPool;
    use zone_chat::history::ReplayMessage;

    use super::*;
    use crate::db::chats::provider::{self, Choice};
    use crate::db::chats::{create_chat_with_title, create_message, update_chat};
    use crate::db::context::Store;
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
                "UPDATE chats SET used_at = NOW() - make_interval(days => $2), \
                 updated_at = NOW() - make_interval(days => $2) WHERE id = $1",
            )
            .bind(chat)
            .bind(days)
            .execute(&self.pool)
            .await
            .expect("the chat ages");
        }

        fn store(&self, chat: Uuid) -> Store {
            Store::new(self.pool.clone(), chat, Some(self.workspace))
        }

        async fn turn(&self, chat: Uuid, metadata: Option<Value>) {
            let store = self.store(chat);
            let lease = store
                .acquire(Uuid::new_v4(), Duration::from_secs(60))
                .await
                .expect("a lease");
            store
                .begin(
                    &lease,
                    Uuid::new_v4(),
                    Uuid::new_v4(),
                    "Hello",
                    metadata,
                    ReplayMessage::from(&Message::user("Hello")),
                )
                .await
                .expect("a turn");
            store.release(&lease).await.expect("the lease is released");
        }

        async fn post(&self, chat: Uuid, role: &str, metadata: Option<Value>) {
            let store = self.store(chat);
            let lease = store
                .acquire(Uuid::new_v4(), Duration::from_secs(60))
                .await
                .expect("a lease");
            store
                .create_message(&lease, role, "Hello", metadata)
                .await
                .expect("a message");
            store.release(&lease).await.expect("the lease is released");
        }

        async fn recent(&self) -> Recent {
            get(&self.pool, self.workspace)
                .await
                .unwrap()
                .expect("a recent chat")
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
    async fn the_chat_created_last_is_the_recent_one() {
        let fixture = Fixture::new().await;
        let codex = Choice {
            provider: Provider::Codex,
            model: Some("gpt-6-sol".to_string()),
        };
        fixture.chat(fixture.workspace, AUTO, Some(&codex)).await;
        let following = fixture.chat(fixture.workspace, "qwen3:8b", None).await;
        fixture.age(following, 2).await;

        assert_eq!(
            fixture.recent().await,
            Recent {
                model_name: AUTO.to_string(),
                provider: Some(Provider::Codex),
                provider_model: Some("gpt-6-sol".to_string()),
                reasoning_effort: ReasoningEffort::High,
                agent_enabled: true,
                offline: true,
                context_tokens: Some(32_768),
            },
            "every setting but auto-approve is carried"
        );
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn typing_in_or_reconfiguring_an_older_chat_makes_it_the_recent_one() {
        let fixture = Fixture::new().await;
        let older = fixture.chat(fixture.workspace, "older", None).await;
        fixture.age(older, 3).await;
        let newer = fixture.chat(fixture.workspace, "newer", None).await;

        fixture.turn(older, None).await;
        assert_eq!(
            fixture.recent().await.model_name,
            "older",
            "a typed turn counts"
        );

        fixture.age(older, 3).await;
        fixture.post(older, "user", None).await;
        assert_eq!(
            fixture.recent().await.model_name,
            "older",
            "a posted message counts"
        );

        fixture.age(older, 3).await;
        fixture.age(newer, 2).await;
        update_chat(
            &fixture.pool,
            newer,
            None,
            None,
            None,
            None,
            Some(ReasoningEffort::Low),
            None,
            None,
        )
        .await
        .unwrap();
        let recent = fixture.recent().await;
        assert_eq!(
            (recent.model_name.as_str(), recent.reasoning_effort),
            ("newer", ReasoningEffort::Low),
            "changing a setting counts"
        );

        let openai = Choice {
            provider: Provider::OpenAI,
            model: Some("gpt-4.1".to_string()),
        };
        provider::set(&fixture.pool, older, Some(&openai))
            .await
            .unwrap();
        let recent = fixture.recent().await;
        assert_eq!(
            (recent.provider, recent.provider_model.as_deref()),
            (Some(Provider::OpenAI), Some("gpt-4.1")),
            "choosing a provider counts"
        );
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn automations_renames_and_uncarried_settings_never_make_a_chat_the_recent_one() {
        let fixture = Fixture::new().await;
        let older = fixture.chat(fixture.workspace, "older", None).await;
        fixture.age(older, 3).await;
        fixture.chat(fixture.workspace, "newer", None).await;

        create_message(&fixture.pool, older, "assistant", "Reply", None)
            .await
            .unwrap();
        create_message(
            &fixture.pool,
            older,
            "user",
            "Scheduled",
            Some(json!({ "source": "reminder" })),
        )
        .await
        .unwrap();
        fixture
            .turn(older, Some(json!({ "source": "reminder" })))
            .await;
        fixture.post(older, "assistant", None).await;
        fixture
            .post(older, "user", Some(json!({ "source": "auto_project" })))
            .await;
        update_chat(
            &fixture.pool,
            older,
            Some("Renamed"),
            None,
            Some(true),
            Some(true),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE chats SET updated_at = NOW() WHERE id = $1")
            .bind(older)
            .execute(&fixture.pool)
            .await
            .unwrap();

        assert_eq!(fixture.recent().await.model_name, "newer");
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn archived_project_and_other_workspace_chats_never_count() {
        let fixture = Fixture::new().await;
        let kept = fixture.chat(fixture.workspace, "kept", None).await;
        fixture.age(kept, 5).await;

        let archived = fixture.chat(fixture.workspace, "archived", None).await;
        sqlx::query("UPDATE chats SET archived = TRUE WHERE id = $1")
            .bind(archived)
            .execute(&fixture.pool)
            .await
            .unwrap();
        let planner = fixture.chat(fixture.workspace, "planner", None).await;
        sqlx::query("UPDATE chats SET purpose = 'project_planner' WHERE id = $1")
            .bind(planner)
            .execute(&fixture.pool)
            .await
            .unwrap();
        let elsewhere = Fixture::workspace(&fixture.pool, fixture.organization).await;
        fixture.chat(elsewhere, "elsewhere", None).await;

        assert_eq!(fixture.recent().await.model_name, "kept");
        let recent = get(&fixture.pool, elsewhere).await.unwrap().unwrap();
        assert_eq!(
            recent.model_name, "elsewhere",
            "each workspace keeps its own"
        );
        fixture.cleanup().await;
    }
}
