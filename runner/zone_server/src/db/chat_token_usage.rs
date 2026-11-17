use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reported {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    /// Can exceed the other two together once cache writes and reads are counted.
    pub total_tokens: i64,
}

/// Where a turn's final context stood, by category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Breakdown {
    pub total: i64,
    pub instructions: i64,
    pub conversation: i64,
    pub tools: i64,
    pub results: i64,
    pub summary: i64,
    /// Absent rather than zero when the turn carried no attachments, so an
    /// average over attachment cost is not diluted by turns that had none.
    pub attachments: Option<i64>,
    pub overhead: i64,
    /// Whether the breakdown is an estimate rather than a tokenizer's count.
    pub estimated: bool,
}

/// What one model spent of a turn. A turn handed over to another model has
/// one share per model, and only the share of the model it ended on carries
/// the closing context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    pub model: String,
    pub reported: Option<Reported>,
    pub rounds: i32,
    pub tool_calls: i32,
    pub context: Option<Breakdown>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// The assistant message the turn produced. It names the turn for good,
    /// while each row's link to the message is cleared if the message is deleted.
    pub message_id: Uuid,
    pub chat_id: Uuid,
    pub workspace_id: Uuid,
    pub reasoning_effort: String,
    pub shares: Vec<Share>,
}

/// Store every share of `turn` together. A message or chat deleted before the
/// write lands is stored as gone rather than refused, since its tokens were
/// still billed.
pub async fn record(pool: &PgPool, turn: &Turn) -> DbResult<()> {
    let mut transaction = pool.begin().await?;
    for share in &turn.shares {
        let context = share.context;
        sqlx::query(
            "INSERT INTO chat_token_usage ( \
                 turn_id, message_id, chat_id, workspace_id, model, reasoning_effort, \
                 prompt_tokens, completion_tokens, total_tokens, rounds, tool_calls, \
                 context_tokens, instructions_tokens, conversation_tokens, tools_tokens, \
                 results_tokens, summary_tokens, attachments_tokens, overhead_tokens, estimated \
             ) VALUES ( \
                 $1, (SELECT id FROM messages WHERE id = $1), (SELECT id FROM chats WHERE id = $2), \
                 $3, $4, $5, $6, $7, $8, $9, $10, \
                 $11, $12, $13, $14, $15, $16, $17, $18, $19 \
             )",
        )
        .bind(turn.message_id)
        .bind(turn.chat_id)
        .bind(turn.workspace_id)
        .bind(&share.model)
        .bind(&turn.reasoning_effort)
        .bind(share.reported.map(|reported| reported.prompt_tokens))
        .bind(share.reported.map(|reported| reported.completion_tokens))
        .bind(share.reported.map(|reported| reported.total_tokens))
        .bind(share.rounds)
        .bind(share.tool_calls)
        .bind(context.map(|context| context.total))
        .bind(context.map(|context| context.instructions))
        .bind(context.map(|context| context.conversation))
        .bind(context.map(|context| context.tools))
        .bind(context.map(|context| context.results))
        .bind(context.map(|context| context.summary))
        .bind(context.and_then(|context| context.attachments))
        .bind(context.map(|context| context.overhead))
        .bind(context.map(|context| context.estimated))
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await
}

/// One model's share of one turn as stored.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct TurnRow {
    pub turn_id: Uuid,
    pub message_id: Option<Uuid>,
    pub model: String,
    pub reasoning_effort: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub rounds: i32,
    pub tool_calls: i32,
    pub context_tokens: Option<i64>,
    pub instructions_tokens: Option<i64>,
    pub conversation_tokens: Option<i64>,
    pub tools_tokens: Option<i64>,
    pub results_tokens: Option<i64>,
    pub summary_tokens: Option<i64>,
    pub attachments_tokens: Option<i64>,
    pub overhead_tokens: Option<i64>,
    pub estimated: Option<bool>,
    pub recorded_at: DateTime<Utc>,
}

