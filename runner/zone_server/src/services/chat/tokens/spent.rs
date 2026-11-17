use abnegate_agent::context::ContextUsage;
use abnegate_llm::{ReasoningEffort, Usage};
use uuid::Uuid;

use super::ledger::Ledger;
use super::share::Share;
use crate::db::chat_token_usage::{self, Breakdown, Reported, Turn};
use crate::metrics::BilledTokens;

const INSTRUCTIONS: &str = "instructions";
const CONVERSATION: &str = "conversation";
const TOOLS: &str = "tools";
const RESULTS: &str = "results";
const SUMMARY: &str = "summary";
const ATTACHMENTS: &str = "attachments";
const OVERHEAD: &str = "overhead";

/// One finished turn, as the generation that ran it saw it.
#[derive(Debug, Clone)]
pub struct Spent {
    pub message: Uuid,
    pub chat: Uuid,
    pub workspace: Uuid,
    /// The model the turn ended on, whose share carries the closing context.
    pub model: String,
    pub effort: ReasoningEffort,
    pub billed: Ledger,
    pub context: ContextUsage,
}

impl Spent {
    pub(super) fn turn(&self) -> Turn {
        let mut shares: Vec<chat_token_usage::Share> =
            self.billed.shares().iter().map(stored).collect();
        let closing = match shares.iter().position(|share| share.model == self.model) {
            Some(index) => index,
            None => {
                shares.push(stored(&Share::new(&self.model)));
                shares.len() - 1
            }
        };
        shares[closing].context = Some(self.breakdown());
        Turn {
            message_id: self.message,
            chat_id: self.chat,
            workspace_id: self.workspace,
            reasoning_effort: self.effort.as_str().to_string(),
            shares,
        }
    }

    fn breakdown(&self) -> Breakdown {
        let breakdown = &self.context.breakdown;
        Breakdown {
            total: to_bigint(self.context.used),
            instructions: to_bigint(breakdown.instructions),
            conversation: to_bigint(breakdown.conversation),
            tools: to_bigint(breakdown.tools),
            results: to_bigint(breakdown.results),
            summary: to_bigint(breakdown.summary),
            attachments: breakdown.attachments.map(to_bigint),
            overhead: to_bigint(breakdown.overhead),
            estimated: self.context.estimated,
        }
    }

