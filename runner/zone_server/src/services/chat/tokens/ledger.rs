use abnegate_llm::Usage;

use super::share::Share;

/// What a turn spent so far, kept per model so a turn handed over to another
/// model bills each model for its own part.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ledger {
    shares: Vec<Share>,
}

impl Ledger {
    pub fn round(&mut self, model: &str) {
        let share = self.share(model);
        share.rounds = share.rounds.saturating_add(1);
    }

    pub fn called(&mut self, model: &str) {
        let share = self.share(model);
        share.tool_calls = share.tool_calls.saturating_add(1);
    }

    /// Add one round's `usage` to what `model` was billed.
    pub fn add(&mut self, model: &str, usage: Usage) {
        let share = self.share(model);
        share.billed = Some(share.billed.map_or(usage, |billed| billed.plus(usage)));
    }

    /// Every model's bill together, or `None` when no provider reported one.
    pub fn total(&self) -> Option<Usage> {
        self.shares
            .iter()
            .filter_map(|share| share.billed)
            .reduce(Usage::plus)
    }

    pub fn rounds(&self) -> usize {
        self.shares.iter().map(|share| share.rounds).sum()
    }

    pub fn tool_calls(&self) -> usize {
        self.shares.iter().map(|share| share.tool_calls).sum()
    }

    pub fn shares(&self) -> &[Share] {
        &self.shares
    }

    fn share(&mut self, model: &str) -> &mut Share {
        let index = match self.shares.iter().position(|share| share.model == model) {
            Some(index) => index,
            None => {
                self.shares.push(Share::new(model));
                self.shares.len() - 1
            }
        };
        &mut self.shares[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_model_is_billed_only_for_its_own_rounds() {
        let mut ledger = Ledger::default();
        ledger.round("sonnet");
        ledger.add("sonnet", Usage::new(3, 2));
        ledger.called("sonnet");
        ledger.round("gpt-5");
        ledger.add("gpt-5", Usage::new(5, 7));
        ledger.round("gpt-5");
        ledger.add("gpt-5", Usage::new(1, 1));

        let shares = ledger.shares();
        assert_eq!(shares.len(), 2, "{shares:?}");
        assert_eq!(shares[0].model, "sonnet");
        assert_eq!(shares[0].billed, Some(Usage::new(3, 2)));
        assert_eq!(shares[0].rounds, 1);
        assert_eq!(shares[0].tool_calls, 1);
        assert_eq!(shares[1].model, "gpt-5");
        assert_eq!(shares[1].billed, Some(Usage::new(6, 8)));
        assert_eq!(shares[1].rounds, 2);
        assert_eq!(ledger.total(), Some(Usage::new(9, 10)));
        assert_eq!(ledger.rounds(), 3);
        assert_eq!(ledger.tool_calls(), 1);
    }

    #[test]
    fn a_model_handed_back_to_keeps_one_share() {
        let mut ledger = Ledger::default();
        ledger.add("sonnet", Usage::new(1, 1));
        ledger.add("gpt-5", Usage::new(2, 2));
        ledger.add("sonnet", Usage::new(3, 3));

        assert_eq!(ledger.shares().len(), 2);
        assert_eq!(ledger.shares()[0].billed, Some(Usage::new(4, 4)));
    }

    #[test]
    fn rounds_are_counted_when_no_provider_reports_usage() {
        let mut ledger = Ledger::default();
        ledger.round("llama3");
        ledger.round("llama3");

        assert_eq!(ledger.rounds(), 2);
        assert_eq!(ledger.total(), None, "an unreported turn is not a free one");
    }
}
