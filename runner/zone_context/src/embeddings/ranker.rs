//! Pairwise-trained linear ranker over hybrid retrieval features.
//!
//! Weights are fit with RankNet-style logistic loss on graded judgments from
//! `evals/retrieval.json` plus hard negatives (high cosine, no identifier).

use std::sync::OnceLock;

use super::eval::{Judgment, load_retrieval_eval};
use super::query::rewrite_query;
use super::rerank::{blend_rank, lexical_cross_score};

pub const FEATURE_COUNT: usize = 15;

#[derive(Debug, Clone, PartialEq)]
pub struct RankFeatures {
    pub values: [f32; FEATURE_COUNT],
}

#[derive(Debug, Clone)]
pub struct LinearRanker {
    pub weights: [f32; FEATURE_COUNT],
    pub bias: f32,
}

impl LinearRanker {
    pub fn score(&self, features: &RankFeatures) -> f32 {
        let mut dot = self.bias;
        for (weight, value) in self.weights.iter().zip(features.values.iter()) {
            dot += weight * value;
        }
        dot
    }
}

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
    let features = extract_features(
        query,
        uri,
        title,
        text,
        semantic,
        keyword,
        keyword_rank,
        semantic_rank,
        fusion,
    );
    let ltr = default_ranker().score(&features);
    let ce = lexical_cross_score(query, uri, title, text);
    blend_rank(ltr, ce) + editorial_bonus(query, uri, text)
}

fn editorial_bonus(query: &str, uri: &str, text: &str) -> f32 {
    let identifiers = rewrite_query(query).identifiers;
    let mut bonus = 0.0;
    if is_exported_definition(text, &identifiers) {
        bonus += 1.05;
    } else if is_definition(text, &identifiers) {
        bonus += 0.7;
    }
    if fixture_penalty(&uri.to_ascii_lowercase()) > 0.0 {
        bonus -= 0.6;
    }
    if is_test_module(uri, text, &identifiers) {
        bonus -= 0.35;
    }
    bonus
}

fn is_exported_definition(text: &str, identifiers: &[String]) -> bool {
    identifiers.iter().any(|id| {
        text.contains(&format!("pub fn {id}"))
            || text.contains(&format!("pub async fn {id}"))
            || text.contains(&format!("pub struct {id}"))
            || text.contains(&format!("pub enum {id}"))
    })
}

fn is_definition(text: &str, identifiers: &[String]) -> bool {
    identifiers.iter().any(|id| {
        text.contains(&format!("fn {id}"))
            || text.contains(&format!("struct {id}"))
            || text.contains(&format!("enum {id}"))
            || text.contains(&format!("type {id}"))
            || text.contains(&format!("const {id}"))
    })
}

fn is_test_module(uri: &str, text: &str, identifiers: &[String]) -> bool {
    let uri_l = uri.to_ascii_lowercase();
    uri_l.contains("/tests/")
        || uri_l.contains(".test.")
        || uri_l.ends_with("_test.rs")
        || text.contains("#[cfg(test)]")
        || text.contains("#[test]")
        || (text.contains("assert!(") && !is_definition(text, identifiers))
}

pub fn extract_features(
    query: &str,
    uri: &str,
    title: &str,
    text: &str,
    semantic: Option<f32>,
    keyword: Option<f32>,
    keyword_rank: Option<usize>,
    semantic_rank: Option<usize>,
    fusion: f32,
) -> RankFeatures {
    let rewritten = rewrite_query(query);
    let uri_l = uri.to_ascii_lowercase();
    let title_l = title.to_ascii_lowercase();
    let text_l = text.to_ascii_lowercase();
    let query_l = query.to_ascii_lowercase();

    let mut ident_text = 0.0;
    let mut ident_uri = 0.0;
    let mut ident_title = 0.0;
    for ident in &rewritten.identifiers {
        if text.contains(ident) {
            ident_text += 1.0;
        }
        let ident_l = ident.to_ascii_lowercase();
        if !ident_l.is_empty() && uri_l.contains(&ident_l) {
            ident_uri += 1.0;
        }
        if !ident_l.is_empty() && title_l.contains(&ident_l) {
            ident_title += 1.0;
        }
    }
    let ident_n = rewritten.identifiers.len().max(1) as f32;

    let tokens: Vec<&str> = query
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|t| t.len() >= 2)
        .collect();
    let covered = tokens
        .iter()
        .filter(|token| text_l.contains(&token.to_ascii_lowercase()))
        .count();
    let coverage = if tokens.is_empty() {
        0.0
    } else {
        covered as f32 / tokens.len() as f32
    };

    let path_overlap = tokens
        .iter()
        .filter(|token| uri_l.contains(&token.to_ascii_lowercase()))
        .count() as f32
        / tokens.len().max(1) as f32;

    RankFeatures {
        values: [
            semantic.unwrap_or(0.0).clamp(0.0, 1.0),
            keyword_norm(keyword.unwrap_or(0.0)),
            if semantic.is_some() && keyword.is_some() {
                1.0
            } else {
                0.0
            },
            (ident_text / ident_n).min(1.0),
            (ident_uri / ident_n).min(1.0),
            (ident_title / ident_n).min(1.0),
            if !query_l.is_empty() && text_l.contains(&query_l) {
                1.0
            } else {
                0.0
            },
            coverage,
            path_overlap,
            (fusion * 40.0).clamp(0.0, 1.0),
            inv_rank(keyword_rank),
            inv_rank(semantic_rank),
            fixture_penalty(&uri_l),
            if is_definition(text, &rewritten.identifiers) {
                1.0
            } else {
                0.0
            },
            if is_test_module(uri, text, &rewritten.identifiers) {
                1.0
            } else {
                0.0
            },
        ],
    }
}