    pub(super) fn categories(&self) -> Vec<(&'static str, u64)> {
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

pub(super) fn billed(usage: Usage) -> BilledTokens {
    BilledTokens {
        prompt: u64::from(usage.prompt_tokens),
        completion: u64::from(usage.completion_tokens),
        total: u64::from(usage.total_tokens),
    }
}

fn stored(share: &Share) -> chat_token_usage::Share {
    chat_token_usage::Share {
        model: share.model.clone(),
        reported: share.billed.map(|billed| Reported {
            prompt_tokens: i64::from(billed.prompt_tokens),
            completion_tokens: i64::from(billed.completion_tokens),
            total_tokens: i64::from(billed.total_tokens),
        }),
        rounds: to_integer(share.rounds),
        tool_calls: to_integer(share.tool_calls),
        context: None,
    }
}

/// Saturates rather than failing, since an overflow is a bug upstream and not
/// worth the whole row.
fn to_bigint(tokens: u64) -> i64 {
    i64::try_from(tokens).unwrap_or(i64::MAX)
}

fn to_integer(count: usize) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use abnegate_agent::context::{ContextBreakdown, ContextSource, ContextStatus};

    /// Both context types are `#[non_exhaustive]`, so they can only be built
    /// outside their crate by mutating a default.
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

    fn spent(billed: Ledger) -> Spent {
        Spent {
            message: Uuid::nil(),
            chat: Uuid::nil(),
            workspace: Uuid::nil(),
            model: "llama3".to_string(),
            effort: ReasoningEffort::Medium,
            billed,
            context: context(),
        }
    }

    fn reported(rounds: usize, usage: Option<Usage>) -> Ledger {
        let mut ledger = Ledger::default();
        for _ in 0..rounds {
            ledger.round("llama3");
        }
        ledger.called("llama3");
        ledger.called("llama3");
        if let Some(usage) = usage {
            ledger.add("llama3", usage);
        }
        ledger
    }

    #[test]
    fn the_row_carries_the_providers_counts_and_zones_breakdown() {
        let turn = spent(reported(3, Some(Usage::new(1_200, 300)))).turn();

        let [share] = turn.shares.as_slice() else {
            panic!("one model, one share: {turn:?}");
        };
        assert_eq!(
            share.reported,
            Some(Reported {
                prompt_tokens: 1_200,
                completion_tokens: 300,
                total_tokens: 1_500,
            })
        );
        let context = share
            .context
            .expect("the closing model carries the context");
        assert_eq!(context.total, 9_000);
        assert_eq!(context.instructions, 4_000);
        assert_eq!(context.attachments, Some(10));
        assert!(context.estimated);
        assert_eq!(share.rounds, 3);
        assert_eq!(share.tool_calls, 2);
        assert_eq!(turn.reasoning_effort, "medium");
    }

    #[test]
    fn a_provider_that_reported_nothing_leaves_the_counts_absent() {
        let turn = spent(reported(2, None)).turn();

        assert_eq!(
            turn.shares[0].reported, None,
            "an unreported turn must not be stored as a free one"
        );
        assert_eq!(
            turn.shares[0].rounds, 2,
            "rounds are zone's own count, reported or not"
        );
        assert_eq!(
            turn.shares[0].context.map(|context| context.instructions),
            Some(4_000),
            "zone's own breakdown is observed either way"
        );
    }

    #[test]
    fn a_handed_over_turn_bills_each_model_and_closes_on_the_last() {
        let mut ledger = Ledger::default();
        ledger.round("sonnet");
        ledger.add("sonnet", Usage::new(3, 2));
        ledger.round("llama3");
        ledger.add("llama3", Usage::new(5, 7));

        let turn = spent(ledger).turn();

        let [first, last] = turn.shares.as_slice() else {
            panic!("two models, two shares: {turn:?}");
        };
        assert_eq!(first.model, "sonnet");
        assert_eq!(
            first.reported.map(|reported| reported.total_tokens),
            Some(5)
        );
        assert_eq!(
            first.context, None,
            "only the closing model carries the context"
        );
        assert_eq!(last.model, "llama3");
        assert_eq!(
            last.reported.map(|reported| reported.total_tokens),
            Some(12)
        );
        assert!(last.context.is_some());
    }

    #[test]
    fn a_turn_that_never_reached_its_model_still_keeps_its_context() {
        let turn = spent(Ledger::default()).turn();

        let [share] = turn.shares.as_slice() else {
            panic!("{turn:?}");
        };
        assert_eq!(share.model, "llama3");
        assert_eq!(share.rounds, 0);
        assert!(share.context.is_some());
    }

    #[test]
    fn an_absent_attachment_count_is_not_published_as_zero() {
        let mut spent = spent(Ledger::default());
        spent.context.breakdown.attachments = None;

        let categories = spent.categories();
        assert!(
            !categories.iter().any(|(name, _)| *name == ATTACHMENTS),
            "a turn with no attachments reports no attachment category: {categories:?}"
        );
        assert_eq!(spent.breakdown().attachments, None);
    }

    #[test]
    fn every_breakdown_category_is_published() {
        let spent = spent(Ledger::default());
        let categories = spent.categories();

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
            spent.context.breakdown.total(),
            "the published categories account for the whole breakdown"
        );
    }

    #[test]
    fn a_count_too_large_for_postgres_is_clamped_rather_than_lost() {
        let mut spent = spent(Ledger::default());
        spent.context.used = u64::MAX;

        assert_eq!(spent.breakdown().total, i64::MAX);
    }
}
