//! What a finished chat turn spent, recorded once for two readers.
//!
//! Prometheus answers whether the fleet is getting cheaper and `chat_token_usage`
//! answers which chat spent that, so both are written from the same observation
//! rather than from two walks of the same data that could disagree.
//!
//! Recording never fails a turn. The answer is already streamed and stored by
//! the time this runs, and losing one row of telemetry is a smaller harm than
//! turning a delivered answer into an error, so a failed write is logged and
//! the turn closes.

use abnegate_agent::context::ContextUsage;
use abnegate_llm::{ReasoningEffort, Usage};
use uuid::Uuid;

use crate::db::chat_token_usage::{self, Breakdown, Reported, Turn};
use crate::metrics::{self, BilledTokens};

/// The categories a context breakdown is split into, as Prometheus labels.
const INSTRUCTIONS: &str = "instructions";
const CONVERSATION: &str = "conversation";
const TOOLS: &str = "tools";
const RESULTS: &str = "results";
const SUMMARY: &str = "summary";
const ATTACHMENTS: &str = "attachments";
const OVERHEAD: &str = "overhead";

/// One finished turn, as the generation that ran it saw it.
#[derive(Debug, Clone, Copy)]
pub struct Spent<'turn> {
    /// The stored assistant message this turn produced. The row hangs off it,
    /// so deleting the message takes the row with it.
    pub message: Uuid,
    pub chat: Uuid,
    pub workspace: Uuid,
    pub model: &'turn str,
    pub effort: ReasoningEffort,
    /// The provider's own counts for every round of the turn added together,
    /// or `None` when it reported none.
    pub billed: Option<Usage>,
    /// Model rounds the turn took, counted as the usage reports it produced.
    pub rounds: usize,
    pub tool_calls: usize,
    /// Where the turn's context stood when it closed.
    pub context: &'turn ContextUsage,
}

impl Spent<'_> {
    fn row(&self) -> Turn {
        let breakdown = &self.context.breakdown;
        Turn {
            message_id: self.message,
            chat_id: self.chat,
            workspace_id: Some(self.workspace),
            model: self.model.to_string(),
            reasoning_effort: self.effort.as_str().to_string(),
            reported: self.billed.map(|billed| Reported {
                prompt_tokens: count(u64::from(billed.prompt_tokens)),
                completion_tokens: count(u64::from(billed.completion_tokens)),
                total_tokens: count(u64::from(billed.total_tokens)),
            }),
            rounds: clamp(self.rounds),
            tool_calls: clamp(self.tool_calls),
            context: Breakdown {
                total: count(self.context.used),
                instructions: count(breakdown.instructions),
                conversation: count(breakdown.conversation),
                tools: count(breakdown.tools),
                results: count(breakdown.results),
                summary: count(breakdown.summary),
                attachments: breakdown.attachments.map(count),
                overhead: count(breakdown.overhead),
            },
            estimated: self.context.estimated,
        }
    }

    fn billed(&self) -> Option<BilledTokens> {
        self.billed.map(|billed| BilledTokens {
            prompt: u64::from(billed.prompt_tokens),
            completion: u64::from(billed.completion_tokens),
            total: u64::from(billed.total_tokens),
        })
    }

    fn categories(&self) -> Vec<(&'static str, u64)> {
        let breakdown = &self.context.breakdown;
        let mut categories = vec![
            (INSTRUCTIONS, breakdown.instructions),
            (CONVERSATION, breakdown.conversation),
            (TOOLS, breakdown.tools),
            (RESULTS, breakdown.results),
            (SUMMARY, breakdown.summary),
            (OVERHEAD, breakdown.overhead),
        ];
        // Absent rather than zero: a turn that carried no attachment should not
        // pull the attachment distribution down towards nothing.
        if let Some(attachments) = breakdown.attachments {
            categories.push((ATTACHMENTS, attachments));
        }
        categories
    }
}

/// Postgres has no unsigned integers, and a count that overflowed `i64` is a
/// bug upstream rather than something to lose the whole row over.
fn count(tokens: u64) -> i64 {
    i64::try_from(tokens).unwrap_or(i64::MAX)
}

fn clamp(count: usize) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}

