//! What each chat turn spent in tokens, kept per turn so a change meant to cut
//! token use can be measured rather than argued about.
//!
//! Two numbers are stored side by side because they answer different
//! questions. The provider's own `prompt_tokens`/`completion_tokens` are what
//! the turn was billed; they are absent when the provider reported nothing,
//! which several of the coding-agent backends do, and a NULL says so instead
//! of a zero that would quietly drag an average down. Zone's own end-of-turn
//! breakdown is always present, because zone computes it, and it says *where*
//! the prompt went: the system instructions, the conversation, the tool
//! schemas, the tool results, the summary, the attachments. A prompt that is
//! mostly tool schemas and a prompt that is mostly conversation want opposite
//! fixes, and the breakdown is the only thing that tells them apart.

use chrono::{DateTime, Utc};
use sqlx::{Executor, PgPool, Postgres};
use uuid::Uuid;

use super::DbResult;

/// The provider's own count for one turn, as it reported it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reported {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    /// What the provider billed in all, which can exceed the other two
    /// together once cache writes and reads are counted.
    pub total_tokens: i64,
}

/// Where the turn's final context stood, by category.
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
}

/// One assistant turn's spend, as the turn that produced it saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub message_id: Uuid,
    pub chat_id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub model: String,
    pub reasoning_effort: String,
    /// What the provider said, when it said anything.
    pub reported: Option<Reported>,
    /// Model rounds the turn took. An agentic turn replays its whole context
    /// every round, so this is usually the largest single lever on the bill.
    pub rounds: i32,
    pub tool_calls: i32,
    pub context: Breakdown,
    /// Whether the breakdown is an estimate rather than a tokenizer's count.
    pub estimated: bool,
}

/// Store what `turn` spent, replacing an earlier row for the same message.
///
/// A turn whose close had to be retried would otherwise be counted twice, and
/// double-counting one turn is worse than losing it: the figure it corrupts is
/// the one an experiment is read from.
pub async fn record<'connection, E>(executor: E, turn: &Turn) -> DbResult<()>
where
    E: Executor<'connection, Database = Postgres>,
{
    sqlx::query(
        "INSERT INTO chat_token_usage ( \
             message_id, chat_id, workspace_id, model, reasoning_effort, \
             prompt_tokens, completion_tokens, total_tokens, rounds, tool_calls, \
             context_tokens, instructions_tokens, conversation_tokens, tools_tokens, \
             results_tokens, summary_tokens, attachments_tokens, overhead_tokens, estimated \
         ) VALUES ( \
             $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, \
             $11, $12, $13, $14, $15, $16, $17, $18, $19 \
         ) \
         ON CONFLICT (message_id) DO UPDATE SET \
             model = EXCLUDED.model, \
             reasoning_effort = EXCLUDED.reasoning_effort, \
             prompt_tokens = EXCLUDED.prompt_tokens, \
             completion_tokens = EXCLUDED.completion_tokens, \
             total_tokens = EXCLUDED.total_tokens, \
             rounds = EXCLUDED.rounds, \
             tool_calls = EXCLUDED.tool_calls, \
             context_tokens = EXCLUDED.context_tokens, \
             instructions_tokens = EXCLUDED.instructions_tokens, \
             conversation_tokens = EXCLUDED.conversation_tokens, \
             tools_tokens = EXCLUDED.tools_tokens, \
             results_tokens = EXCLUDED.results_tokens, \
             summary_tokens = EXCLUDED.summary_tokens, \
             attachments_tokens = EXCLUDED.attachments_tokens, \
             overhead_tokens = EXCLUDED.overhead_tokens, \
             estimated = EXCLUDED.estimated, \
             recorded_at = now()",
    )
    .bind(turn.message_id)
    .bind(turn.chat_id)
    .bind(turn.workspace_id)
    .bind(&turn.model)
    .bind(&turn.reasoning_effort)
    .bind(turn.reported.map(|reported| reported.prompt_tokens))
    .bind(turn.reported.map(|reported| reported.completion_tokens))
    .bind(turn.reported.map(|reported| reported.total_tokens))
    .bind(turn.rounds)
    .bind(turn.tool_calls)
    .bind(turn.context.total)
    .bind(turn.context.instructions)
    .bind(turn.context.conversation)
    .bind(turn.context.tools)
    .bind(turn.context.results)
    .bind(turn.context.summary)
    .bind(turn.context.attachments)
    .bind(turn.context.overhead)
    .bind(turn.estimated)
    .execute(executor)
    .await
    .map(|_| ())
}

