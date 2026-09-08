//! What the learning loop reads out of, and writes back into, a run's artifacts.
//!
//! `task_runs.artifacts` is already where a run records what it did: item 16 writes
//! `attempts`, item 19 writes `evaluation`, and the pull request worker writes `pr`.
//! The quality score belongs alongside them, and the reception facts it needs — when
//! the change was merged, how many review rounds it took, who approved it — arrive the
//! same way.
//!
//! Every reader here is total: a missing key, a null, or a value of the wrong type
//! yields the neutral default rather than an error, because a run whose pull request was
//! never synced is missing evidence, not evidence of a bad change.

use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::Value;

use super::error_category::Categorization;
use super::quality::{ChangeReception, QualityScore};

pub const PULL_REQUEST_KEY: &str = "pr";
pub const REVIEW_KEY: &str = "review";
pub const QUALITY_KEY: &str = "quality";
pub const FAILURE_KEY: &str = "failure";

const MERGED_AT_KEY: &str = "merged_at";
const OPENED_AT_KEY: &str = "opened_at";
const MINUTES_TO_MERGE_KEY: &str = "minutes_to_merge";
const REVIEW_CYCLES_KEY: &str = "review_cycles";
const APPROVALS_KEY: &str = "approvals";
const COMMENTS_KEY: &str = "comments";
const BODY_KEY: &str = "body";

const MAXIMUM_COMMENTS: usize = 200;

fn section<'a>(artifacts: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    artifacts?.get(key).filter(|value| !value.is_null())
}

fn count(section: Option<&Value>, key: &str) -> u32 {
    section
        .and_then(|value| value.get(key))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_default()
}

fn timestamp(section: Option<&Value>, key: &str) -> Option<NaiveDateTime> {
    let text = section?.get(key)?.as_str()?;
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|moment| moment.with_timezone(&Utc).naive_utc())
}

/// How a change was received, as far as the artifacts record it.
///
/// `opened_at` is the fallback for when the pull request was opened, used only if the
/// artifacts do not carry their own opening timestamp.
pub fn reception(artifacts: Option<&Value>, opened_at: Option<NaiveDateTime>) -> ChangeReception {
    let pull_request = section(artifacts, PULL_REQUEST_KEY);

    let recorded_minutes = pull_request
        .and_then(|value| value.get(MINUTES_TO_MERGE_KEY))
        .and_then(Value::as_i64);

    let derived_minutes = timestamp(pull_request, MERGED_AT_KEY).and_then(|merged| {
        let opened = timestamp(pull_request, OPENED_AT_KEY).or(opened_at)?;
        Some((merged - opened).num_minutes())
    });

    ChangeReception {
        minutes_to_merge: recorded_minutes.or(derived_minutes),
        review_cycles: count(pull_request, REVIEW_CYCLES_KEY),
        approvals: count(pull_request, APPROVALS_KEY),
    }
}

/// The review comment bodies recorded against a run, if any were ever synced.
///
/// Accepts either a list of strings or a list of objects carrying a `body`, so whichever
/// shape the pull request sync settles on will read correctly.
pub fn review_comments(artifacts: Option<&Value>) -> Vec<String> {
    let Some(comments) = section(artifacts, REVIEW_KEY)
        .and_then(|review| review.get(COMMENTS_KEY))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    comments
        .iter()
        .filter_map(|comment| match comment {
            Value::String(body) => Some(body.clone()),
            Value::Object(_) => comment
                .get(BODY_KEY)
                .and_then(Value::as_str)
                .map(str::to_string),
            _ => None,
        })
        .map(|body| body.trim().to_string())
        .filter(|body| !body.is_empty())
        .take(MAXIMUM_COMMENTS)
        .collect()
}

/// The score as it is written back under `artifacts.quality`.
pub fn quality_artifact(score: QualityScore) -> Value {
    serde_json::json!({
        "value": score.value,
        "band": score.band,
        "components": {
            "merge_speed": score.merge_speed,
            "review_cycles": score.review_cycles,
            "approvals": score.approvals,
        },
    })
}

/// Similarities are single-precision, and widening them to JSON's double leaves a tail
/// of noise digits. Rounding keeps the recorded number readable and comparable.
fn rounded(value: f32) -> f64 {
    (f64::from(value) * 1_000.0).round() / 1_000.0
}

/// The failure kind as it is written back under `artifacts.failure`, so a run carries
/// its own diagnosis rather than only contributing to an aggregate.
pub fn failure_artifact(categorization: Categorization) -> Value {
    serde_json::json!({
        "category": categorization.category,
        "confidence": rounded(categorization.confidence),
        "margin": rounded(categorization.margin),
    })
}