/// Count `spent` into Prometheus and store it against the turn's message.
pub async fn record(pool: &sqlx::PgPool, spent: Spent<'_>) {
    metrics::record_chat_turn(
        spent.model,
        spent.billed(),
        spent.rounds as u64,
        spent.tool_calls as u64,
    );
    metrics::record_chat_context(&spent.categories());
    if let Err(error) = chat_token_usage::record(pool, &spent.row()).await {
        tracing::warn!(
            chat = %spent.chat,
            message = %spent.message,
            %error,
            "Could not store what this turn spent; the answer itself is stored"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abnegate_agent::context::{ContextBreakdown, ContextSource, ContextStatus};

    /// Both context types are `#[non_exhaustive]`, which forbids struct
    /// literal syntax here, functional update included. Mutating a default is
    /// the only way to build one outside the defining crate, so the lint that
    /// would otherwise ask for a literal is waived rather than worked around.
    #[allow(clippy::field_reassign_with_default)]
    fn context() -> ContextUsage {
        let mut breakdown = ContextBreakdown::default();
        breakdown.instructions = 4_000;
        breakdown.conversation = 3_000;
        breakdown.tools = 1_500;
        breakdown.results = 400;
        breakdown.summary = 50;
        breakdown.attachments = Some(10);
        breakdown.overhead = 40;

        let mut usage = ContextUsage::default();
        usage.model = "llama3".to_string();
        usage.used = 9_000;
        usage.limit = Some(32_768);
        usage.reserved = 4_096;
        usage.estimated = true;
        usage.source = ContextSource::Configured;
        usage.status = ContextStatus::Ready;
        usage.breakdown = breakdown;
        usage
    }

    fn spent<'turn>(context: &'turn ContextUsage, billed: Option<Usage>) -> Spent<'turn> {
        Spent {
            message: Uuid::nil(),
            chat: Uuid::nil(),
            workspace: Uuid::nil(),
            model: "llama3",
            effort: ReasoningEffort::Medium,
            billed,
            rounds: 3,
            tool_calls: 2,
            context,
        }
    }

    #[test]
    fn the_row_carries_the_providers_counts_and_zones_breakdown() {
        let context = context();
        let row = spent(&context, Some(Usage::new(1_200, 300))).row();

        assert_eq!(
            row.reported,
            Some(Reported {
                prompt_tokens: 1_200,
                completion_tokens: 300,
                total_tokens: 1_500,
            })
        );
        assert_eq!(row.context.total, 9_000);
        assert_eq!(row.context.instructions, 4_000);
        assert_eq!(row.context.attachments, Some(10));
        assert_eq!(row.rounds, 3);
        assert_eq!(row.tool_calls, 2);
        assert_eq!(row.reasoning_effort, "medium");
        assert!(row.estimated);
    }

    #[test]
    fn a_provider_that_reported_nothing_leaves_the_counts_absent() {
        let context = context();
        let row = spent(&context, None).row();

        assert_eq!(
            row.reported, None,
            "an unreported turn must not be stored as a free one"
        );
        assert_eq!(
            row.context.instructions, 4_000,
            "zone's own breakdown is observed either way"
        );
    }

    #[test]
    fn an_absent_attachment_count_is_not_published_as_zero() {
        let mut context = context();
        context.breakdown.attachments = None;
        let spent = spent(&context, None);

        let categories = spent.categories();
        assert!(
            !categories.iter().any(|(name, _)| *name == ATTACHMENTS),
            "a turn with no attachments reports no attachment category: {categories:?}"
        );
        assert_eq!(spent.row().context.attachments, None);
    }

    #[test]
    fn every_breakdown_category_is_published() {
        let context = context();
        let categories = spent(&context, None).categories();

        assert_eq!(
            categories,
            vec![
                (INSTRUCTIONS, 4_000),
                (CONVERSATION, 3_000),
                (TOOLS, 1_500),
                (RESULTS, 400),
                (SUMMARY, 50),
                (OVERHEAD, 40),
                (ATTACHMENTS, 10),
            ]
        );
        assert_eq!(
            categories.iter().map(|(_, tokens)| tokens).sum::<u64>(),
            context.breakdown.total(),
            "the published categories account for the whole breakdown"
        );
    }

    #[test]
    fn a_count_too_large_for_postgres_is_clamped_rather_than_lost() {
        let mut context = context();
        context.used = u64::MAX;
        let row = spent(&context, None).row();

        assert_eq!(row.context.total, i64::MAX);
    }
}