/// sqlx takes only a `&'static str`, so the shared column list is spliced in
/// with `concat!` on literals.
macro_rules! turn_query {
    ($tail:literal) => {
        concat!(
            "SELECT turn_id, message_id, model, reasoning_effort, prompt_tokens, \
             completion_tokens, total_tokens, rounds, tool_calls, context_tokens, \
             instructions_tokens, conversation_tokens, tools_tokens, results_tokens, \
             summary_tokens, attachments_tokens, overhead_tokens, estimated, recorded_at ",
            $tail
        )
    };
}

/// `reported_turns` counts only the turns a provider gave a figure for, which
/// is the denominator a per-turn average belongs over.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Totals {
    pub turns: i64,
    pub reported_turns: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub rounds: i64,
    pub tool_calls: i64,
    /// The largest context any one turn ended on, not a sum.
    pub peak_context_tokens: i64,
    pub instructions_tokens: i64,
    pub conversation_tokens: i64,
    pub tools_tokens: i64,
    pub results_tokens: i64,
    pub summary_tokens: i64,
    pub attachments_tokens: i64,
    pub overhead_tokens: i64,
    pub first_at: Option<DateTime<Utc>>,
    pub last_at: Option<DateTime<Utc>>,
}

macro_rules! totals_query {
    ($grouped:literal, $tail:literal) => {
        concat!(
            "SELECT ",
            $grouped,
            "count(DISTINCT u.turn_id)::bigint AS turns, \
             (count(DISTINCT u.turn_id) FILTER (WHERE u.total_tokens IS NOT NULL))::bigint AS reported_turns, \
             COALESCE(sum(u.prompt_tokens), 0)::bigint AS prompt_tokens, \
             COALESCE(sum(u.completion_tokens), 0)::bigint AS completion_tokens, \
             COALESCE(sum(u.total_tokens), 0)::bigint AS total_tokens, \
             COALESCE(sum(u.rounds), 0)::bigint AS rounds, \
             COALESCE(sum(u.tool_calls), 0)::bigint AS tool_calls, \
             COALESCE(max(u.context_tokens), 0)::bigint AS peak_context_tokens, \
             COALESCE(sum(u.instructions_tokens), 0)::bigint AS instructions_tokens, \
             COALESCE(sum(u.conversation_tokens), 0)::bigint AS conversation_tokens, \
             COALESCE(sum(u.tools_tokens), 0)::bigint AS tools_tokens, \
             COALESCE(sum(u.results_tokens), 0)::bigint AS results_tokens, \
             COALESCE(sum(u.summary_tokens), 0)::bigint AS summary_tokens, \
             COALESCE(sum(u.attachments_tokens), 0)::bigint AS attachments_tokens, \
             COALESCE(sum(u.overhead_tokens), 0)::bigint AS overhead_tokens, \
             min(u.recorded_at) AS first_at, \
             max(u.recorded_at) AS last_at ",
            $tail
        )
    };
}

/// A chat that has never run a turn totals zero rather than returning nothing.
pub async fn chat_totals(pool: &PgPool, chat: Uuid) -> DbResult<Totals> {
    sqlx::query_as(totals_query!(
        "",
        "FROM chat_token_usage u WHERE u.chat_id = $1"
    ))
    .bind(chat)
    .fetch_one(pool)
    .await
}