/// Whether the stored artifact already matches, so an unchanged run is not rewritten.
pub fn is_current(artifacts: Option<&Value>, key: &str, artifact: &Value) -> bool {
    section(artifacts, key) == Some(artifact)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::learning::quality::{QualityBand, QualityWeights, score};
    use chrono::NaiveDate;
    use serde_json::json;

    fn moment(day: u32, hour: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 9, day)
            .unwrap()
            .and_hms_opt(hour, 0, 0)
            .unwrap()
    }

    #[test]
    fn a_run_without_artifacts_reads_as_no_evidence() {
        let empty = reception(None, None);
        assert_eq!(empty, ChangeReception::default());
        assert!(review_comments(None).is_empty());
    }

    #[test]
    fn an_unsynced_pull_request_reads_as_no_evidence() {
        let artifacts = json!({ "pr": { "pr_url": "https://example.test/pull/1" } });
        assert_eq!(
            reception(Some(&artifacts), Some(moment(1, 9))),
            ChangeReception::default(),
            "a pull request nobody has looked at yet must score neutrally, not badly"
        );
    }

    #[test]
    fn a_recorded_merge_duration_is_used_directly() {
        let artifacts = json!({
            "pr": { "minutes_to_merge": 45, "review_cycles": 1, "approvals": 2 }
        });

        assert_eq!(
            reception(Some(&artifacts), None),
            ChangeReception {
                minutes_to_merge: Some(45),
                review_cycles: 1,
                approvals: 2,
            }
        );
    }

    #[test]
    fn a_merge_duration_is_derived_from_timestamps_when_it_is_not_recorded() {
        let artifacts = json!({
            "pr": {
                "opened_at": "2026-09-04T09:00:00Z",
                "merged_at": "2026-09-04T11:30:00Z",
            }
        });

        assert_eq!(
            reception(Some(&artifacts), None).minutes_to_merge,
            Some(150)
        );
    }

    #[test]
    fn the_run_can_supply_the_opening_time_the_artifacts_lack() {
        let artifacts = json!({ "pr": { "merged_at": "2026-09-04T12:00:00Z" } });
        assert_eq!(
            reception(Some(&artifacts), Some(moment(4, 9))).minutes_to_merge,
            Some(180)
        );
    }

    #[test]
    fn a_merge_with_no_opening_time_anywhere_stays_unknown() {
        let artifacts = json!({ "pr": { "merged_at": "2026-09-04T12:00:00Z" } });
        assert_eq!(reception(Some(&artifacts), None).minutes_to_merge, None);
    }

    #[test]
    fn malformed_reception_values_fall_back_to_the_neutral_default() {
        let artifacts = json!({
            "pr": {
                "minutes_to_merge": "soon",
                "review_cycles": -4,
                "approvals": null,
                "merged_at": "not a timestamp",
            }
        });

        assert_eq!(
            reception(Some(&artifacts), Some(moment(4, 9))),
            ChangeReception::default(),
            "a malformed artifact must not be read as a score"
        );
    }

    #[test]
    fn review_comments_are_read_as_strings_or_as_objects() {
        let strings = json!({ "review": { "comments": ["needs a test", "  ", ""] } });
        assert_eq!(review_comments(Some(&strings)), vec!["needs a test"]);

        let objects = json!({
            "review": { "comments": [{ "body": "rename this" }, { "author": "nobody" }] }
        });
        assert_eq!(review_comments(Some(&objects)), vec!["rename this"]);
    }

    #[test]
    fn a_flood_of_comments_is_capped() {
        let comments: Vec<Value> = (0..500)
            .map(|index| json!(format!("comment {index}")))
            .collect();
        let artifacts = json!({ "review": { "comments": comments } });
        assert_eq!(review_comments(Some(&artifacts)).len(), MAXIMUM_COMMENTS);
    }

    #[test]
    fn the_quality_artifact_carries_the_score_and_its_components() {
        let scored = score(
            ChangeReception {
                minutes_to_merge: Some(0),
                review_cycles: 0,
                approvals: 2,
            },
            QualityWeights::default(),
        );

        let artifact = quality_artifact(scored);
        assert_eq!(artifact["band"], json!(QualityBand::Exemplary));
        assert_eq!(artifact["value"], json!(1.0));
        assert_eq!(artifact["components"]["merge_speed"], json!(1.0));
    }

    #[test]
    fn an_unchanged_artifact_is_recognised_as_already_written() {
        let scored = score(
            ChangeReception {
                minutes_to_merge: Some(120),
                review_cycles: 1,
                approvals: 1,
            },
            QualityWeights::default(),
        );
        let artifact = quality_artifact(scored);
        let artifacts = json!({ "quality": artifact });

        assert!(
            is_current(Some(&artifacts), QUALITY_KEY, &quality_artifact(scored)),
            "a repeat pass must recognise its own earlier write and skip it"
        );
        assert!(
            !is_current(None, QUALITY_KEY, &quality_artifact(scored)),
            "a run with no artifacts has nothing written yet"
        );
    }

    #[test]
    fn the_failure_artifact_carries_the_diagnosis_and_its_certainty() {
        let diagnosed = failure_artifact(Categorization {
            category: crate::workers::learning::error_category::ErrorCategory::Timeout,
            confidence: 0.71,
            margin: 0.12,
        });

        assert_eq!(diagnosed["category"], json!("timeout"));
        assert_eq!(
            diagnosed["confidence"],
            json!(0.71),
            "widening a single-precision similarity must not leave noise digits behind"
        );
        assert_eq!(diagnosed["margin"], json!(0.12));

        let unknown = failure_artifact(Categorization::unknown());
        assert_eq!(
            unknown["category"],
            json!("unknown"),
            "a failure nobody could categorise must say so rather than be omitted"
        );
    }

    #[test]
    fn writing_the_same_diagnosis_twice_produces_an_identical_artifact() {
        let categorization = Categorization {
            category: crate::workers::learning::error_category::ErrorCategory::Build,
            confidence: 0.638_291_4,
            margin: 0.071_23,
        };

        assert_eq!(
            failure_artifact(categorization),
            failure_artifact(categorization),
            "a repeat pass must not rewrite a run it has already diagnosed"
        );
    }
}
