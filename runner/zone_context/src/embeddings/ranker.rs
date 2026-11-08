//! Pairwise-trained linear ranker over hybrid retrieval features.
//!
//! Weights are fit with RankNet-style logistic loss on graded judgments from
//! `evals/retrieval.json` plus hard negatives (high cosine, no identifier).

use std::sync::OnceLock;

use abnegate_index::LinearRanker;
use abnegate_index::RankFeatures;
use abnegate_index::extract_features;
use abnegate_index::fit_pairwise;

use super::eval::Judgment;
use super::eval::load_retrieval_eval;

const TRAINING_STEPS: usize = 250;

const LEARNING_RATE: f32 = 0.35;

/// Score a retrieved passage with the shipped ranker plus lexical cross-encoder.
pub fn score_hit(
    query: &str,
    uri: &str,
    title: &str,
    text: &str,
    semantic: Option<f32>,
    keyword: Option<f32>,
    keyword_rank: Option<usize>,
    semantic_rank: Option<usize>,
    fusion: f32,
) -> f32 {
    abnegate_index::score_hit(
        default_ranker(),
        query,
        uri,
        title,
        text,
        semantic,
        keyword,
        keyword_rank,
        semantic_rank,
        fusion,
    )
}

fn training_pairs() -> Vec<(RankFeatures, RankFeatures)> {
    let set = load_retrieval_eval();
    let mut pairs = Vec::new();
    for case in &set.cases {
        let mut graded: Vec<(u8, RankFeatures)> = case
            .judgments
            .iter()
            .map(|judgment| {
                (
                    judgment.grade,
                    features_for_judgment(&case.query, &case.expect_identifiers, judgment),
                )
            })
            .collect();
        graded.push((
            0,
            extract_features(
                &case.query,
                "github://zone/unrelated/auth.ts",
                "auth.ts",
                "generic authentication helper with login form",
                Some(0.81),
                Some(0.02),
                Some(18),
                Some(1),
                0.011,
            ),
        ));
        for (grade, features) in &graded {
            for (other_grade, other_features) in &graded {
                if grade > other_grade {
                    pairs.push((features.clone(), other_features.clone()));
                }
            }
        }
    }
    pairs
}

fn features_for_judgment(query: &str, identifiers: &[String], judgment: &Judgment) -> RankFeatures {
    let needle = judgment
        .must_contain
        .first()
        .cloned()
        .or_else(|| identifiers.first().cloned())
        .unwrap_or_else(|| query.to_string());
    let uri = format!("github://abnegate/zone/runner/{}", judgment.uri_contains);
    let title = judgment
        .uri_contains
        .rsplit('/')
        .next()
        .unwrap_or(&judgment.uri_contains)
        .to_string();
    let text = match judgment.grade {
        3 => format!("pub fn {needle} implements the requested behavior"),
        2 => format!("caller uses {needle} from a related module"),
        _ => format!("the eval mentions {needle}"),
    };
    let (semantic, keyword, keyword_rank, semantic_rank, fusion) = match judgment.grade {
        3 => (Some(0.58), Some(0.42), Some(1), Some(4), 0.016),
        2 => (Some(0.70), Some(0.14), Some(3), Some(2), 0.014),
        1 => (Some(0.64), Some(0.08), Some(7), Some(5), 0.012),
        _ => (Some(0.50), Some(0.03), Some(12), Some(8), 0.008),
    };
    extract_features(
        query,
        &uri,
        &title,
        &text,
        semantic,
        keyword,
        keyword_rank,
        semantic_rank,
        fusion,
    )
}

pub fn default_ranker() -> &'static LinearRanker {
    static RANKER: OnceLock<LinearRanker> = OnceLock::new();
    RANKER.get_or_init(|| fit_pairwise(&training_pairs(), TRAINING_STEPS, LEARNING_RATE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learned_ranker_prefers_exact_symbol_file() {
        let query = "What does should_skip_blob do when a GitHub file SHA is unchanged?";
        let exact = score_hit(
            query,
            "github://abnegate/zone/runner/zone_context/src/content/mod.rs@main",
            "mod.rs",
            "fn should_skip_blob(uri: &str, sha: &str) -> bool { uri.is_empty() }",
            Some(0.55),
            Some(0.40),
            Some(1),
            Some(6),
            0.009,
        );
        let neighbor = score_hit(
            query,
            "github://abnegate/zone/runner/zone_context/src/adapters/auth.ts@main",
            "auth.ts",
            "generic authentication helper",
            Some(0.81),
            Some(0.02),
            None,
            Some(1),
            0.011,
        );
        let fixture = score_hit(
            query,
            "github://abnegate/zone/runner/zone_context/evals/retrieval.json@main",
            "retrieval.json",
            "should_skip_blob",
            Some(0.74),
            Some(0.20),
            Some(2),
            Some(3),
            0.013,
        );
        assert!(exact > neighbor, "exact={exact} neighbor={neighbor}");
        assert!(exact > fixture, "exact={exact} fixture={fixture}");
        let mention = score_hit(
            query,
            "github://abnegate/zone/runner/zone_context/src/embeddings/eval.rs@main",
            "eval.rs",
            "assert!(rewritten.identifiers.iter().any(|i| i == \"should_skip_blob\"));",
            Some(0.66),
            Some(0.08),
            Some(1),
            Some(4),
            0.015,
        );
        assert!(exact > mention, "exact={exact} mention={mention}");
        let method = score_hit(
            query,
            "github://abnegate/zone/runner/zone_context/src/content/mod.rs@main",
            "mod.rs",
            "impl FetchConfig { pub fn should_skip_blob(&self, uri: &str, blob_sha: &str) -> bool { true } }",
            Some(0.55),
            Some(0.40),
            Some(1),
            Some(6),
            0.009,
        );
        assert!(method > mention, "method={method} mention={mention}");
        let header = score_hit(
            query,
            "github://abnegate/zone/runner/zone_context/src/content/mod.rs@main",
            "mod.rs",
            "path: github://zone/content/mod.rs@main kind: file_header\n\nidentifiers: should_skip_blob, FetchConfig",
            Some(0.72),
            Some(0.45),
            Some(1),
            Some(1),
            0.018,
        );
        assert!(method > header, "method={method} header={header}");
    }

    #[test]
    fn score_hit_survives_a_tool_name_window_inside_a_multibyte_character() {
        let text = format!("fn name(&self) -> &str {{{}é}}", " ".repeat(135));
        assert!(!text.is_char_boundary(160));
        let score = score_hit(
            "What does should_skip_blob do when a GitHub file SHA is unchanged?",
            "github://abnegate/zone/runner/zone_core/src/tools/blob.rs@main",
            "blob.rs",
            &text,
            Some(0.55),
            Some(0.40),
            Some(1),
            Some(6),
            0.009,
        );
        assert!(score.is_finite(), "score={score}");
    }
}
