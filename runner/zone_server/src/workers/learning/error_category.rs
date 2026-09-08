//! Item 11: what kind of failure a run hit, decided by meaning rather than spelling.
//!
//! Substring matching on error text is brittle in both directions. It misses the
//! failure it was meant to catch the moment a tool rewords its message, and it fires
//! on innocent text that merely contains the word — "latest" is not a test failure and
//! "author" is not an authorisation problem. Both mistakes teach the loop something
//! false.
//!
//! So each category is described by a handful of exemplar phrases, those phrases are
//! embedded once, and an error is categorised by how close its own embedding sits to
//! them. A category is only assigned when the best match clears an absolute similarity
//! bar *and* beats the runner-up by a margin: an error that looks equally like two
//! categories is recorded as [`ErrorCategory::Unknown`] rather than guessed at.

use std::fmt;

use serde::{Deserialize, Serialize};
use zone_context::embeddings::EmbeddingService;
use zone_context::error::{ContextError, Result as ContextResult};

use crate::workers::promotion::cosine_similarity;

const MINIMUM_SIMILARITY: f32 = 0.45;
const MINIMUM_MARGIN: f32 = 0.05;

/// The kinds of failure the loop distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Build,
    TestFailure,
    Dependency,
    Permission,
    Timeout,
    RateLimit,
    Network,
    MergeConflict,
    Configuration,
    Refusal,
    Unknown,
}

impl ErrorCategory {
    /// Every category an error can be assigned, excluding [`ErrorCategory::Unknown`],
    /// which is the answer when none of these fits well enough.
    pub const CLASSIFIED: [ErrorCategory; 10] = [
        ErrorCategory::Build,
        ErrorCategory::TestFailure,
        ErrorCategory::Dependency,
        ErrorCategory::Permission,
        ErrorCategory::Timeout,
        ErrorCategory::RateLimit,
        ErrorCategory::Network,
        ErrorCategory::MergeConflict,
        ErrorCategory::Configuration,
        ErrorCategory::Refusal,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCategory::Build => "build",
            ErrorCategory::TestFailure => "test_failure",
            ErrorCategory::Dependency => "dependency",
            ErrorCategory::Permission => "permission",
            ErrorCategory::Timeout => "timeout",
            ErrorCategory::RateLimit => "rate_limit",
            ErrorCategory::Network => "network",
            ErrorCategory::MergeConflict => "merge_conflict",
            ErrorCategory::Configuration => "configuration",
            ErrorCategory::Refusal => "refusal",
            ErrorCategory::Unknown => "unknown",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::CLASSIFIED
            .into_iter()
            .chain(std::iter::once(ErrorCategory::Unknown))
            .find(|category| category.as_str() == text)
    }

    /// The phrases that define this category. They are embedded once and every error
    /// is scored against them, so they must describe the failure rather than name it.
    pub fn exemplars(self) -> &'static [&'static str] {
        match self {
            ErrorCategory::Build => &[
                "the compiler rejected the code and the build stopped",
                "compilation error: mismatched types, unresolved name, borrow checker",
                "the project no longer builds after this change",
            ],
            ErrorCategory::TestFailure => &[
                "a test case failed and the assertion did not hold",
                "the test suite reported failures after the change",
                "expected one value but the code produced another",
            ],
            ErrorCategory::Dependency => &[
                "a package version is incompatible with another package",
                "the dependency could not be resolved or the lockfile is out of date",
                "a required crate or module is not installed",
            ],
            ErrorCategory::Permission => &[
                "permission denied when opening the file or directory",
                "the credentials were rejected and access was forbidden",
                "the token is not authorised to perform this operation",
            ],
            ErrorCategory::Timeout => &[
                "the operation ran past its deadline and was cancelled",
                "the command timed out before it produced a result",
                "the run exceeded the time budget allowed for it",
            ],
            ErrorCategory::RateLimit => &[
                "too many requests were sent and the service throttled them",
                "the rate limit was exceeded, retry after a delay",
                "quota exhausted for this account or api key",
            ],
            ErrorCategory::Network => &[
                "the connection was refused or reset before completing",
                "the host could not be reached and dns lookup failed",
                "a network error interrupted the transfer",
            ],
            ErrorCategory::MergeConflict => &[
                "the branch conflicts with the base and cannot be merged",
                "git reported conflicting changes in the same lines",
                "the rebase stopped on a conflict that needs resolving",
            ],
            ErrorCategory::Configuration => &[
                "a required setting or environment variable is missing",
                "the configuration file is malformed and could not be read",
                "the tool was pointed at a path that does not exist",
            ],
            ErrorCategory::Refusal => &[
                "the model declined to carry out the request",
                "the assistant stopped because it would not perform the task",
                "the request was rejected by a safety policy",
            ],
            ErrorCategory::Unknown => &[],
        }
    }
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The two bars an assignment must clear before it is believed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CategorizationPolicy {
    /// How close an error must sit to a category's nearest exemplar.
    pub minimum_similarity: f32,
    /// How far ahead of the runner-up the winner must be. Without this, an error
    /// sitting between two categories would be filed under whichever won by a hair.
    pub minimum_margin: f32,
}