/// One chat's shares, newest first, at most `limit` of them.
pub async fn chat_turns(pool: &PgPool, chat: Uuid, limit: i64) -> DbResult<Vec<TurnRow>> {
    sqlx::query_as(turn_query!(
        "FROM chat_token_usage \
         WHERE chat_id = $1 \
         ORDER BY recorded_at DESC, turn_id, model \
         LIMIT $2"
    ))
    .bind(chat)
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ChatTotals {
    pub chat_id: Uuid,
    pub title: String,
    #[sqlx(flatten)]
    pub totals: Totals,
}

/// The workspace's chats, heaviest first, counting only turns recorded at or
/// after `since`.
pub async fn workspace_chats(
    pool: &PgPool,
    workspace: Uuid,
    since: DateTime<Utc>,
    limit: i64,
) -> DbResult<Vec<ChatTotals>> {
    sqlx::query_as(totals_query!(
        "u.chat_id, c.title, ",
        "FROM chat_token_usage u \
         JOIN chats c ON c.id = u.chat_id \
         WHERE u.workspace_id = $1 AND u.recorded_at >= $2 \
         GROUP BY u.chat_id, c.title \
         ORDER BY total_tokens DESC, turns DESC, u.chat_id \
         LIMIT $3"
    ))
    .bind(workspace)
    .bind(since)
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ModelTotals {
    pub model: String,
    #[sqlx(flatten)]
    pub totals: Totals,
}

/// The workspace's models, heaviest first, turns of deleted chats and
/// messages included.
pub async fn workspace_models(
    pool: &PgPool,
    workspace: Uuid,
    since: DateTime<Utc>,
) -> DbResult<Vec<ModelTotals>> {
    sqlx::query_as(totals_query!(
        "u.model, ",
        "FROM chat_token_usage u \
         WHERE u.workspace_id = $1 AND u.recorded_at >= $2 \
         GROUP BY u.model \
         ORDER BY total_tokens DESC, turns DESC, u.model"
    ))
    .bind(workspace)
    .bind(since)
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTEXT: Breakdown = Breakdown {
        total: 9_000,
        instructions: 4_000,
        conversation: 3_000,
        tools: 1_500,
        results: 400,
        summary: 50,
        attachments: Some(10),
        overhead: 40,
        estimated: true,
    };

    fn share(model: &str, context: Option<Breakdown>) -> Share {
        Share {
            model: model.to_string(),
            reported: Some(Reported {
                prompt_tokens: 1_200,
                completion_tokens: 300,
                total_tokens: 1_500,
            }),
            rounds: 3,
            tool_calls: 2,
            context,
        }
    }

    fn turn(message: Uuid, chat: Uuid, workspace: Uuid) -> Turn {
        Turn {
            message_id: message,
            chat_id: chat,
            workspace_id: workspace,
            reasoning_effort: "auto".to_string(),
            shares: vec![share("llama3", Some(CONTEXT))],
        }
    }

    #[test]
    fn the_spliced_queries_are_well_formed() {
        let chat = totals_query!("", "FROM chat_token_usage u WHERE u.chat_id = $1");
        assert!(
            chat.starts_with("SELECT count(DISTINCT u.turn_id)::bigint AS turns,"),
            "no grouping columns means the counts come first: {chat}"
        );
        assert!(
            chat.contains("max(u.recorded_at) AS last_at FROM chat_token_usage"),
            "the last column and the FROM need a space between them: {chat}"
        );

        let grouped = totals_query!("u.model, ", "FROM chat_token_usage u GROUP BY u.model");
        assert!(
            grouped.starts_with("SELECT u.model, count(DISTINCT u.turn_id)::bigint AS turns,"),
            "the grouping column comes before the counts: {grouped}"
        );

        let turns = turn_query!("FROM chat_token_usage WHERE chat_id = $1");
        assert!(
            turns.starts_with("SELECT turn_id, message_id, model,"),
            "{turns}"
        );
        assert!(
            turns.contains("recorded_at FROM chat_token_usage"),
            "the last column and the FROM need a space between them: {turns}"
        );
    }

    async fn pool() -> PgPool {
        PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
            .await
            .expect("the test database accepts connections")
    }

    struct Fixture {
        pool: PgPool,
        workspace: Uuid,
        chat: Uuid,
    }

    async fn fixture() -> Fixture {
        let pool = pool().await;
        let organization = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        let chat = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO organizations (id, name, slug) VALUES ($1, 'Token usage', $1::text)",
        )
        .bind(organization)
        .execute(&pool)
        .await
        .expect("an organization");
        sqlx::query("INSERT INTO workspaces (id, organization_id, name, slug) VALUES ($1, $2, 'Token usage', $1::text)")
            .bind(workspace)
            .bind(organization)
            .execute(&pool)
            .await
            .expect("a workspace");
        sqlx::query(
            "INSERT INTO chats (id, workspace_id, title, model_name) VALUES ($1, $2, 'Spend', 'llama3')",
        )
        .bind(chat)
        .bind(workspace)
        .execute(&pool)
        .await
        .expect("a chat");
        Fixture {
            pool,
            workspace,
            chat,
        }
    }

    async fn message(pool: &PgPool, chat: Uuid) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO messages (id, chat_id, role, content) VALUES ($1, $2, 'assistant', 'answer')",
        )
        .bind(id)
        .bind(chat)
        .execute(pool)
        .await
        .expect("an assistant message");
        id
    }

    fn hour_ago() -> DateTime<Utc> {
        Utc::now() - chrono::Duration::hours(1)
    }

    #[tokio::test]
    async fn a_chat_with_no_turns_totals_zero() {
        let Fixture { pool, chat, .. } = fixture().await;
        let totals = chat_totals(&pool, chat).await.expect("totals");
        assert_eq!(totals.turns, 0);
        assert_eq!(totals.total_tokens, 0);
        assert_eq!(totals.first_at, None);
    }

    #[tokio::test]
    async fn a_recorded_turn_adds_to_the_chat_total() {
        let Fixture {
            pool,
            chat,
            workspace,
        } = fixture().await;
        let first = message(&pool, chat).await;
        let second = message(&pool, chat).await;
        record(&pool, &turn(first, chat, workspace))
            .await
            .expect("the first turn");
        record(&pool, &turn(second, chat, workspace))
            .await
            .expect("the second turn");

        let totals = chat_totals(&pool, chat).await.expect("totals");
        assert_eq!(totals.turns, 2);
        assert_eq!(totals.reported_turns, 2);
        assert_eq!(totals.total_tokens, 3_000);
        assert_eq!(totals.prompt_tokens, 2_400);
        assert_eq!(totals.rounds, 6);
        assert_eq!(
            totals.peak_context_tokens, 9_000,
            "the peak is one turn's context, not the sum of both"
        );
    }

    #[tokio::test]
    async fn a_turn_the_provider_did_not_report_still_carries_its_breakdown() {
        let Fixture {
            pool,
            chat,
            workspace,
        } = fixture().await;
        let only = message(&pool, chat).await;
        let mut spent = turn(only, chat, workspace);
        spent.shares[0].reported = None;
        record(&pool, &spent).await.expect("the turn");

        let totals = chat_totals(&pool, chat).await.expect("totals");
        assert_eq!(totals.turns, 1);
        assert_eq!(
            totals.reported_turns, 0,
            "a provider that said nothing must not count towards a reported average"
        );
        assert_eq!(totals.total_tokens, 0);
        assert_eq!(totals.instructions_tokens, 4_000);

        let turns = chat_turns(&pool, chat, 10).await.expect("the turns");
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].total_tokens, None);
        assert_eq!(turns[0].instructions_tokens, Some(4_000));
    }

    #[tokio::test]
    async fn a_handed_over_turn_is_one_turn_split_between_its_models() {
        let Fixture {
            pool,
            chat,
            workspace,
        } = fixture().await;
        let only = message(&pool, chat).await;
        let mut spent = turn(only, chat, workspace);
        spent.shares = vec![share("sonnet", None), share("gpt-5", Some(CONTEXT))];
        record(&pool, &spent).await.expect("the turn");

        let totals = chat_totals(&pool, chat).await.expect("totals");
        assert_eq!(totals.turns, 1, "two models, one turn");
        assert_eq!(totals.reported_turns, 1);
        assert_eq!(totals.total_tokens, 3_000);
        assert_eq!(
            totals.instructions_tokens, 4_000,
            "the closing context is counted once, not once per model"
        );

        let models = workspace_models(&pool, workspace, hour_ago())
            .await
            .expect("the models");
        assert_eq!(models.len(), 2, "each model is its own row: {models:?}");
        for model in &models {
            assert_eq!(model.totals.turns, 1, "{model:?}");
            assert_eq!(model.totals.total_tokens, 1_500, "{model:?}");
        }
    }

    #[tokio::test]
    async fn the_workspace_rollup_groups_by_chat_and_by_model() {
        let Fixture {
            pool,
            chat,
            workspace,
        } = fixture().await;
        let first = message(&pool, chat).await;
        let second = message(&pool, chat).await;
        record(&pool, &turn(first, chat, workspace))
            .await
            .expect("the first turn");
        let mut other = turn(second, chat, workspace);
        other.shares[0].model = "qwen3".to_string();
        record(&pool, &other).await.expect("the second turn");

        let chats = workspace_chats(&pool, workspace, hour_ago(), 10)
            .await
            .expect("the chats");
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].chat_id, chat);
        assert_eq!(chats[0].totals.turns, 2);

        let models = workspace_models(&pool, workspace, hour_ago())
            .await
            .expect("the models");
        assert_eq!(models.len(), 2, "each model is its own row: {models:?}");
        assert!(models.iter().all(|model| model.totals.turns == 1));
    }

    #[tokio::test]
    async fn deleting_the_message_keeps_what_its_turn_spent() {
        let Fixture {
            pool,
            chat,
            workspace,
        } = fixture().await;
        let only = message(&pool, chat).await;
        record(&pool, &turn(only, chat, workspace))
            .await
            .expect("the turn");
        sqlx::query("DELETE FROM messages WHERE id = $1")
            .bind(only)
            .execute(&pool)
            .await
            .expect("the message is deleted");

        let models = workspace_models(&pool, workspace, hour_ago())
            .await
            .expect("the models");
        assert_eq!(models.len(), 1, "{models:?}");
        assert_eq!(models[0].totals.turns, 1);
        assert_eq!(models[0].totals.total_tokens, 1_500);
        let totals = chat_totals(&pool, chat).await.expect("totals");
        assert_eq!(totals.total_tokens, 1_500, "the chat still counts it");
        let turns = chat_turns(&pool, chat, 10).await.expect("the turns");
        assert_eq!(turns[0].turn_id, only);
        assert_eq!(turns[0].message_id, None);
    }

    #[tokio::test]
    async fn deleting_the_chat_keeps_what_its_turns_spent_in_the_workspace() {
        let Fixture {
            pool,
            chat,
            workspace,
        } = fixture().await;
        let only = message(&pool, chat).await;
        record(&pool, &turn(only, chat, workspace))
            .await
            .expect("the turn");
        sqlx::query("DELETE FROM chats WHERE id = $1")
            .bind(chat)
            .execute(&pool)
            .await
            .expect("the chat is deleted");

        let models = workspace_models(&pool, workspace, hour_ago())
            .await
            .expect("the models");
        assert_eq!(models.len(), 1, "{models:?}");
        assert_eq!(models[0].totals.total_tokens, 1_500);
    }

    #[tokio::test]
    async fn a_turn_whose_message_was_deleted_first_is_still_stored() {
        let Fixture {
            pool,
            chat,
            workspace,
        } = fixture().await;
        let gone = Uuid::new_v4();

        record(&pool, &turn(gone, chat, workspace))
            .await
            .expect("a deleted message does not refuse the write");

        let turns = chat_turns(&pool, chat, 10).await.expect("the turns");
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].turn_id, gone);
        assert_eq!(turns[0].message_id, None);
    }
}
