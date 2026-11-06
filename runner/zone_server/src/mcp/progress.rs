//! Whether a turn served over MCP is still getting anywhere.
//!
//! Zone's own loop stops a turn whose round only repeats calls that already
//! failed with nothing changed since. Over MCP the agent runs the loop and
//! sends its calls one at a time, so there is no round to judge. A repeat is
//! refused unrun and the agent told why, and only a second repeat with nothing
//! learned in between stops the turn.

use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

type Signature = (String, String);

#[derive(Default)]
pub(super) struct Progress {
    failures: HashSet<Signature>,
    observations: HashMap<Signature, String>,
    /// Bumped whenever something changes. A call that fails after a change it
    /// started before is not remembered: it may succeed if made again.
    generation: usize,
    warned: bool,
}

impl Progress {
    pub(super) fn generation(&self) -> usize {
        self.generation
    }

    /// Whether this call already failed and nothing has changed since.
    pub(super) fn repeats(&self, signature: &Signature) -> bool {
        self.failures.contains(signature)
    }

    /// Note a refused repeat. True when the turn was already warned and has
    /// learned nothing since, which is when it should stop.
    pub(super) fn warn(&mut self) -> bool {
        std::mem::replace(&mut self.warned, true)
    }

    /// A successful change, or a read that sees something new, is progress and
    /// forgives every earlier failure. A failure not met before is progress
    /// too, as it is in zone's own loop: the agent learned why.
    pub(super) fn record(
        &mut self,
        signature: Signature,
        started: usize,
        mutating: bool,
        success: bool,
        output: &str,
    ) {
        if !success {
            if started == self.generation && self.failures.insert(signature) {
                self.warned = false;
            }
            return;
        }
        if mutating {
            self.observations.clear();
            self.changed();
            return;
        }
        let digest = hex::encode(Sha256::digest(output.as_bytes()));
        if self.observations.insert(signature, digest.clone()) != Some(digest) {
            self.changed();
        }
    }

    fn changed(&mut self) {
        self.failures.clear();
        self.generation += 1;
        self.warned = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str) -> Signature {
        (name.to_string(), "{}".to_string())
    }

    #[test]
    fn a_failed_call_repeats_until_something_changes() {
        let mut progress = Progress::default();
        progress.record(call("read_file"), 0, false, false, "missing");

        assert!(progress.repeats(&call("read_file")));
        assert!(!progress.repeats(&call("list_files")));

        progress.record(call("write_file"), 1, true, true, "written");

        assert!(!progress.repeats(&call("read_file")));
    }

    #[test]
    fn only_a_read_that_sees_something_new_forgives_a_failure() {
        let mut progress = Progress::default();
        progress.record(call("list_files"), 0, false, true, "a.txt");
        let started = progress.generation();
        progress.record(call("read_file"), started, false, false, "missing");

        progress.record(call("list_files"), started, false, true, "a.txt");
        assert!(
            progress.repeats(&call("read_file")),
            "reading the same thing again is not progress"
        );

        progress.record(call("list_files"), started, false, true, "a.txt\nb.txt");
        assert!(!progress.repeats(&call("read_file")));
    }

    #[test]
    fn a_failure_that_started_before_a_change_is_not_remembered() {
        let mut progress = Progress::default();
        let started = progress.generation();

        progress.record(call("write_file"), started, true, true, "written");
        progress.record(call("read_file"), started, false, false, "missing");

        assert!(
            !progress.repeats(&call("read_file")),
            "a read that raced a write may succeed if made again"
        );
    }

    #[test]
    fn a_turn_is_warned_once_and_learning_anything_forgives_the_warning() {
        let mut progress = Progress::default();

        assert!(!progress.warn(), "the first repeat is only a warning");
        assert!(
            progress.warn(),
            "a second repeat with nothing learned stops"
        );

        progress.record(call("read_file"), 0, false, false, "missing");

        assert!(!progress.warn(), "a new failure taught the agent something");
    }
}