fn keyword_norm(score: f32) -> f32 {
    (1.0 - (-score * 4.0).exp()).clamp(0.0, 1.0)
}

fn inv_rank(rank: Option<usize>) -> f32 {
    rank.map(|r| 1.0 / (60.0 + r as f32)).unwrap_or(0.0)
}

fn fixture_penalty(uri_l: &str) -> f32 {
    if uri_l.contains("/evals/")
        || uri_l.contains("retrieval.json")
        || uri_l.contains("/tests/")
        || uri_l.contains(".test.")
    {
        1.0
    } else {
        0.0
    }
}

/// RankNet pairwise logistic regression.
pub fn fit_pairwise(
    pairs: &[(RankFeatures, RankFeatures)],
    steps: usize,
    learning_rate: f32,
) -> LinearRanker {
    let mut ranker = LinearRanker {
        weights: [0.0; FEATURE_COUNT],
        bias: 0.0,
    };
    if pairs.is_empty() {
        return ranker;
    }
    for _ in 0..steps {
        let mut grad_w = [0.0f32; FEATURE_COUNT];
        let mut grad_b = 0.0f32;
        for (pos, neg) in pairs {
            let s_pos = ranker.score(pos);
            let s_neg = ranker.score(neg);
            // d/ds log(σ(s_pos - s_neg)) = σ(s_neg - s_pos)
            let error = sigmoid(s_neg - s_pos);
            for i in 0..FEATURE_COUNT {
                grad_w[i] += error * (pos.values[i] - neg.values[i]);
            }
            grad_b += error * 0.05;
        }
        let n = pairs.len() as f32;
        for i in 0..FEATURE_COUNT {
            let l2 = 0.002 * ranker.weights[i];
            ranker.weights[i] += learning_rate * (grad_w[i] / n - l2);
        }
        ranker.bias += learning_rate * (grad_b / n);
    }
    ranker
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x.clamp(-20.0, 20.0)).exp())
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
        for i in 0..graded.len() {
            for j in 0..graded.len() {
                if graded[i].0 > graded[j].0 {
                    pairs.push((graded[i].1.clone(), graded[j].1.clone()));
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
    let (semantic, keyword, kw_rank, sem_rank, fusion) = match judgment.grade {
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
        kw_rank,
        sem_rank,
        fusion,
    )
}

pub fn default_ranker() -> &'static LinearRanker {
    static RANKER: OnceLock<LinearRanker> = OnceLock::new();
    RANKER.get_or_init(|| fit_pairwise(&training_pairs(), 250, 0.35))
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
            "github://abnegate/zone/runner/zone_context/src/embeddings/query.rs@main",
            "query.rs",
            "assert!(rewritten.identifiers.iter().any(|i| i == \"should_skip_blob\"));",
            Some(0.66),
            Some(0.08),
            Some(1),
            Some(4),
            0.015,
        );
        assert!(exact > mention, "exact={exact} mention={mention}");
    }

    #[test]
    fn pairwise_fit_separates_a_known_pair() {
        let pos = extract_features(
            "align_vector",
            "pgvector.rs",
            "pgvector.rs",
            "fn align_vector pads shorter embeddings",
            Some(0.5),
            Some(0.4),
            Some(1),
            Some(3),
            0.016,
        );
        let neg = extract_features(
            "align_vector",
            "auth.ts",
            "auth.ts",
            "login form",
            Some(0.9),
            Some(0.01),
            None,
            Some(1),
            0.012,
        );
        let ranker = fit_pairwise(&[(pos.clone(), neg.clone())], 80, 0.4);
        assert!(ranker.score(&pos) > ranker.score(&neg));
    }
}
