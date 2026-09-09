//! Item 15b: how the agent went about a change, condensed to something comparable.
//!
//! Two runs can both succeed while working in completely different ways: one reads
//! twenty files and edits once, another writes a failing test and loops on it. Which of
//! those works better in a given repository is exactly the thing worth learning, and it
//! cannot be learned without a shape that can be compared.
//!
//! A fingerprint keeps three things: the mix of tool kinds used, the order they were
//! used in (collapsed so a run of six reads is one reading phase), and the approach
//! those two imply. [`similarity`] scores two fingerprints against each other, and
//! [`StrategyFingerprint::digest`] identifies one exactly, so a repeated approach
//! updates a single learned fact instead of accumulating near-duplicates.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const DIGEST_CHARACTERS: usize = 32;

const TEST_COMMANDS: [&str; 12] = [
    "cargo test",
    "cargo nextest",
    "npm test",
    "yarn test",
    "pnpm test",
    "bun test",
    "pytest",
    "go test",
    "make test",
    "jest",
    "vitest",
    "phpunit",
];

/// What a tool call was doing, independent of the tool's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Read,
    Search,
    Edit,
    Execute,
    Other,
}

impl ToolKind {
    pub const ALL: [ToolKind; 5] = [
        ToolKind::Read,
        ToolKind::Search,
        ToolKind::Edit,
        ToolKind::Execute,
        ToolKind::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ToolKind::Read => "read",
            ToolKind::Search => "search",
            ToolKind::Edit => "edit",
            ToolKind::Execute => "execute",
            ToolKind::Other => "other",
        }
    }

    /// Classify by what the tool's name says it does, so a new tool slots in without
    /// this list having to know about it.
    pub fn of(tool_name: &str) -> Self {
        let name = tool_name.to_ascii_lowercase();
        let mentions = |needles: &[&str]| needles.iter().any(|needle| name.contains(needle));

        if mentions(&[
            "write", "edit", "patch", "apply", "replace", "insert", "delete",
        ]) {
            ToolKind::Edit
        } else if mentions(&["search", "grep", "glob", "find", "list", "query", "lookup"]) {
            ToolKind::Search
        } else if mentions(&[
            "bash", "shell", "exec", "run", "command", "terminal", "script",
        ]) {
            ToolKind::Execute
        } else if mentions(&["read", "open", "view", "cat", "fetch", "get"]) {
            ToolKind::Read
        } else {
            ToolKind::Other
        }
    }
}

impl fmt::Display for ToolKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One tool call, in the order the agent made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInvocation {
    pub tool_name: String,
    /// The shell command, when the tool ran one. Used to spot test runs.
    pub command: Option<String>,
}

impl ToolInvocation {
    pub fn new(tool_name: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            command: None,
        }
    }

    pub fn with_command(tool_name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            command: Some(command.into()),
        }
    }

    pub fn kind(&self) -> ToolKind {
        ToolKind::of(&self.tool_name)
    }

    fn runs_tests(&self) -> bool {
        let Some(command) = &self.command else {
            return false;
        };
        let command = command.to_ascii_lowercase();
        TEST_COMMANDS.iter().any(|needle| command.contains(needle))
    }
}

/// The shape a run's work took.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixApproach {
    /// Edits driven by a test loop.
    TestDriven,
    /// Read far more than it wrote before committing to a change.
    Investigation,
    /// Went straight at the code.
    DirectFix,
    /// Looked around and changed nothing.
    Exploration,
    /// Did nothing recognisable.
    Unknown,
}

impl FixApproach {
    pub const ALL: [FixApproach; 5] = [
        FixApproach::TestDriven,
        FixApproach::Investigation,
        FixApproach::DirectFix,
        FixApproach::Exploration,
        FixApproach::Unknown,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            FixApproach::TestDriven => "test_driven",
            FixApproach::Investigation => "investigation",
            FixApproach::DirectFix => "direct_fix",
            FixApproach::Exploration => "exploration",
            FixApproach::Unknown => "unknown",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.as_str() == text)
    }
}

