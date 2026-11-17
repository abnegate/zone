use abnegate_llm::Usage;

/// Where one turn's usage ends and the next begins differs by backend. An
/// API request is one stream, so a usage frame after its finish reason
/// restates that request. A coding agent's stream can hold several agent
/// turns, and its parser reports each turn's usage before that turn's
/// `Finished`, so usage after a finish belongs to the next turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Tally {
    turns: bool,
    finished: Option<Usage>,
    current: Option<Usage>,
    closed: bool,
}

impl Tally {
    pub(super) fn request() -> Self {
        Self::new(false)
    }

    pub(super) fn agent() -> Self {
        Self::new(true)
    }

    fn new(turns: bool) -> Self {
        Self {
            turns,
            finished: None,
            current: None,
            closed: false,
        }
    }

    pub(super) fn observe(&mut self, usage: Usage) {
        if self.turns && self.closed {
            self.finished = self.current;
            self.closed = false;
        }
        self.current = Some(self.finished.map_or(usage, |finished| finished.plus(usage)));
    }

    pub(super) fn finish(&mut self) {
        self.closed = true;
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
        let mut tally = Tally::agent();
        tally.observe(Usage::new(10, 1));
        tally.observe(Usage::new(20, 2));
        tally.observe(Usage::new(30, 3));
        tally.finish();

        assert_eq!(tally.total(), Some(Usage::new(30, 3)));
    }

    #[test]
    fn finished_agent_turns_add_up() {
        let mut tally = Tally::agent();
        tally.observe(Usage::new(10, 1));
        tally.finish();
        tally.observe(Usage::new(4, 4));
        tally.observe(Usage::new(5, 5));
        tally.finish();

        assert_eq!(tally.total(), Some(Usage::new(15, 6)));
    }

    #[test]
    fn usage_reported_after_the_finish_is_counted_once() {
        let mut tally = Tally::request();
        tally.observe(Usage::new(7, 2));
        tally.finish();
        tally.observe(Usage::new(7, 3));

        assert_eq!(tally.total(), Some(Usage::new(7, 3)));
    }

    #[test]
    fn a_usage_only_frame_after_the_finish_is_the_requests_whole_usage() {
        let mut tally = Tally::request();
        tally.finish();
        tally.observe(Usage::new(7, 3));

        assert_eq!(tally.total(), Some(Usage::new(7, 3)));
    }

    #[test]
    fn a_next_agent_turn_that_reports_less_before_it_writes_does_not_replace_the_finished_one() {
        let mut tally = Tally::agent();
        tally.observe(Usage::new(10, 1));
        tally.finish();
        tally.observe(Usage::new(4, 0));
        tally.observe(Usage::new(4, 2));

        assert_eq!(tally.total(), Some(Usage::new(14, 3)));
    }

    #[test]
    fn a_next_agent_turn_whose_context_grew_is_still_a_turn_of_its_own() {
        let mut tally = Tally::agent();
        tally.observe(Usage::new(10_000, 50));
        tally.finish();
        tally.observe(Usage::new(10_200, 80));

        assert_eq!(tally.total(), Some(Usage::new(20_200, 130)));
    }

    #[test]
    fn a_stream_that_reported_nothing_totals_nothing() {
        let mut tally = Tally::agent();
        tally.finish();

        assert_eq!(tally.total(), None);
    }
}