/// One turn as it was stored, newest first when listed.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct TurnRow {
    pub message_id: Uuid,
    pub model: String,
    pub reasoning_effort: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub rounds: i32,
    pub tool_calls: i32,
    pub context_tokens: i64,
    pub instructions_tokens: i64,
    pub conversation_tokens: i64,
    pub tools_tokens: i64,
    pub results_tokens: i64,
    pub summary_tokens: i64,
    pub attachments_tokens: Option<i64>,
    pub overhead_tokens: i64,
    pub estimated: bool,
    pub recorded_at: DateTime<Utc>,
}

/// One turn's columns, spliced into a query rather than written out three
/// times. sqlx 0.9 takes only a `&'static str`, so a shared fragment has to go
/// through `concat!` on literals; a `format!` of the same pieces is refused as
/// dynamic SQL, and rightly, since the compiler cannot tell a const from a
/// request parameter.
macro_rules! turn_query {
    ($tail:literal) => {
        concat!(
            "SELECT message_id, model, reasoning_effort, prompt_tokens, \
             completion_tokens, total_tokens, rounds, tool_calls, context_tokens, \
             instructions_tokens, conversation_tokens, tools_tokens, results_tokens, \
             summary_tokens, attachments_tokens, overhead_tokens, estimated, recorded_at ",
            $tail
        )
    };
}

/// What a set of turns adds up to.
///
/// `turns` counts every row and `reported_turns` only those the provider gave
/// a figure for, so a total read against the wrong denominator is visible
/// rather than plausible.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Totals {
    pub turns: i64,
    pub reported_turns: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub rounds: i64,
    pub tool_calls: i64,
    /// The largest context any one turn ended on, which is what the window has
    /// to hold; summing it across turns would mean nothing.
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

/// The same for the totals, with `$grouped` naming whatever the rollup groups
/// by. `count(total_tokens)` counts only the rows a provider reported, which is
/// the denominator a per-turn average belongs over.
macro_rules! totals_query {
    ($grouped:literal, $tail:literal) => {
        concat!(
            "SELECT ",
            $grouped,
            "count(*)::bigint AS turns, \
             count(total_tokens)::bigint AS reported_turns, \
             COALESCE(sum(prompt_tokens), 0)::bigint AS prompt_tokens, \
             COALESCE(sum(completion_tokens), 0)::bigint AS completion_tokens, \
             COALESCE(sum(total_tokens), 0)::bigint AS total_tokens, \
             COALESCE(sum(rounds), 0)::bigint AS rounds, \
             COALESCE(sum(tool_calls), 0)::bigint AS tool_calls, \
             COALESCE(max(context_tokens), 0)::bigint AS peak_context_tokens, \
             COALESCE(sum(instructions_tokens), 0)::bigint AS instructions_tokens, \
             COALESCE(sum(conversation_tokens), 0)::bigint AS conversation_tokens, \
             COALESCE(sum(tools_tokens), 0)::bigint AS tools_tokens, \
             COALESCE(sum(results_tokens), 0)::bigint AS results_tokens, \
             COALESCE(sum(summary_tokens), 0)::bigint AS summary_tokens, \
             COALESCE(sum(attachments_tokens), 0)::bigint AS attachments_tokens, \
             COALESCE(sum(overhead_tokens), 0)::bigint AS overhead_tokens, \
             min(recorded_at) AS first_at, \
             max(recorded_at) AS last_at ",
            $tail
        )
    };
}

/// Everything one chat has spent. A chat that has never run a turn totals zero
/// rather than returning nothing, because "no turns yet" and "no such chat"
/// are the caller's two different answers and the route already knows which.
pub async fn chat_totals(pool: &PgPool, chat: Uuid) -> DbResult<Totals> {
    sqlx::query_as(totals_query!(
        "",
        "FROM chat_token_usage WHERE chat_id = $1"
    ))
    .bind(chat)
    .fetch_one(pool)
    .await
}

