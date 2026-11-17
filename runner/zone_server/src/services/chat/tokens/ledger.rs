use abnegate_llm::Usage;

use super::share::Share;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ledger {
    shares: Vec<Share>,
}

impl Ledger {
    pub fn round(&mut self, model: &str) {
        let share = self.share(model);
        share.rounds = share.rounds.saturating_add(1);
    }

    pub fn tool_call(&mut self, model: &str) {
        let share = self.share(model);
        share.tool_calls = share.tool_calls.saturating_add(1);
    }

    pub fn add(&mut self, model: &str, usage: Usage) {
        let share = self.share(model);
        share.reported = Some(
            share
                .reported
                .map_or(usage, |reported| reported.plus(usage)),
        );
    }

    pub fn total(&self) -> Option<Usage> {
        self.shares
            .iter()
            .filter_map(|share| share.reported)
            .reduce(Usage::plus)
    }

    pub fn is_empty(&self) -> bool {
        self.shares.is_empty()
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
    fn each_model_counts_only_its_own_rounds() {
        let mut ledger = Ledger::default();
        ledger.round("sonnet");
        ledger.add("sonnet", Usage::new(3, 2));
        ledger.tool_call("sonnet");
        ledger.round("gpt-5");
        ledger.add("gpt-5", Usage::new(5, 7));
        ledger.round("gpt-5");
        ledger.add("gpt-5", Usage::new(1, 1));

        let shares = ledger.shares();
        assert_eq!(shares.len(), 2, "{shares:?}");
        assert_eq!(shares[0].model, "sonnet");
        assert_eq!(shares[0].reported, Some(Usage::new(3, 2)));
        assert_eq!(shares[0].rounds, 1);
        assert_eq!(shares[0].tool_calls, 1);
        assert_eq!(shares[1].model, "gpt-5");
        assert_eq!(shares[1].reported, Some(Usage::new(6, 8)));
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
        assert_eq!(ledger.shares()[0].reported, Some(Usage::new(4, 4)));
    }

    #[test]
    fn a_ledger_nothing_was_added_to_is_empty() {
        let mut ledger = Ledger::default();
        assert!(ledger.is_empty());
        ledger.round("llama3");
        assert!(!ledger.is_empty());
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
