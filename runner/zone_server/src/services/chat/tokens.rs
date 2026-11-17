mod ledger;
mod pending;
mod share;
mod spent;

pub use ledger::Ledger;
pub use pending::Pending;
pub use share::Share;
pub use spent::Spent;

use sqlx::PgPool;

use crate::db::chat_token_usage;
use crate::metrics;

/// A failed write is logged rather than raised, because the answer it describes
/// is already delivered.
pub async fn record(pool: &PgPool, spent: &Spent) {
    publish(spent);
    if let Err(error) = chat_token_usage::record(pool, &spent.turn()).await {
        tracing::warn!(
            chat = %spent.chat,
            message = %spent.message,
            %error,
            "Could not store what this turn spent; the answer itself is stored"
        );
    }
}

fn publish(spent: &Spent) {
    metrics::record_chat_turn(
        &spent.model,
        spent
            .ledger
            .total()
            .map(|total| u64::from(total.total_tokens)),
        spent.ledger.rounds() as u64,
        spent.ledger.tool_calls() as u64,
    );
    for share in spent.ledger.shares() {
        if let Some(reported) = share.reported {
            metrics::record_chat_tokens(&share.model, spent::counted(reported));
        }
    }
    metrics::record_chat_context(&spent.categories());
}

#[cfg(test)]
mod tests {
    use super::*;
    use abnegate_llm::{ReasoningEffort, Usage};
    use uuid::Uuid;

    #[test]
    fn a_handed_over_turn_observes_the_turn_histogram_once_under_its_final_model() {
        let mut ledger = Ledger::default();
        ledger.round("handover-first");
        ledger.add("handover-first", Usage::new(3, 2));
        ledger.round("handover-last");
        ledger.add("handover-last", Usage::new(5, 7));

        publish(&Spent {
            message: Uuid::new_v4(),
            chat: Uuid::new_v4(),
            workspace: Uuid::new_v4(),
            model: "handover-last".to_string(),
            effort: ReasoningEffort::Medium,
            ledger,
            context: None,
        });

        let body = metrics::rendered();
        let line = |series: &str| {
            body.lines()
                .find(|line| line.starts_with(series))
                .unwrap_or_else(|| panic!("no {series} in {body}"))
                .to_string()
        };
        assert!(
            line("zone_chat_turn_tokens_count{model=\"handover-last\"}").ends_with(" 1"),
            "the turn is observed once"
        );
        assert!(
            line("zone_chat_turn_tokens_sum{model=\"handover-last\"}").ends_with(" 17"),
            "with both models' tokens"
        );
        assert!(
            !body.contains("zone_chat_turn_tokens_count{model=\"handover-first\"}"),
            "{body}"
        );
        assert!(
            line("zone_chat_tokens_total{model=\"handover-first\",kind=\"prompt\"}")
                .ends_with(" 3"),
            "per-model spend stays exact"
        );
        assert!(
            line("zone_chat_tokens_total{model=\"handover-last\",kind=\"prompt\"}").ends_with(" 5")
        );
    }
}