impl Default for CategorizationPolicy {
    fn default() -> Self {
        Self {
            minimum_similarity: MINIMUM_SIMILARITY,
            minimum_margin: MINIMUM_MARGIN,
        }
    }
}

/// One category's exemplar embeddings.
#[derive(Debug, Clone, PartialEq)]
pub struct CategoryReference {
    pub category: ErrorCategory,
    pub exemplars: Vec<Vec<f32>>,
}

/// The embedded exemplars every error is scored against.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReferenceEmbeddings {
    references: Vec<CategoryReference>,
}

impl ReferenceEmbeddings {
    pub fn new(references: Vec<CategoryReference>) -> Self {
        Self { references }
    }

    pub fn is_empty(&self) -> bool {
        self.references.is_empty()
    }

    /// Embed every category's exemplars in one batch.
    pub async fn build(service: &dyn EmbeddingService) -> ContextResult<Self> {
        let mut phrases: Vec<&str> = Vec::new();
        for category in ErrorCategory::CLASSIFIED {
            phrases.extend_from_slice(category.exemplars());
        }

        let vectors = service.embed_batch(&phrases).await?;
        if vectors.len() != phrases.len() {
            return Err(ContextError::Embedding(format!(
                "embedding service returned {} vectors for {} exemplars",
                vectors.len(),
                phrases.len()
            )));
        }

        let mut cursor = 0;
        let mut references = Vec::with_capacity(ErrorCategory::CLASSIFIED.len());
        for category in ErrorCategory::CLASSIFIED {
            let count = category.exemplars().len();
            references.push(CategoryReference {
                category,
                exemplars: vectors[cursor..cursor + count].to_vec(),
            });
            cursor += count;
        }

        Ok(Self::new(references))
    }
}

/// A category assignment with the evidence that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Categorization {
    pub category: ErrorCategory,
    /// Similarity to the winning category's nearest exemplar.
    pub confidence: f32,
    /// How far the winner beat the runner-up. Zero when nothing else was close.
    pub margin: f32,
}

impl Categorization {
    pub fn unknown() -> Self {
        Self {
            category: ErrorCategory::Unknown,
            confidence: 0.0,
            margin: 0.0,
        }
    }
}