/// One chat's turns, newest first, at most `limit` of them.
pub async fn chat_turns(pool: &PgPool, chat: Uuid, limit: i64) -> DbResult<Vec<TurnRow>> {
    sqlx::query_as(turn_query!(
        "FROM chat_token_usage \
         WHERE chat_id = $1 \
         ORDER BY recorded_at DESC, message_id DESC \
         LIMIT $2"
    ))
    .bind(chat)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// One chat's spend inside a workspace rollup, with the title that names it.
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

/// One model's spend across a workspace.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ModelTotals {
    pub model: String,
    #[sqlx(flatten)]
    pub totals: Totals,
}

/// The workspace's models, heaviest first. Comparing two models over the same
/// window is the cheapest experiment there is, so it gets its own query.
pub async fn workspace_models(
    pool: &PgPool,
    workspace: Uuid,
    since: DateTime<Utc>,
) -> DbResult<Vec<ModelTotals>> {
    sqlx::query_as(totals_query!(
        "model, ",
        "FROM chat_token_usage \
         WHERE workspace_id = $1 AND recorded_at >= $2 \
         GROUP BY model \
         ORDER BY total_tokens DESC, turns DESC, model"
    ))
    .bind(workspace)
    .bind(since)
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(message: Uuid, chat: Uuid, workspace: Uuid) -> Turn {
        Turn {
            message_id: message,
            chat_id: chat,
            workspace_id: Some(workspace),
            model: "llama3".to_string(),
            reasoning_effort: "auto".to_string(),
            reported: Some(Reported {
                prompt_tokens: 1_200,
                completion_tokens: 300,
                total_tokens: 1_500,
            }),
            rounds: 3,
            tool_calls: 2,
            context: Breakdown {
                total: 9_000,
                instructions: 4_000,
                conversation: 3_000,
                tools: 1_500,
                results: 400,
                summary: 50,
                attachments: Some(10),
                overhead: 40,
            },
            estimated: true,
        }
    }

    /// `concat!` cannot tell that two fragments need a space between them, and
    /// a query missing one fails only when it reaches Postgres. These assert
    /// the splice, so the mistake shows up without a database.
    #[test]
    fn the_spliced_queries_are_well_formed() {
        let chat = totals_query!("", "FROM chat_token_usage WHERE chat_id = $1");
        assert!(
            chat.starts_with("SELECT count(*)::bigint AS turns,"),
            "no grouping columns means the counts come first: {chat}"
        );
        assert!(
            chat.contains("max(recorded_at) AS last_at FROM chat_token_usage"),
            "the last column and the FROM need a space between them: {chat}"
        );

        let grouped = totals_query!("model, ", "FROM chat_token_usage GROUP BY model");
        assert!(
            grouped.starts_with("SELECT model, count(*)::bigint AS turns,"),
            "the grouping column comes before the counts: {grouped}"
        );

        let turns = turn_query!("FROM chat_token_usage WHERE chat_id = $1");
        assert!(turns.starts_with("SELECT message_id, model,"), "{turns}");
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
    async fn recording_the_same_turn_twice_does_not_double_count_it() {
        let Fixture {
            pool,
            chat,
            workspace,
        } = fixture().await;
        let only = message(&pool, chat).await;
        let mut spent = turn(only, chat, workspace);
        record(&pool, &spent).await.expect("the first write");
        spent.reported = Some(Reported {
            prompt_tokens: 1_000,
            completion_tokens: 100,
            total_tokens: 1_100,
        });
        record(&pool, &spent).await.expect("the replacement");

        let totals = chat_totals(&pool, chat).await.expect("totals");
        assert_eq!(totals.turns, 1);
        assert_eq!(totals.total_tokens, 1_100);
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
        spent.reported = None;
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
        assert_eq!(turns[0].instructions_tokens, 4_000);
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
        other.model = "qwen3".to_string();
        record(&pool, &other).await.expect("the second turn");

        let since = Utc::now() - chrono::Duration::hours(1);
        let chats = workspace_chats(&pool, workspace, since, 10)
            .await
            .expect("the chats");
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].chat_id, chat);
        assert_eq!(chats[0].totals.turns, 2);

        let models = workspace_models(&pool, workspace, since)
            .await
            .expect("the models");
        assert_eq!(models.len(), 2, "each model is its own row: {models:?}");
        assert!(models.iter().all(|model| model.totals.turns == 1));
    }

    #[tokio::test]
    async fn deleting_the_message_takes_its_usage_row_with_it() {
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

        let totals = chat_totals(&pool, chat).await.expect("totals");
        assert_eq!(totals.turns, 0);
    }
}
