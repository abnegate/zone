//! Offline retrieval eval: rewrite coverage plus graded ranking metrics.

use serde::Deserialize;

use super::rewrite_query;

#[derive(Debug, Deserialize, Clone)]
pub struct Judgment {
    pub uri_contains: String,
    pub grade: u8,
    #[serde(default)]
    pub must_contain: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RetrievalCase {
    pub id: String,
    pub query: String,
    pub expect_identifiers: Vec<String>,
    #[serde(default)]
    pub expect_uri_contains: Vec<String>,
    #[serde(default)]
    pub judgments: Vec<Judgment>,
}

#[derive(Debug, Deserialize)]
pub struct RetrievalEvalSet {
    pub cases: Vec<RetrievalCase>,
}

pub fn load_retrieval_eval() -> RetrievalEvalSet {
    serde_json::from_str(include_str!("../../evals/retrieval.json"))
        .expect("evals/retrieval.json must parse")
}

/// Grade a hit against the case judgments. Unjudged documents are 0.
pub fn grade_hit(uri: &str, text: &str, judgments: &[Judgment]) -> u8 {
    let mut best = 0u8;
    for judgment in judgments {
        if !uri.contains(&judgment.uri_contains) {
            continue;
        }
        if !judgment.must_contain.is_empty()
            && !judgment
                .must_contain
                .iter()
                .any(|needle| text.contains(needle) || uri.contains(needle))
        {
            continue;
        }
        best = best.max(judgment.grade);
    }
    best
}

pub fn dcg(grades: &[u8]) -> f64 {
    grades
        .iter()
        .enumerate()
        .map(|(index, grade)| {
            let gain = (2u32.pow(u32::from(*grade)) - 1) as f64;
            gain / ((index as f64) + 2.0).log2()
        })
        .sum()
}

pub fn ndcg_at(grades: &[u8], ideal: &[u8], k: usize) -> f64 {
    let actual: Vec<u8> = grades.iter().copied().take(k).collect();
    let mut ideal: Vec<u8> = ideal.to_vec();
    ideal.sort_by(|a, b| b.cmp(a));
    ideal.truncate(k);
    while ideal.len() < actual.len() {
        ideal.push(0);
    }
    let denom = dcg(&ideal);
    if denom == 0.0 {
        0.0
    } else {
        (dcg(&actual) / denom).min(1.0)
    }
}

/// One grade per file so two chunks of the same URI cannot inflate nDCG.
pub fn unique_file_grades<'a>(
    hits: impl IntoIterator<Item = (&'a str, &'a str)>,
    judgments: &[Judgment],
) -> Vec<u8> {
    let mut seen = std::collections::HashSet::new();
    let mut grades = Vec::new();
    for (uri, text) in hits {
        let key = uri.split_once('@').map(|(head, _)| head).unwrap_or(uri);
        if !seen.insert(key.to_string()) {
            continue;
        }
        grades.push(grade_hit(uri, text, judgments));
    }
    grades
}

/// Mean average precision treating grade >= `relevant` as a hit.
pub fn average_precision(grades: &[u8], relevant: u8) -> f64 {
    let mut seen = 0.0;
    let mut acc = 0.0;
    for (index, grade) in grades.iter().enumerate() {
        if *grade >= relevant {
            seen += 1.0;
            acc += seen / (index as f64 + 1.0);
        }
    }
    let total = grades.iter().filter(|g| **g >= relevant).count();
    if total == 0 {
        0.0
    } else {
        acc / total as f64
    }
}

pub fn first_relevant_rank(grades: &[u8], relevant: u8) -> Option<usize> {
    grades
        .iter()
        .position(|grade| *grade >= relevant)
        .map(|index| index + 1)
}

/// Every case's expected identifiers must survive query rewrite.
pub fn assert_rewrite_coverage(set: &RetrievalEvalSet) {
    for case in &set.cases {
        let rewritten = rewrite_query(&case.query);
        for expected in &case.expect_identifiers {
            assert!(
                rewritten.identifiers.iter().any(|got| got == expected),
                "case {} expected identifier {expected:?} in {:?}",
                case.id,
                rewritten.identifiers
            );
        }
    }
}

pub fn assert_graded_coverage(set: &RetrievalEvalSet) {
    for case in &set.cases {
        assert!(
            case.judgments.iter().any(|j| j.grade >= 3),
            "case {} needs a grade-3 judgment",
            case.id
        );
        for needle in &case.expect_uri_contains {
            assert!(
                case.judgments.iter().any(|j| j.uri_contains.contains(needle) && j.grade >= 3),
                "case {} expect_uri_contains {needle} must have a grade-3 judgment",
                case.id
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retrieval_json_identifiers_survive_rewrite() {
        assert_rewrite_coverage(&load_retrieval_eval());
    }

    #[test]
    fn retrieval_json_has_graded_judgments() {
        assert_graded_coverage(&load_retrieval_eval());
    }

    #[test]
    fn ndcg_is_1_for_perfect_order() {
        let grades = [3, 2, 1, 0];
        assert!((ndcg_at(&grades, &grades, 4) - 1.0).abs() < 1e-9);
        let worse = [0, 1, 2, 3];
        assert!(ndcg_at(&worse, &grades, 4) < 0.8);
    }

    #[test]
    fn ndcg_does_not_exceed_one_when_two_chunks_share_a_grade() {
        let judgments = vec![Judgment {
            uri_contains: "content/mod.rs".into(),
            grade: 3,
            must_contain: vec!["should_skip_blob".into()],
        }];
        let grades = unique_file_grades(
            [
                (
                    "github://zone/content/mod.rs@a",
                    "fn should_skip_blob()",
                ),
                (
                    "github://zone/content/mod.rs@b",
                    "fn should_skip_blob() again",
                ),
            ],
            &judgments,
        );
        assert_eq!(grades, vec![3]);
        assert!((ndcg_at(&grades, &[3], 10) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn grade_hit_uses_uri_and_optional_text() {
        let judgments = vec![Judgment {
            uri_contains: "content/mod.rs".into(),
            grade: 3,
            must_contain: vec!["should_skip_blob".into()],
        }];
        assert_eq!(
            grade_hit(
                "github://zone/content/mod.rs",
                "fn should_skip_blob()",
                &judgments
            ),
            3
        );
        assert_eq!(
            grade_hit("github://zone/content/mod.rs", "unrelated", &judgments),
            0
        );
        assert_eq!(grade_hit("github://zone/other.rs", "should_skip_blob", &judgments), 0);
    }

    #[test]
    fn average_precision_rewards_early_relevant() {
        assert!((average_precision(&[3, 0, 2], 2) - (1.0 + 2.0 / 3.0) / 2.0).abs() < 1e-9);
    }
}
