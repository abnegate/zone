use abnegate_llm::Usage;

/// A provider may restate a finished turn's usage after its finish, so a turn
/// only gives way to the next once the stream carries content again.
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

    pub(super) fn content(&mut self) {
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
        tally.content();
        tally.observe(Usage::new(10, 1));
        tally.content();
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
        tally.content();
        tally.observe(Usage::new(4, 4));
        tally.observe(Usage::new(5, 5));
        tally.finish();

        assert_eq!(tally.total(), Some(Usage::new(15, 6)));
    }

    #[test]
    fn usage_reported_after_the_finish_is_counted_once() {
        let mut tally = Tally::default();
        tally.content();
        tally.observe(Usage::new(7, 2));
        tally.finish();
        tally.observe(Usage::new(7, 3));

        assert_eq!(tally.total(), Some(Usage::new(7, 3)));
    }

    #[test]
    fn a_stream_that_reported_nothing_totals_nothing() {
        let mut tally = Tally::default();
        tally.content();
        tally.finish();

        assert_eq!(tally.total(), None);
    }
}