impl fmt::Display for FixApproach {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// How the agent worked, in a form two runs can be compared through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyFingerprint {
    /// Stable identity of this shape of work.
    pub digest: String,
    pub approach: FixApproach,
    /// The order of work with consecutive repeats collapsed, so twenty reads in a row
    /// read as one phase of reading rather than twenty separate steps.
    pub phases: Vec<ToolKind>,
    /// How many calls of each kind, keyed in a fixed order so the digest is stable.
    pub tool_mix: BTreeMap<ToolKind, usize>,
    pub tests_run: usize,
    pub files_touched: usize,
}

impl StrategyFingerprint {
    pub fn calls(&self) -> usize {
        self.tool_mix.values().sum()
    }

    fn count(&self, kind: ToolKind) -> usize {
        self.tool_mix.get(&kind).copied().unwrap_or_default()
    }

    /// One line naming the approach and the evidence for it.
    pub fn summary(&self) -> String {
        format!(
            "{} - {} calls across {} phases, {} file(s) touched, {} test run(s)",
            self.approach,
            self.calls(),
            self.phases.len(),
            self.files_touched,
            self.tests_run
        )
    }
}

fn approach_for(reads: usize, edits: usize, searches: usize, tests_run: usize) -> FixApproach {
    if tests_run > 0 && edits > 0 {
        FixApproach::TestDriven
    } else if edits > 0 && reads + searches > edits * 2 {
        FixApproach::Investigation
    } else if edits > 0 {
        FixApproach::DirectFix
    } else if reads + searches > 0 {
        FixApproach::Exploration
    } else {
        FixApproach::Unknown
    }
}

fn collapse(kinds: impl Iterator<Item = ToolKind>) -> Vec<ToolKind> {
    let mut phases: Vec<ToolKind> = Vec::new();
    for kind in kinds {
        if phases.last() != Some(&kind) {
            phases.push(kind);
        }
    }
    phases
}

fn digest_of(
    approach: FixApproach,
    phases: &[ToolKind],
    mix: &BTreeMap<ToolKind, usize>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(approach.as_str().as_bytes());
    hasher.update(b"|");
    for phase in phases {
        hasher.update(phase.as_str().as_bytes());
        hasher.update(b">");
    }
    hasher.update(b"|");
    for (kind, count) in mix {
        hasher.update(format!("{kind}={count};").as_bytes());
    }
    hex::encode(hasher.finalize())
        .chars()
        .take(DIGEST_CHARACTERS)
        .collect()
}

/// Build a fingerprint from an ordered run of tool calls. Pure.
pub fn fingerprint(calls: &[ToolInvocation], files_touched: usize) -> StrategyFingerprint {
    let mut tool_mix: BTreeMap<ToolKind, usize> = BTreeMap::new();
    let mut tests_run = 0usize;

    for call in calls {
        *tool_mix.entry(call.kind()).or_default() += 1;
        if call.runs_tests() {
            tests_run += 1;
        }
    }

    let phases = collapse(calls.iter().map(ToolInvocation::kind));
    let reads = tool_mix.get(&ToolKind::Read).copied().unwrap_or_default();
    let edits = tool_mix.get(&ToolKind::Edit).copied().unwrap_or_default();
    let searches = tool_mix.get(&ToolKind::Search).copied().unwrap_or_default();
    let approach = approach_for(reads, edits, searches, tests_run);

    StrategyFingerprint {
        digest: digest_of(approach, &phases, &tool_mix),
        approach,
        phases,
        tool_mix,
        tests_run,
        files_touched,
    }
}

fn mix_cosine(left: &StrategyFingerprint, right: &StrategyFingerprint) -> f32 {
    let (dot, left_norm, right_norm) = ToolKind::ALL.iter().fold(
        (0.0f32, 0.0f32, 0.0f32),
        |(dot, left_norm, right_norm), kind| {
            let first = left.count(*kind) as f32;
            let second = right.count(*kind) as f32;
            (
                dot + first * second,
                left_norm + first * first,
                right_norm + second * second,
            )
        },
    );

    let magnitude = left_norm.sqrt() * right_norm.sqrt();
    if magnitude == 0.0 || !magnitude.is_finite() {
        return 0.0;
    }
    dot / magnitude
}

