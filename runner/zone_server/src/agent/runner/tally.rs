use abnegate_llm::Usage;

/// What one completion stream was billed, read the way
/// `abnegate_agent_cli::StdoutParseResult` reads a coding agent's run: within
/// a turn each report restates the turn so far, so the latest stands, and
/// turns that finished add up. A turn ends at its finish but only gives way
/// to the next once that one writes something, since a provider may restate
/// the finished turn's usage after the finish.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Tally {
    finished: Option<Usage>,
    current: Option<Usage>,
    closed: bool,
}

impl Tally {
    pub(super) fn observe(&mut self, usage: Usage) {
        self.current = Some(self.finished.map_or(usage, |finished| finished.plus(usage)));
    }

    pub(super) fn finish(&mut self) {
        self.closed = true;
    }

    /// Note that the stream wrote something, which after a finish starts the next turn.
    pub(super) fn wrote(&mut self) {
        if self.closed {
            self.finished = self.current;
            self.closed = false;
        }
    }

    pub(super) fn total(&self) -> Option<Usage> {
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turns_result_replaces_the_messages_that_led_to_it() {
        let mut tally = Tally::default();
        tally.wrote();
        tally.observe(Usage::new(10, 1));
        tally.wrote();
        tally.observe(Usage::new(20, 2));
        tally.observe(Usage::new(30, 3));
        tally.finish();

        assert_eq!(tally.total(), Some(Usage::new(30, 3)));
    }

    #[test]
    fn finished_turns_add_up() {
        let mut tally = Tally::default();
        tally.observe(Usage::new(10, 1));
        tally.finish();
        tally.wrote();
        tally.observe(Usage::new(4, 4));
        tally.observe(Usage::new(5, 5));
        tally.finish();

        assert_eq!(tally.total(), Some(Usage::new(15, 6)));
    }

    #[test]
    fn usage_reported_after_the_finish_is_counted_once() {
        let mut tally = Tally::default();
        tally.wrote();
        tally.observe(Usage::new(7, 2));
        tally.finish();
        tally.observe(Usage::new(7, 3));

        assert_eq!(tally.total(), Some(Usage::new(7, 3)));
    }

    #[test]
    fn a_stream_that_reported_nothing_totals_nothing() {
        let mut tally = Tally::default();
        tally.wrote();
        tally.finish();

        assert_eq!(tally.total(), None);
    }
}
