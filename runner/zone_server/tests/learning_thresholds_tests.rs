//! Thresholds and bounds that the in-tree tests state relative to their own
//! constant, and so cannot hold in place. Each number here is written out, and
//! each was seen to fail when its constant moved by one step: the convention
//! gates at 4 observations / 3 distinct runs / 75% agreement, the error
//! category's 0.05 margin over the runner-up, and the 64 KiB compiled-size
//! limit -- which the regex crate's own 10 MB default hides, because the bomb
//! the existing tests use is refused either way.

use regex::RegexBuilder;
use std::time::Instant;
use uuid::Uuid;

use zone_server::services::prioritisation::change::{Change, Origin};
use zone_server::services::prioritisation::suppression::{
    Field, MAX_PATTERN_LENGTH, MAX_RULES, MAX_SUBJECT_LENGTH, MatchMode, RejectedRule, Rejection,
    Rule, RuleSet,
};
use zone_server::workers::learning::convention::{ConventionPolicy, learn};
use zone_server::workers::learning::error_category::{
    CategorizationPolicy, CategoryReference, ErrorCategory, ReferenceEmbeddings, categorize,
};
use zone_server::workers::learning::observation::{ConventionKind, ConventionSignal};
use zone_server::workers::learning::review::{ReviewCategory, classify};

// Claim 1: regex size limits, and no backtracking on user patterns.

fn regex_rule(name: &str, pattern: &str) -> Rule {
    Rule {
        mode: MatchMode::Regex,
        ..Rule::new(name, Field::Title, pattern)
    }
}

fn titled(title: &str) -> Change {
    Change {
        title: title.to_string(),
        ..Change::new("probe", Origin::Task)
    }
}

/// The smallest `size_limit` at which a pattern compiles at all: its real program size.
fn compiled_size_of(pattern: &str) -> usize {
    let mut limit = 1_024;
    while limit <= 64 * 1024 * 1024 {
        if RegexBuilder::new(pattern).size_limit(limit).build().is_ok() {
            return limit;
        }
        limit *= 2;
    }
    panic!("{pattern} did not compile even at 64MiB");
}

/// The explicit limit is load-bearing, not a restatement of the crate default.
///
/// A 13 byte pattern needing 8MiB of compiled program sits under the regex crate's own
/// 10MiB default, so the default would admit it. The configured 64KiB limit refuses it.
#[test]
fn the_configured_limit_refuses_patterns_the_crate_default_would_admit() {
    for bomb in [
        "(?s).{0,6400}",
        "a{500}{500}",
        "\\p{Any}{0,5000}",
        "(?i)[a-z]{0,4000}",
    ] {
        assert!(
            RegexBuilder::new(bomb).build().is_ok(),
            "{bomb:?} must be admitted by the crate default, or this proves nothing"
        );
        let set = RuleSet::compile(vec![regex_rule("bomb", bomb)]);
        assert_eq!(
            set.len(),
            0,
            "{bomb:?} was admitted despite the configured size limit"
        );
        assert!(
            matches!(
                set.rejected().first().map(|entry| &entry.rejection),
                Some(Rejection::PatternTooComplex { .. })
            ),
            "{bomb:?} rejected for the wrong reason: {:?}",
            set.rejected()
        );
    }
}

/// One doubling either side of the 64KiB constant, so the bracket is tight.
#[test]
fn the_size_limit_sits_where_the_constant_says_it_does() {
    let under = "(?s).{0,50}";
    let over = "(?s).{0,100}";

    assert_eq!(
        compiled_size_of(under),
        64 * 1024,
        "the small pattern must need exactly the configured 64KiB and no more"
    );
    assert_eq!(
        compiled_size_of(over),
        128 * 1024,
        "the next pattern up must need the next doubling"
    );

    assert_eq!(
        RuleSet::compile(vec![regex_rule("small", under)]).len(),
        1,
        "a pattern inside the limit must compile"
    );
    assert_eq!(
        RuleSet::compile(vec![regex_rule("large", over)]).len(),
        0,
        "a pattern one doubling outside the limit must be refused"
    );
}

// Claim 2: exponential decay with a two-hour half-life.

// Claim 3: four observations, three distinct runs, 75% agreement, conjunctively.

fn signal(value: &str, run: u128) -> ConventionSignal {
    ConventionSignal {
        kind: ConventionKind::FileNaming,
        scope: "src/db".to_string(),
        value: value.to_string(),
        run_id: Uuid::from_u128(run),
    }
}

/// `winner` observations of "snake_case" and `competing` of "camelCase", spread over `runs`.
fn evidence(winner: usize, competing: usize, runs: usize) -> Vec<ConventionSignal> {
    let mut signals: Vec<ConventionSignal> = (0..winner)
        .map(|index| signal("snake_case", (index % runs) as u128 + 1))
        .collect();
    signals.extend((0..competing).map(|index| signal("camelCase", (index % runs) as u128 + 1)));
    signals
}

