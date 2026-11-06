//! Whether a turn served over MCP is still getting anywhere.
//!
//! Zone's own loop stops a turn that repeats a call which already failed with
//! nothing changed since. Over MCP the agent runs the loop, so the same
//! judgement is made here, call by call, on the same terms.

use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

type Signature = (String, String);

#[derive(Default)]
pub(super) struct Progress {
    failures: HashSet<Signature>,
    observations: HashMap<Signature, String>,
}

impl Progress {
    /// Whether this call already failed and nothing has changed since.
    pub(super) fn repeats(&self, signature: &Signature) -> bool {
        self.failures.contains(signature)
    }

    /// A successful change, or a read that sees something new, is progress
    /// and forgives every earlier failure. A failure is remembered.
    pub(super) fn record(
        &mut self,
        signature: Signature,
        mutating: bool,
        success: bool,
        output: &str,
    ) {
        if !success {
            self.failures.insert(signature);
            return;
        }
        if mutating {
            self.failures.clear();
            self.observations.clear();
            return;
        }
        let digest = hex::encode(Sha256::digest(output.as_bytes()));
        if self.observations.insert(signature, digest.clone()) != Some(digest) {
            self.failures.clear();
        }
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
        progress.record(call("read_file"), false, false, "missing");

        assert!(progress.repeats(&call("read_file")));
        assert!(!progress.repeats(&call("list_files")));

        progress.record(call("write_file"), true, true, "written");

        assert!(!progress.repeats(&call("read_file")));
    }

    #[test]
    fn only_a_read_that_sees_something_new_forgives_a_failure() {
        let mut progress = Progress::default();
        progress.record(call("list_files"), false, true, "a.txt");
        progress.record(call("read_file"), false, false, "missing");

        progress.record(call("list_files"), false, true, "a.txt");
        assert!(
            progress.repeats(&call("read_file")),
            "reading the same thing again is not progress"
        );

        progress.record(call("list_files"), false, true, "a.txt\nb.txt");
        assert!(!progress.repeats(&call("read_file")));
    }
}
