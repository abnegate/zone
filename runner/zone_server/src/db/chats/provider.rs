use std::collections::HashMap;

use sqlx::{Executor, Postgres};
use uuid::Uuid;

use crate::db::DbResult;
use crate::services::provider::Provider;
use crate::services::stages::AUTO;

/// The provider one chat runs on in place of its workspace's, and the model it
/// asks that provider for. No model leaves the choice to the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub provider: Provider,
    pub model: Option<String>,
}

/// The chat's chosen provider, or `None` when it follows its workspace or the
/// chat does not exist.
pub async fn get<'e, E>(executor: E, chat_id: Uuid) -> DbResult<Option<Choice>>
where
    E: Executor<'e, Database = Postgres>,
{
    let row: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT provider, provider_model FROM chats WHERE id = $1")
            .bind(chat_id)
            .fetch_optional(executor)
            .await?;
    Ok(row.and_then(|(provider, model)| {
        Some(Choice {
            provider: Provider::named(&provider?)?,
            model,
        })
    }))
}

/// The chosen provider of each of `ids` that chose one. A chat missing from the
/// result follows its workspace or does not exist.
pub async fn of<'e, E>(executor: E, ids: &[Uuid]) -> DbResult<HashMap<Uuid, Choice>>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT id, provider, provider_model FROM chats \
         WHERE id = ANY($1) AND provider IS NOT NULL",
    )
    .bind(ids)
    .fetch_all(executor)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, provider, model)| {
            Provider::named(&provider).map(|provider| (id, Choice { provider, model }))
        })
        .collect())
}

/// Records `choice` as the chat's provider, or with `None` returns the chat to
/// its workspace's. False when there is no such chat.
pub async fn set<'e, E>(executor: E, chat_id: Uuid, choice: Option<&Choice>) -> DbResult<bool>
where
    E: Executor<'e, Database = Postgres>,
{
    let updated = sqlx::query(
        "UPDATE chats SET provider = $2, provider_model = $3, updated_at = NOW() WHERE id = $1",
    )
    .bind(chat_id)
    .bind(choice.map(|choice| choice.provider.as_str()))
    .bind(choice.and_then(|choice| choice.model.as_deref()))
    .execute(executor)
    .await?
    .rows_affected();
    Ok(updated > 0)
}