#[test]
fn every_convention_threshold_refuses_independently() {
    let policy = ConventionPolicy::default();
    let refused = [
        ("3 observations, 3 runs, 100%", evidence(3, 0, 3)),
        ("4 observations, 2 runs, 100%", evidence(4, 0, 2)),
        ("4 observations, 1 run, 100%", evidence(4, 0, 1)),
        ("74 of 100 agreement over 3 runs", evidence(74, 26, 3)),
        ("5 of 7 agreement (71%) over 3 runs", evidence(5, 2, 3)),
        ("11 of 15 agreement (73%) over 3 runs", evidence(11, 4, 3)),
    ];
    for (label, signals) in refused {
        let learned = learn(&signals, &policy);
        assert!(
            learned.is_empty(),
            "{label} must not promote, promoted {learned:?}"
        );
    }

    let promoted = [
        (
            "4 observations, 3 runs, 100%",
            evidence(4, 0, 3),
            4usize,
            1.0f32,
        ),
        (
            "75 of 100 agreement over 3 runs",
            evidence(75, 25, 3),
            75,
            0.75,
        ),
        (
            "6 of 8 agreement (exactly 75%) over 3 runs",
            evidence(6, 2, 3),
            6,
            0.75,
        ),
    ];
    for (label, signals, observations, agreement) in promoted {
        let learned = learn(&signals, &policy);
        assert_eq!(learned.len(), 1, "{label} must promote");
        assert_eq!(learned[0].value, "snake_case", "{label}");
        assert_eq!(learned[0].observations, observations, "{label}");
        assert_eq!(learned[0].distinct_runs, 3, "{label}");
        assert!(
            (learned[0].agreement - agreement).abs() < 1e-6,
            "{label}: agreement {} is not {agreement}",
            learned[0].agreement
        );
        println!(
            "{label}: observations={} runs={} agreement={:.6} confidence={:.6}",
            learned[0].observations,
            learned[0].distinct_runs,
            learned[0].agreement,
            learned[0].confidence
        );
    }

    assert_eq!(policy.minimum_observations, 4);
    assert_eq!(policy.minimum_distinct_runs, 3);
    assert!((policy.minimum_agreement - 0.75).abs() < 1e-6);
}

#[test]
fn the_convention_gates_are_conjunctive_not_disjunctive() {
    let policy = ConventionPolicy::default();
    // Each of these clears two gates and fails exactly one. All three must refuse.
    assert!(
        learn(&evidence(3, 0, 3), &policy).is_empty(),
        "runs and agreement pass, observations fail"
    );
    assert!(
        learn(&evidence(8, 0, 2), &policy).is_empty(),
        "observations and agreement pass, runs fail"
    );
    assert!(
        learn(&evidence(8, 4, 4), &policy).is_empty(),
        "observations and runs pass, agreement is 66% and fails"
    );
    assert_eq!(
        learn(&evidence(8, 0, 4), &policy).len(),
        1,
        "all three passing must promote"
    );
}

// Claim 4: an absolute similarity bar plus a margin over the runner-up.

const DIMENSION: usize = 12;

fn basis(index: usize) -> Vec<f32> {
    let mut vector = vec![0.0; DIMENSION];
    vector[index] = 1.0;
    vector
}

fn blend(first: usize, second: usize, weight: f32) -> Vec<f32> {
    let mut vector = vec![0.0; DIMENSION];
    vector[first] = 1.0 - weight;
    vector[second] = weight;
    vector
}

fn references() -> ReferenceEmbeddings {
    ReferenceEmbeddings::new(
        ErrorCategory::CLASSIFIED
            .into_iter()
            .enumerate()
            .map(|(index, category)| CategoryReference {
                category,
                exemplars: vec![basis(index)],
            })
            .collect(),
    )
}

fn direction(category: ErrorCategory) -> usize {
    ErrorCategory::CLASSIFIED
        .iter()
        .position(|candidate| *candidate == category)
        .expect("classified")
}

