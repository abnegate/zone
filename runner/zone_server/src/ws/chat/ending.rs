//! How an answer ended, as far as the reader needs telling.

use std::borrow::Cow;

use super::{CUT_SHORT_NOTE, INTERRUPTED_NOTE, TRUNCATED_NOTE};

/// The longest reason a note carries. A provider that answers a broken turn
/// with a paragraph, or with its whole traceback, says the useful part first;
/// the rest would bury the answer the reader came back for.
const LONGEST_REASON: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ending<'a> {
    /// The model finished it.
    Finished,
    /// It ran past the length limit, which is what the reader can act on, so it outranks the rest.
    Truncated,
    /// The turn failed partway, whatever the agent kept, for the reason it failed for. A turn
    /// that has no reason to give still says it was interrupted.
    Interrupted(Option<&'a str>),
    /// A coding agent was stopped before it finished, keeping the answer it had given.
    CutShort,
}

impl Ending<'_> {
    /// What the stored answer closes with, so a reader can tell it did not end on its own.
    ///
    /// The reason is stored with the note rather than sent beside it: the frame that carries a
    /// failure is live only, so a reader who navigates away and back is otherwise left with a
    /// marker and nothing that says what broke.
    pub(super) fn note(self) -> Option<Cow<'static, str>> {
        match self {
            Self::Finished => None,
            Self::Truncated => Some(Cow::Borrowed(TRUNCATED_NOTE)),
            Self::Interrupted(reason) => Some(match reason.and_then(said) {
                Some(reason) => Cow::Owned(format!("\n\n[Response interrupted: {reason}]")),
                None => Cow::Borrowed(INTERRUPTED_NOTE),
            }),
            Self::CutShort => Some(Cow::Borrowed(CUT_SHORT_NOTE)),
        }
    }
}

/// `reason` as a note can carry it: one line, bounded, and nothing at all when it says nothing.
fn said(reason: &str) -> Option<String> {
    let said = reason.split('\n').find_map(|line| {
        let line = line.trim();
        (!line.is_empty()).then_some(line)
    })?;
    let said = match said.char_indices().nth(LONGEST_REASON) {
        Some((end, _)) => format!("{}…", said[..end].trim_end()),
        None => said.to_string(),
    };
    Some(said)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_interruption_with_no_reason_to_give_still_says_it_was_interrupted() {
        assert_eq!(
            Ending::Interrupted(None).note(),
            Some(Cow::Borrowed(INTERRUPTED_NOTE))
        );
        assert_eq!(
            Ending::Interrupted(Some("  \n ")).note(),
            Some(Cow::Borrowed(INTERRUPTED_NOTE)),
            "a reason that is only whitespace is no reason"
        );
    }

    #[test]
    fn an_interruption_keeps_why_it_broke_where_the_reader_finds_it_again() {
        let note = Ending::Interrupted(Some("Stream error: connection reset"))
            .note()
            .expect("an interrupted answer is marked");
        assert_eq!(
            note, "\n\n[Response interrupted: Stream error: connection reset]",
            "the stored answer has to carry the reason, since the error frame does not survive a reload"
        );
    }

    #[test]
    fn a_reason_that_runs_on_is_cut_to_its_first_line_and_bounded() {
        let note = Ending::Interrupted(Some(
            "\nStream error: the gateway gave up\nTraceback: line 1\nline 2",
        ))
        .note()
        .expect("an interrupted answer is marked");
        assert_eq!(
            note,
            "\n\n[Response interrupted: Stream error: the gateway gave up]"
        );

        let long = "x".repeat(LONGEST_REASON * 2);
        let note = Ending::Interrupted(Some(&long))
            .note()
            .expect("an interrupted answer is marked");
        assert!(
            note.chars().count() < long.chars().count(),
            "a paragraph of provider noise must not become the answer"
        );
        assert!(note.ends_with("…]"), "a cut reason says it was cut: {note}");
    }

    #[test]
    fn an_answer_cut_short_is_marked_as_neither_finished_nor_failed() {
        assert_eq!(Ending::Finished.note(), None);
        assert_eq!(Ending::CutShort.note(), Some(Cow::Borrowed(CUT_SHORT_NOTE)));
        assert_ne!(Ending::CutShort.note(), Ending::Interrupted(None).note());
        assert_ne!(Ending::CutShort.note(), Ending::Truncated.note());
    }
}