/// Assign a category to one embedded error message.
///
/// Pure over the embedding and the references, so every threshold is testable
/// without an embedding service or a database.
pub fn categorize(
    embedding: &[f32],
    references: &ReferenceEmbeddings,
    policy: &CategorizationPolicy,
) -> Categorization {
    if embedding.is_empty() || references.is_empty() {
        return Categorization::unknown();
    }

    let mut scored: Vec<(ErrorCategory, f32)> = references
        .references
        .iter()
        .map(|reference| {
            let best = reference
                .exemplars
                .iter()
                .map(|exemplar| cosine_similarity(exemplar, embedding))
                .fold(f32::NEG_INFINITY, f32::max);
            (reference.category, best)
        })
        .filter(|(_, similarity)| similarity.is_finite())
        .collect();

    scored.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then(left.0.as_str().cmp(right.0.as_str()))
    });

    let Some(&(category, confidence)) = scored.first() else {
        return Categorization::unknown();
    };

    let margin = scored
        .get(1)
        .map_or(confidence, |&(_, runner_up)| confidence - runner_up);

    if confidence < policy.minimum_similarity || margin < policy.minimum_margin {
        return Categorization::unknown();
    }

    Categorization {
        category,
        confidence,
        margin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// One orthogonal direction per category, so a synthetic error pointing along a
    /// direction is unambiguously that category.
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
            .expect("category is classified")
    }

    #[test]
    fn every_category_recovers_its_own_exemplar() {
        let references = references();
        for category in ErrorCategory::CLASSIFIED {
            let assigned = categorize(
                &basis(direction(category)),
                &references,
                &CategorizationPolicy::default(),
            );
            assert_eq!(
                assigned.category, category,
                "an error identical to {category}'s exemplar must be filed as {category}"
            );
            assert!(assigned.confidence > 0.99);
        }
    }

    #[test]
    fn related_failures_cluster_onto_one_category() {
        let references = references();
        let build = direction(ErrorCategory::Build);
        let unrelated = direction(ErrorCategory::Network);

        for weight in [0.0, 0.1, 0.2, 0.3] {
            let assigned = categorize(
                &blend(build, unrelated, weight),
                &references,
                &CategorizationPolicy::default(),
            );
            assert_eq!(
                assigned.category,
                ErrorCategory::Build,
                "a message {weight} of the way towards an unrelated category is still a build failure"
            );
        }
    }

    #[test]
    fn unrelated_failures_stay_apart() {
        let references = references();
        let policy = CategorizationPolicy::default();

        let timeout = categorize(
            &basis(direction(ErrorCategory::Timeout)),
            &references,
            &policy,
        );
        let conflict = categorize(
            &basis(direction(ErrorCategory::MergeConflict)),
            &references,
            &policy,
        );

        assert_eq!(timeout.category, ErrorCategory::Timeout);
        assert_eq!(conflict.category, ErrorCategory::MergeConflict);
        assert_ne!(
            timeout.category, conflict.category,
            "a timeout and a merge conflict must never collapse into one category"
        );
    }

    #[test]
    fn an_error_between_two_categories_is_left_unknown() {
        let references = references();
        let ambiguous = blend(
            direction(ErrorCategory::Build),
            direction(ErrorCategory::TestFailure),
            0.5,
        );

        let assigned = categorize(&ambiguous, &references, &CategorizationPolicy::default());
        assert_eq!(
            assigned.category,
            ErrorCategory::Unknown,
            "an error equally like two categories must not be guessed at"
        );
    }

    #[test]
    fn a_distant_error_is_left_unknown() {
        let references = references();
        let mut stranger = vec![0.0; DIMENSION];
        stranger[DIMENSION - 1] = 1.0;

        let assigned = categorize(&stranger, &references, &CategorizationPolicy::default());
        assert_eq!(
            assigned.category,
            ErrorCategory::Unknown,
            "an error resembling nothing must not be forced into the nearest category"
        );
    }

    #[test]
    fn empty_input_is_unknown() {
        let references = references();
        let policy = CategorizationPolicy::default();
        assert_eq!(
            categorize(&[], &references, &policy).category,
            ErrorCategory::Unknown
        );
        assert_eq!(
            categorize(&basis(0), &ReferenceEmbeddings::default(), &policy).category,
            ErrorCategory::Unknown
        );
    }

    #[test]
    fn mismatched_dimensions_do_not_categorize() {
        let references = references();
        let assigned = categorize(
            &[1.0, 0.0, 0.0],
            &references,
            &CategorizationPolicy::default(),
        );
        assert_eq!(
            assigned.category,
            ErrorCategory::Unknown,
            "a vector from a different model must not be scored against these references"
        );
    }

    #[test]
    fn the_margin_bar_can_be_relaxed_deliberately() {
        let references = references();
        let ambiguous = blend(
            direction(ErrorCategory::Build),
            direction(ErrorCategory::TestFailure),
            0.5,
        );

        let relaxed = categorize(
            &ambiguous,
            &references,
            &CategorizationPolicy {
                minimum_similarity: 0.4,
                minimum_margin: 0.0,
            },
        );
        assert_ne!(
            relaxed.category,
            ErrorCategory::Unknown,
            "the margin is what rejects the tie, so removing it must admit one"
        );
    }

    #[test]
    fn categorizing_the_same_error_twice_gives_the_same_answer() {
        let references = references();
        let embedding = blend(direction(ErrorCategory::Permission), 0, 0.1);
        let policy = CategorizationPolicy::default();
        assert_eq!(
            categorize(&embedding, &references, &policy),
            categorize(&embedding, &references, &policy),
            "categorisation must be deterministic so a repeat pass learns nothing new"
        );
    }

    #[test]
    fn category_names_round_trip() {
        for category in ErrorCategory::CLASSIFIED
            .into_iter()
            .chain(std::iter::once(ErrorCategory::Unknown))
        {
            assert_eq!(
                ErrorCategory::parse(category.as_str()),
                Some(category),
                "category identifiers are persisted and must parse back"
            );
        }
        assert_eq!(ErrorCategory::parse("not-a-category"), None);
    }

    #[test]
    fn every_classified_category_defines_exemplars() {
        for category in ErrorCategory::CLASSIFIED {
            assert!(
                !category.exemplars().is_empty(),
                "{category} has no exemplars, so nothing could ever be filed under it"
            );
        }
        assert!(
            ErrorCategory::Unknown.exemplars().is_empty(),
            "unknown is the absence of a match, never a target"
        );
    }
}