#[test]
fn the_margin_bar_brackets_a_near_tie() {
    let references = references();
    let policy = CategorizationPolicy::default();
    let build = direction(ErrorCategory::Build);
    let failure = direction(ErrorCategory::TestFailure);

    // A blend at 0.5 is an exact tie; the margin grows as the weight moves away from it.
    // `unbarred` reads the true similarity and margin, `policy` reads the verdict, so
    // each case is labelled with the numbers that decided it.
    let unbarred = CategorizationPolicy {
        minimum_similarity: 0.0,
        minimum_margin: 0.0,
    };

    for (weight, refused) in [(0.5f32, true), (0.49, true), (0.485, true), (0.48, false)] {
        let embedding = blend(build, failure, weight);
        let measured = categorize(&embedding, &references, &unbarred);
        let assigned = categorize(&embedding, &references, &policy);
        println!(
            "weight {weight}: similarity={:.6} margin={:.6} -> {:?}",
            measured.confidence, measured.margin, assigned.category
        );
        assert!(
            measured.confidence >= policy.minimum_similarity,
            "weight {weight} has similarity {:.6}, below the absolute bar, so the margin \
             would not be what decides it",
            measured.confidence
        );
        if refused {
            assert!(
                measured.margin < policy.minimum_margin,
                "weight {weight} margin {:.6} is not inside the bar",
                measured.margin
            );
            assert_eq!(
                assigned.category,
                ErrorCategory::Unknown,
                "weight {weight} is inside the margin and must be Unknown"
            );
        } else {
            assert!(
                measured.margin >= policy.minimum_margin,
                "weight {weight} margin {:.6} is not outside the bar",
                measured.margin
            );
            assert_eq!(
                assigned.category,
                ErrorCategory::Build,
                "weight {weight} clears the margin and must be assigned"
            );
            assert!((assigned.margin - measured.margin).abs() < 1e-6);
        }
    }

    assert!((policy.minimum_margin - 0.05).abs() < 1e-6);
    assert!((policy.minimum_similarity - 0.45).abs() < 1e-6);
}

// Claim 5: whole-word sequences, not substrings.

#[test]
fn the_named_false_positive_is_gone_and_substring_matching_would_still_have_it() {
    let comment = "Please rebase onto the latest release.";
    assert_eq!(
        classify(comment).category,
        ReviewCategory::Other,
        "'the latest release' is not a comment about missing tests"
    );

    // "test" on its own is enough to classify, so a substring matcher over the same
    // vocabulary would have filed this comment under missing tests.
    let bare = classify("test");
    assert_eq!(bare.category, ReviewCategory::MissingTests);
    assert_eq!(
        bare.evidence,
        ["test"],
        "the matched phrase is the bare word"
    );
    assert!(
        comment.to_lowercase().contains("test"),
        "'latest' contains 'test', which is what substring matching would have found"
    );
}

// Claim 6: readiness refuses evidence whose arithmetic does not close.

/// Regression: the length cap must apply to every match mode.
///
/// It used to live inside `build`, which only runs for `Regex` mode, so a `Contains`
/// or `Exact` pattern of any length was admitted. `contains_ignoring_case` is a naive
/// window search, so 256 rules holding a 4KB literal cost 4.3 billion byte comparisons
/// for one change: 15.8s in a debug build before this was fixed.
#[test]
fn an_over_long_pattern_is_refused_in_every_match_mode() {
    let oversized = "a".repeat(MAX_PATTERN_LENGTH + 1);

    for mode in [MatchMode::Contains, MatchMode::Exact, MatchMode::Regex] {
        let set = RuleSet::compile(vec![Rule {
            mode,
            ..Rule::new("long", Field::Title, oversized.clone())
        }]);
        assert_eq!(set.len(), 0, "{mode:?} admitted an over-long pattern");
        assert_eq!(
            set.rejected(),
            [RejectedRule {
                name: "long".into(),
                rejection: Rejection::PatternTooLong {
                    length: MAX_PATTERN_LENGTH + 1,
                    maximum: MAX_PATTERN_LENGTH,
                },
            }],
            "{mode:?} did not refuse it for its length"
        );
    }
}

/// The cap is what keeps a whole rule set cheap to evaluate.
#[test]
fn a_full_rule_set_of_literal_patterns_evaluates_promptly() {
    let needle = format!("{}b", "a".repeat(MAX_PATTERN_LENGTH - 1));
    assert_eq!(
        needle.len(),
        MAX_PATTERN_LENGTH,
        "the longest allowed literal"
    );

    let rules: Vec<Rule> = (0..MAX_RULES)
        .map(|index| Rule::new(format!("rule-{index}"), Field::Title, needle.clone()))
        .collect();
    let set = RuleSet::compile(rules);
    assert_eq!(set.len(), MAX_RULES);

    let subject = "a".repeat(MAX_SUBJECT_LENGTH * 2);
    let started = Instant::now();
    let outcome = set.evaluate(&titled(&subject));
    let elapsed = started.elapsed();
    println!("{MAX_RULES} worst-case literal rules over a bounded subject: {elapsed:?}");

    assert!(
        !outcome.suppressed(),
        "the needle ends in 'b' and cannot match"
    );
    // The quadratic scan took 3.9s here and the linear one takes ~16ms, so this
    // bound is two orders of magnitude clear of the defect while leaving room
    // for a loaded runner. A tighter one would flake rather than measure.
    assert!(
        elapsed.as_millis() < 500,
        "evaluating one change took {elapsed:?}, which is a denial of service surface"
    );
}