/// The model a chat's completions ask for: the chosen model, [`AUTO`] for a
/// choice that names none, and the chat's stored model while it follows its
/// workspace.
pub fn requested(choice: Option<&Choice>, stored: &str) -> String {
    match choice {
        Some(choice) => choice.model.as_deref().unwrap_or(AUTO).to_string(),
        None => stored.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use sqlx::PgPool;

    use super::*;
    use crate::db::chats::{ChatRow, create_chat, create_chat_with_title, get_chat};
    use abnegate_llm::ReasoningEffort;

    #[test]
    fn a_chat_following_its_workspace_asks_for_its_stored_model() {
        assert_eq!(requested(None, "qwen3:8b"), "qwen3:8b");
        assert_eq!(requested(None, AUTO), AUTO);
    }

    #[test]
    fn a_choice_replaces_the_stored_model() {
        let named = Choice {
            provider: Provider::OpenAI,
            model: Some("gpt-5.5".to_string()),
        };
        assert_eq!(requested(Some(&named), "qwen3:8b"), "gpt-5.5");

        let automatic = Choice {
            provider: Provider::ClaudeCode,
            model: None,
        };
        assert_eq!(
            requested(Some(&automatic), "qwen3:8b"),
            AUTO,
            "a choice without a model leaves the provider to pick, never the stored model"
        );
    }

    async fn pool() -> PgPool {
        PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
            .await
            .expect("the test database accepts connections")
    }

    struct Fixture {
        pool: PgPool,
        organization: Uuid,
        workspace: Uuid,
    }

    impl Fixture {
        async fn new() -> Self {
            let pool = pool().await;
            let organization = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO organizations (id, name, slug) VALUES ($1, 'Providers', $1::text)",
            )
            .bind(organization)
            .execute(&pool)
            .await
            .expect("an organization");
            let workspace = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO workspaces (id, organization_id, name, slug) VALUES ($1, $2, 'Providers', $1::text)",
            )
            .bind(workspace)
            .bind(organization)
            .execute(&pool)
            .await
            .expect("a workspace");
            Self {
                pool,
                organization,
                workspace,
            }
        }

        async fn chat(&self, title: &str) -> Uuid {
            create_chat(&self.pool, Some(self.workspace), title, AUTO, false, true)
                .await
                .expect("a chat")
                .id
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
    async fn a_chat_keeps_its_choice_until_it_is_cleared() {
        let fixture = Fixture::new().await;
        let pool = &fixture.pool;
        let chat = fixture.chat("Chosen").await;
        let other = fixture.chat("Bystander").await;
        assert_eq!(get(pool, chat).await.unwrap(), None);

        let codex = Choice {
            provider: Provider::Codex,
            model: Some("gpt-6-sol".to_string()),
        };
        assert!(set(pool, chat, Some(&codex)).await.unwrap());
        assert_eq!(get(pool, chat).await.unwrap().as_ref(), Some(&codex));
        assert_eq!(
            get(pool, other).await.unwrap(),
            None,
            "a choice belongs to its own chat"
        );

        let automatic = Choice {
            provider: Provider::OpenAI,
            model: None,
        };
        assert!(set(pool, chat, Some(&automatic)).await.unwrap());
        assert_eq!(get(pool, chat).await.unwrap().as_ref(), Some(&automatic));

        assert!(set(pool, chat, None).await.unwrap());
        assert_eq!(get(pool, chat).await.unwrap(), None);
        let kept: i32 = sqlx::query_scalar(
            "SELECT num_nonnulls(provider, provider_model) FROM chats WHERE id = $1",
        )
        .bind(chat)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(kept, 0, "clearing forgets the provider and its model");

        assert!(
            !set(pool, Uuid::new_v4(), Some(&codex)).await.unwrap(),
            "no chat, nothing set"
        );
        assert_eq!(get(pool, Uuid::new_v4()).await.unwrap(), None);
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn the_choices_of_many_chats_are_read_at_once() {
        let fixture = Fixture::new().await;
        let pool = &fixture.pool;
        let chosen = fixture.chat("Chosen").await;
        let automatic = fixture.chat("Automatic").await;
        let following = fixture.chat("Following").await;
        let unlisted = fixture.chat("Unlisted").await;
        let codex = Choice {
            provider: Provider::Codex,
            model: Some("gpt-6-sol".to_string()),
        };
        let openai = Choice {
            provider: Provider::OpenAI,
            model: None,
        };
        set(pool, chosen, Some(&codex)).await.unwrap();
        set(pool, automatic, Some(&openai)).await.unwrap();
        set(pool, unlisted, Some(&codex)).await.unwrap();

        let choices = of(pool, &[chosen, automatic, following, Uuid::new_v4()])
            .await
            .unwrap();

        assert_eq!(
            choices,
            HashMap::from([(chosen, codex), (automatic, openai)]),
            "only the listed chats that chose a provider have a choice"
        );
        assert!(of(pool, &[]).await.unwrap().is_empty());
        fixture.cleanup().await;
    }

    async fn create(fixture: &Fixture, title: &str, choice: &Choice) -> DbResult<ChatRow> {
        create_chat_with_title(
            &fixture.pool,
            Some(fixture.workspace),
            title,
            AUTO,
            (false, true),
            false,
            false,
            ReasoningEffort::Auto,
            None,
            false,
            None,
            Some(choice),
        )
        .await
    }

    #[tokio::test]
    async fn a_new_chat_is_created_with_its_choice_or_not_at_all() {
        let fixture = Fixture::new().await;
        let pool = &fixture.pool;
        let codex = Choice {
            provider: Provider::Codex,
            model: Some("gpt-6-sol".to_string()),
        };
        let chat = create(&fixture, "Created chosen", &codex)
            .await
            .expect("a chat created on its choice");
        assert_eq!(get(pool, chat.id).await.unwrap().as_ref(), Some(&codex));

        let blank = Choice {
            provider: Provider::OpenAI,
            model: Some(" ".to_string()),
        };
        assert!(
            create(&fixture, "Refused choice", &blank).await.is_err(),
            "the database refuses a blank model"
        );
        let refused: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chats WHERE workspace_id = $1 AND title = $2")
                .bind(fixture.workspace)
                .bind("Refused choice")
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(refused, 0, "a choice that can't be stored leaves no chat");
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn choosing_a_provider_bumps_the_chat() {
        let fixture = Fixture::new().await;
        let pool = &fixture.pool;
        let chat = fixture.chat("Bumped").await;
        sqlx::query("UPDATE chats SET updated_at = NOW() - INTERVAL '1 day' WHERE id = $1")
            .bind(chat)
            .execute(pool)
            .await
            .unwrap();
        let before = get_chat(pool, chat).await.unwrap().unwrap().updated_at;

        let choice = Choice {
            provider: Provider::ClaudeCode,
            model: None,
        };
        set(pool, chat, Some(&choice)).await.unwrap();
        let chosen = get_chat(pool, chat).await.unwrap().unwrap().updated_at;
        assert!(chosen > before, "choosing moves updated_at forward");

        sqlx::query("UPDATE chats SET updated_at = NOW() - INTERVAL '1 day' WHERE id = $1")
            .bind(chat)
            .execute(pool)
            .await
            .unwrap();
        let before = get_chat(pool, chat).await.unwrap().unwrap().updated_at;
        set(pool, chat, None).await.unwrap();
        let cleared = get_chat(pool, chat).await.unwrap().unwrap().updated_at;
        assert!(cleared > before, "clearing moves updated_at forward");
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn the_schema_refuses_a_model_without_a_provider_or_a_blank_one() {
        let fixture = Fixture::new().await;
        let pool = &fixture.pool;
        let chat = fixture.chat("Refused").await;

        for (provider, model) in [
            (None, Some("gpt-5.5")),
            (Some("openai"), Some("")),
            (Some("openai"), Some("   ")),
            (Some("gemini"), None),
        ] {
            let refused =
                sqlx::query("UPDATE chats SET provider = $2, provider_model = $3 WHERE id = $1")
                    .bind(chat)
                    .bind(provider)
                    .bind(model)
                    .execute(pool)
                    .await
                    .expect_err("the schema refuses the pair");
            assert_eq!(
                refused
                    .as_database_error()
                    .and_then(|error| error.code())
                    .as_deref(),
                Some("23514"),
                "{provider:?} with {model:?} breaks a check constraint"
            );
        }
        assert_eq!(get(pool, chat).await.unwrap(), None);
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn the_schema_keeps_the_wider_reasoning_efforts() {
        let fixture = Fixture::new().await;
        let pool = &fixture.pool;
        let chat = fixture.chat("Efforts").await;

        for effort in ["xhigh", "max", "ultra"] {
            sqlx::query("UPDATE chats SET reasoning_effort = $2 WHERE id = $1")
                .bind(chat)
                .bind(effort)
                .execute(pool)
                .await
                .unwrap_or_else(|error| panic!("{effort} is a reasoning effort: {error}"));
            let stored: String =
                sqlx::query_scalar("SELECT reasoning_effort FROM chats WHERE id = $1")
                    .bind(chat)
                    .fetch_one(pool)
                    .await
                    .unwrap();
            assert_eq!(stored, effort);
        }

        let refused = sqlx::query("UPDATE chats SET reasoning_effort = 'bogus' WHERE id = $1")
            .bind(chat)
            .execute(pool)
            .await
            .expect_err("bogus is no reasoning effort");
        assert_eq!(
            refused
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("23514")
        );
        fixture.cleanup().await;
    }
}