fn bigrams(phases: &[ToolKind]) -> BTreeSet<(ToolKind, ToolKind)> {
    phases
        .windows(2)
        .map(|window| (window[0], window[1]))
        .collect()
}

fn order_overlap(left: &StrategyFingerprint, right: &StrategyFingerprint) -> f32 {
    let first = bigrams(&left.phases);
    let second = bigrams(&right.phases);

    if first.is_empty() && second.is_empty() {
        let same: BTreeSet<ToolKind> = left.phases.iter().copied().collect();
        let other: BTreeSet<ToolKind> = right.phases.iter().copied().collect();
        if same.is_empty() && other.is_empty() {
            return 1.0;
        }
        let union = same.union(&other).count();
        return same.intersection(&other).count() as f32 / union as f32;
    }

    let union = first.union(&second).count();
    if union == 0 {
        return 0.0;
    }
    first.intersection(&second).count() as f32 / union as f32
}

/// How alike two ways of working are, in `0.0..=1.0`.
///
/// Half the score is the mix of tools used and half is the order they were used in, so
/// two runs that touched the same tools in a different sequence are not treated as the
/// same approach.
pub fn similarity(left: &StrategyFingerprint, right: &StrategyFingerprint) -> f32 {
    0.5 * mix_cosine(left, right) + 0.5 * order_overlap(left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(path: &str) -> ToolInvocation {
        ToolInvocation::new(format!("read_file:{path}"))
    }

    fn edit() -> ToolInvocation {
        ToolInvocation::new("write_file")
    }

    fn run_tests() -> ToolInvocation {
        ToolInvocation::with_command("bash", "cargo test -p zone_server --lib")
    }

    fn build() -> ToolInvocation {
        ToolInvocation::with_command("bash", "cargo build")
    }

    fn test_driven() -> StrategyFingerprint {
        fingerprint(
            &[
                read("src/lib.rs"),
                edit(),
                run_tests(),
                edit(),
                run_tests(),
                edit(),
                run_tests(),
            ],
            2,
        )
    }

    fn investigation() -> StrategyFingerprint {
        fingerprint(
            &[
                ToolInvocation::new("search_code"),
                read("a.rs"),
                read("b.rs"),
                read("c.rs"),
                read("d.rs"),
                read("e.rs"),
                edit(),
            ],
            1,
        )
    }

    #[test]
    fn tool_names_are_classified_by_what_they_do() {
        assert_eq!(ToolKind::of("write_file"), ToolKind::Edit);
        assert_eq!(ToolKind::of("read_repository_file"), ToolKind::Read);
        assert_eq!(ToolKind::of("search_code"), ToolKind::Search);
        assert_eq!(ToolKind::of("grep"), ToolKind::Search);
        assert_eq!(ToolKind::of("list_files"), ToolKind::Search);
        assert_eq!(ToolKind::of("bash"), ToolKind::Execute);
        assert_eq!(ToolKind::of("send_reminder"), ToolKind::Other);
    }

    #[test]
    fn an_empty_run_fingerprints_as_unknown() {
        let empty = fingerprint(&[], 0);
        assert_eq!(empty.approach, FixApproach::Unknown);
        assert!(empty.phases.is_empty());
        assert_eq!(empty.calls(), 0);
    }

    #[test]
    fn a_test_loop_reads_as_test_driven() {
        let strategy = test_driven();
        assert_eq!(strategy.approach, FixApproach::TestDriven);
        assert_eq!(strategy.tests_run, 3);
    }

    #[test]
    fn heavy_reading_before_one_edit_reads_as_investigation() {
        assert_eq!(investigation().approach, FixApproach::Investigation);
    }

    #[test]
    fn editing_without_looking_reads_as_a_direct_fix() {
        let strategy = fingerprint(&[read("a.rs"), edit(), edit()], 2);
        assert_eq!(strategy.approach, FixApproach::DirectFix);
    }

    #[test]
    fn looking_without_editing_reads_as_exploration() {
        let strategy = fingerprint(&[read("a.rs"), read("b.rs")], 0);
        assert_eq!(
            strategy.approach,
            FixApproach::Exploration,
            "a run that changed nothing explored, it did not fix"
        );
    }

    #[test]
    fn a_build_is_not_a_test_run() {
        let strategy = fingerprint(&[edit(), build()], 1);
        assert_eq!(strategy.tests_run, 0);
        assert_eq!(strategy.approach, FixApproach::DirectFix);
    }

    #[test]
    fn consecutive_calls_of_one_kind_collapse_into_one_phase() {
        let strategy = fingerprint(&[read("a"), read("b"), read("c"), edit(), edit()], 2);
        assert_eq!(
            strategy.phases,
            vec![ToolKind::Read, ToolKind::Edit],
            "three reads then two edits is two phases of work, not five steps"
        );
        assert_eq!(strategy.calls(), 5, "the counts still see every call");
    }

    #[test]
    fn two_genuinely_different_approaches_are_distinguished() {
        let looping = test_driven();
        let reading = investigation();

        assert_ne!(looping.digest, reading.digest);
        assert_ne!(looping.approach, reading.approach);
        assert!(
            similarity(&looping, &reading) < 0.5,
            "a test loop and a reading spree must not look like the same strategy: {}",
            similarity(&looping, &reading)
        );
    }

    #[test]
    fn the_same_approach_scores_as_itself() {
        let strategy = test_driven();
        assert!(
            (similarity(&strategy, &strategy) - 1.0).abs() < 1e-6,
            "a strategy must be maximally similar to itself"
        );
    }

    #[test]
    fn a_near_repeat_of_an_approach_still_looks_like_it() {
        let original = test_driven();
        let repeated = fingerprint(
            &[read("src/lib.rs"), edit(), run_tests(), edit(), run_tests()],
            2,
        );

        assert_eq!(repeated.approach, FixApproach::TestDriven);
        assert!(
            similarity(&original, &repeated) > 0.85,
            "one fewer loop iteration is the same strategy: {}",
            similarity(&original, &repeated)
        );
    }

    #[test]
    fn the_same_tools_in_a_different_order_are_not_the_same_strategy() {
        let first = fingerprint(&[read("a"), read("b"), edit(), run_tests()], 1);
        let second = fingerprint(&[run_tests(), edit(), read("a"), read("b")], 1);

        assert!(
            (mix_cosine(&first, &second) - 1.0).abs() < 1e-6,
            "the tool mix is identical by construction"
        );
        assert!(
            similarity(&first, &second) < 1.0,
            "writing the test first is not the same strategy as writing it last"
        );
        assert_ne!(first.digest, second.digest);
    }

    #[test]
    fn similarity_is_symmetric_and_bounded() {
        let pairs = [
            (test_driven(), investigation()),
            (test_driven(), fingerprint(&[], 0)),
            (investigation(), investigation()),
        ];

        for (left, right) in pairs {
            let forwards = similarity(&left, &right);
            let backwards = similarity(&right, &left);
            assert!(
                (forwards - backwards).abs() < 1e-6,
                "similarity must be symmetric"
            );
            assert!(
                (0.0..=1.0).contains(&forwards),
                "similarity escaped the unit range: {forwards}"
            );
        }
    }

    #[test]
    fn the_digest_depends_only_on_the_shape_of_the_work() {
        let first = fingerprint(&[read("src/one.rs"), edit()], 1);
        let second = fingerprint(&[read("src/two.rs"), edit()], 9);
        assert_eq!(
            first.digest, second.digest,
            "which file was read is not part of the strategy; the shape of the work is"
        );
    }

    #[test]
    fn fingerprinting_twice_gives_the_same_digest() {
        let calls = [read("a"), edit(), run_tests()];
        assert_eq!(
            fingerprint(&calls, 1).digest,
            fingerprint(&calls, 1).digest,
            "digests must be stable so a repeated strategy updates one learned fact"
        );
    }

    #[test]
    fn a_summary_names_the_approach_and_its_evidence() {
        let summary = test_driven().summary();
        assert!(summary.contains("test_driven"), "{summary}");
        assert!(summary.contains("3 test run(s)"), "{summary}");
    }

    #[test]
    fn approach_names_round_trip() {
        for approach in FixApproach::ALL {
            assert_eq!(FixApproach::parse(approach.as_str()), Some(approach));
        }
        assert_eq!(FixApproach::parse("vibes"), None);
    }
}
