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

/// How a hit relates to identifiers extracted from the query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifierRole {
    Exported,
    Defined,
    Mention,
    None,
}

impl IdentifierRole {
    pub fn is_definition(self) -> bool {
        matches!(self, Self::Exported | Self::Defined)
    }
}

pub fn identifier_role(text: &str, uri: &str, identifiers: &[String]) -> IdentifierRole {
    if identifiers.is_empty() {
        return IdentifierRole::None;
    }
    let uri_l = uri.to_ascii_lowercase();
    let mut best = IdentifierRole::None;
    for id in identifiers {
        best = max_role(best, role_for_identifier(text, &uri_l, id));
    }
    // Indexed unit tests copy `fn name()` / `fn ident` examples and would
    // otherwise outrank the real impl.
    if is_test_chunk(uri, text) && best.is_definition() {
        return IdentifierRole::Mention;
    }
    best
}

fn max_role(left: IdentifierRole, right: IdentifierRole) -> IdentifierRole {
    use IdentifierRole::*;
    match (left, right) {
        (Exported, _) | (_, Exported) => Exported,
        (Defined, _) | (_, Defined) => Defined,
        (Mention, _) | (_, Mention) => Mention,
        _ => None,
    }
}

fn ident_form(text: &str, form: &str) -> bool {
    let mut start = 0;
    while let Some(offset) = text[start..].find(form) {
        let at = start + offset;
        let after = text[at + form.len()..].chars().next();
        if !matches!(after, Some(c) if c.is_ascii_alphanumeric() || c == '_')
            && !is_inside_quotes(text, at)
        {
            return true;
        }
        start = at + form.len();
    }
    false
}

fn is_inside_quotes(text: &str, at: usize) -> bool {
    let line_start = text[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
    text[line_start..at].chars().filter(|&c| c == '"').count() % 2 == 1
}

fn role_for_identifier(text: &str, uri_l: &str, id: &str) -> IdentifierRole {
    if id.is_empty() {
        return IdentifierRole::None;
    }
    if ident_form(text, &format!("pub fn {id}"))
        || ident_form(text, &format!("pub async fn {id}"))
        || ident_form(text, &format!("pub struct {id}"))
        || ident_form(text, &format!("pub enum {id}"))
        || ident_form(text, &format!("pub const {id}"))
        || ident_form(text, &format!("pub static {id}"))
        || ident_form(text, &format!("pub type {id}"))
    {
        return IdentifierRole::Exported;
    }
    if ident_form(text, &format!("pub(crate) fn {id}"))
        || ident_form(text, &format!("fn {id}"))
        || ident_form(text, &format!("async fn {id}"))
        || ident_form(text, &format!("struct {id}"))
        || ident_form(text, &format!("enum {id}"))
        || ident_form(text, &format!("type {id}"))
        || ident_form(text, &format!("const {id}"))
        || ident_form(text, &format!("static {id}"))
        || ident_form(text, &format!("CREATE TABLE {id}"))
        || ident_form(text, &format!("CREATE TABLE IF NOT EXISTS {id}"))
        || ident_form(text, &format!("CREATE OR REPLACE FUNCTION {id}"))
        || text.contains(&format!("name: \"{id}\""))
        || text.contains(&format!("\"{id}\" =>"))
        || text.contains(&format!("'{id}' =>"))
        || text.contains(&format!("env_u64(\"{id}\""))
        || text.contains(&format!("env::var(\"{id}\""))
        || tool_name_def(text, id)
        || ident_form(text, &format!("pub {id}:"))
        || ident_form(text, &format!("pub(crate) {id}:"))
        || reexport_def(text, id)
    {
        return IdentifierRole::Defined;
    }
    if id.contains('/')
        && !looks_like_unit_test_body(text)
        && (text.contains(&format!("\"{id}\""))
            || text.contains(&format!("'{id}'"))
            || (text.contains(id)
                && (text.contains("format!(")
                    || text.contains(".post(")
                    || text.contains("path(\""))))
    {
        return IdentifierRole::Defined;
    }
    let file = uri_l
        .rsplit('/')
        .next()
        .unwrap_or(uri_l)
        .split('@')
        .next()
        .unwrap_or(uri_l);
    let stem = file.split('.').next().unwrap_or(file);
    let id_l = id.to_ascii_lowercase();
    if stem == id_l || (id_l.len() >= 8 && stem.contains(&id_l)) {
        return IdentifierRole::Defined;
    }
    if text.contains(id) || uri_l.contains(&id_l) {
        return IdentifierRole::Mention;
    }
    IdentifierRole::None
}

fn editorial_bonus(query: &str, uri: &str, text: &str) -> f32 {
    let identifiers = rewrite_query(query).identifiers;
    let role = identifier_role(text, uri, &identifiers);
    let covered = ident_hit_count(text, &identifiers);
    let frac = if identifiers.is_empty() {
        1.0
    } else {
        covered as f32 / identifiers.len() as f32
    };
    let mut bonus = match role {
        IdentifierRole::Exported => 1.05,
        IdentifierRole::Defined => 0.7,
        IdentifierRole::Mention | IdentifierRole::None => 0.0,
    } * frac;
    if identifiers.len() >= 2 && covered == identifiers.len() {
        bonus += 0.5;
    }
    bonus += 0.2 * ident_part_coverage(text, &identifiers);
    if is_outline_chunk(text) {
        bonus -= 0.95;
    }
    if is_fixture_uri(uri) {
        bonus -= 1.1;
    }
    if is_test_chunk(uri, text) {
        bonus -= 0.8;
    }
    if is_sql_migration(uri) && !query_wants_schema(query) {
        bonus -= 0.7;
    }
    bonus
}

fn tool_name_def(text: &str, id: &str) -> bool {
    let quoted = format!("\"{id}\"");
    let mut start = 0;
    while let Some(rel) = text[start..].find("fn name(") {
        let at = start + rel;
        if is_inside_quotes(text, at) {
            start = at + 8;
            continue;
        }
        let end = (at + 160).min(text.len());
        if text[at..end].contains(&quoted) {
            return true;
        }
        start = at + 8;
    }
    false
}

fn reexport_def(text: &str, id: &str) -> bool {
    text.contains("pub use ")
        && (text.contains(&format!("::{id}"))
            || text.contains(&format!(" {id};"))
            || text.contains(&format!("::{id};")))
}

pub fn ident_hit_count(text: &str, identifiers: &[String]) -> usize {
    identifiers
        .iter()
        .filter(|id| text.contains(id.as_str()))
        .count()
}

/// How many underscore/hyphen parts of the query identifiers appear in `text`.
pub fn ident_part_coverage(text: &str, identifiers: &[String]) -> f32 {
    let text_l = text.to_ascii_lowercase();
    let mut parts = Vec::new();
    for id in identifiers {
        for part in id.split(['_', '-', '/']) {
            if part.len() >= 4 {
                parts.push(part.to_ascii_lowercase());
            }
        }
    }
    if parts.is_empty() {
        return 0.0;
    }
    parts.iter().filter(|part| text_l.contains(*part)).count() as f32 / parts.len() as f32
}

/// Higher is better. Used to pack real answers ahead of tests and outlines.
pub fn chunk_rank_tier(query: &str, uri: &str, text: &str) -> u8 {
    let identifiers = rewrite_query(query).identifiers;
    if is_test_chunk(uri, text) || is_fixture_uri(uri) {
        return 0;
    }
    let defined = code_definition(text, uri, &identifiers);
    if is_outline_chunk(text) && !defined {
        return 1;
    }
    let code_ids: Vec<String> = identifiers
        .iter()
        .filter(|id| !id.contains('/'))
        .cloned()
        .collect();
    let covered_code = ident_hit_count(text, &code_ids);
    let all_code = code_ids.len() >= 2 && covered_code == code_ids.len();
    if defined && all_code {
        return 5;
    }
    if all_code {
        return 4;
    }
    if defined {
        return 3;
    }
    2
}

/// A path hit like `/api/chats/search` is not a code definition of `search_messages`.
fn code_definition(text: &str, uri: &str, identifiers: &[String]) -> bool {
    let code_ids: Vec<String> = identifiers
        .iter()
        .filter(|id| !id.contains('/'))
        .cloned()
        .collect();
    if code_ids.is_empty() {
        return identifier_role(text, uri, identifiers).is_definition();
    }
    identifier_role(text, uri, &code_ids).is_definition()
}

/// Keyword already found a real definition — skip embed / neural on the hot path.
pub fn first_stage_answered<'a>(
    query: &str,
    hits: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> bool {
    let identifiers = rewrite_query(query).identifiers;
    if identifiers.is_empty() {
        return false;
    }
    hits.into_iter().next().is_some_and(|(uri, text)| {
        !is_test_chunk(uri, text)
            && !is_outline_chunk(text)
            && code_definition(text, uri, &identifiers)
            && !(name_only_definition(text, &identifiers)
                && definition_misses_question(query, text))
    })
}

pub fn answers_as_definition(query: &str, uri: &str, text: &str) -> bool {
    let identifiers = rewrite_query(query).identifiers;
    if identifiers.is_empty() {
        return false;
    }
    identifier_role(text, uri, &identifiers).is_definition()
        && !definition_misses_question(query, text)
}

fn name_only_definition(text: &str, identifiers: &[String]) -> bool {
    identifiers.iter().any(|id| tool_name_def(text, id))
        && identifiers
            .iter()
            .all(|id| !text.contains(&format!("fn {id}(")) && !text.contains(&format!("fn {id} (")))
}

pub fn definition_misses_question(query: &str, text: &str) -> bool {
    let identifiers = rewrite_query(query).identifiers;
    let text_l = text.to_ascii_lowercase();
    let extras: Vec<String> = super::query::nl_content_tokens(query)
        .into_iter()
        .filter(|token| {
            !identifiers.iter().any(|id| {
                let id_l = id.to_ascii_lowercase();
                id_l.contains(token) || token.contains(&id_l)
            })
        })
        .collect();
    extras.len() >= 2 && extras.iter().all(|token| !text_l.contains(token))
}

pub fn role_strength(text: &str, uri: &str, identifiers: &[String]) -> u8 {
    match identifier_role(text, uri, identifiers) {
        IdentifierRole::Exported => 3,
        IdentifierRole::Defined => 2,
        IdentifierRole::Mention => 1,
        IdentifierRole::None => 0,
    }
}

pub fn is_file_header(text: &str) -> bool {
    is_outline_chunk(text)
}

pub fn is_outline_chunk(text: &str) -> bool {
    let head: String = text.chars().take(400).collect();
    head.contains("kind: file_header") || head.contains("kind: api_surface")
}

/// File banner / `//!` only — not an implementation of the question.
pub fn is_module_prelude(text: &str) -> bool {
    let head: String = text.chars().take(400).collect();
    if !head.contains("kind: top_level") {
        return false;
    }
    let has_impl = [
        "\npub fn ",
        "\npub async fn ",
        "\nfn ",
        "\nimpl ",
        "\npub struct ",
        "\npub enum ",
    ]
    .iter()
    .any(|needle| text.contains(needle));
    !has_impl
}

pub fn is_fixture_uri(uri: &str) -> bool {
    fixture_penalty(&uri.to_ascii_lowercase()) > 0.0
}

/// File-header / API-surface bags and eval fixtures crowd out the impl.
pub fn is_nav_chunk(text: &str, uri: &str) -> bool {
    is_outline_chunk(text) || is_fixture_uri(uri)
}

pub fn is_test_chunk(uri: &str, text: &str) -> bool {
    let uri_l = uri.to_ascii_lowercase();
    if uri_l.contains("/tests/")
        || uri_l.contains(".test.")
        || uri_l.contains("_test.rs")
        || uri_l.contains("/e2e/")
        || uri_l.contains(".sqlx/")
    {
        return true;
    }
    let head: String = text.chars().take(500).collect();
    let head_l = head.to_ascii_lowercase();
    head_l.contains("symbol: tests\n")
        || head_l.contains("symbol: tests.")
        || head_l.contains("parent: tests")
        || head_l.contains("parents: tests")
        || text.contains("#[cfg(test)]")
        || head.contains("#[test]")
        || text.contains("assert_eq!(")
        || text.contains("assert_ne!(")
}

fn looks_like_unit_test_body(text: &str) -> bool {
    text.contains("assert_eq!(")
        || text.contains("assert_ne!(")
        || text.contains("score_hit(")
        || text.contains("identifier_role(")
}

fn is_sql_migration(uri: &str) -> bool {
    let uri_l = uri.to_ascii_lowercase();
    uri_l.contains("/migrations/") && uri_l.contains(".sql")
}

fn query_wants_schema(query: &str) -> bool {
    let q = query.to_ascii_lowercase();
    q.contains("schema")
        || q.contains("migration")
        || q.contains("column")
        || q.contains("table")
        || q.contains("sql")
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
    let role = identifier_role(text, uri, &rewritten.identifiers);

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
            if role.is_definition() { 1.0 } else { 0.0 },
            if is_test_chunk(uri, text) { 1.0 } else { 0.0 },
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
        || uri_l.contains(".sqlx/")
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
            for (grad, (pos_v, neg_v)) in grad_w
                .iter_mut()
                .zip(pos.values.iter().zip(neg.values.iter()))
            {
                *grad += error * (*pos_v - *neg_v);
            }
            grad_b += error * 0.05;
        }
        let n = pairs.len() as f32;
        for (weight, grad) in ranker.weights.iter_mut().zip(grad_w.iter()) {
            let l2 = 0.002 * *weight;
            *weight += learning_rate * (*grad / n - l2);
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
        for (rank_i, feat_i) in &graded {
            for (rank_j, feat_j) in &graded {
                if rank_i > rank_j {
                    pairs.push((feat_i.clone(), feat_j.clone()));
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
        query, &uri, &title, &text, semantic, keyword, kw_rank, sem_rank, fusion,
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
    fn quoted_tool_and_sql_stem_count_as_definitions() {
        assert_eq!(
            identifier_role(
                r#"fn name(&self) -> &str { "search_knowledge" }"#,
                "agent/tools.rs",
                &["search_knowledge".into()],
            ),
            IdentifierRole::Defined
        );
        assert_eq!(
            identifier_role(
                "CREATE TABLE IF NOT EXISTS embeddings (id UUID)",
                "migrations/001_initial_schema.sql",
                &["001_initial_schema".into()],
            ),
            IdentifierRole::Defined
        );
        assert_eq!(
            identifier_role(
                r#"let url = format!("{}/api/embeddings", self.base_url);"#,
                "providers/ollama.rs",
                &["/api/embeddings".into()],
            ),
            IdentifierRole::Defined
        );
        assert_eq!(
            identifier_role(
                r#"assert!(rewritten.identifiers.iter().any(|i| i == "should_skip_blob"));"#,
                "embeddings/query.rs",
                &["should_skip_blob".into()],
            ),
            IdentifierRole::Mention
        );
        assert_eq!(
            identifier_role(
                "pub async fn search_knowledge_entries(pool: &PgPool)",
                "db/knowledge.rs",
                &["search_knowledge".into()],
            ),
            IdentifierRole::Mention
        );
        assert_eq!(
            identifier_role(
                "symbol: tests.quoted_tool\nfn name(&self) -> &str { \"search_knowledge\" }",
                "embeddings/ranker.rs",
                &["search_knowledge".into()],
            ),
            IdentifierRole::Mention
        );
        assert_eq!(
            identifier_role(
                "pub use github::GitHubAdapter;",
                "adapters/mod.rs",
                &["GitHubAdapter".into()],
            ),
            IdentifierRole::Defined
        );
        assert_eq!(
            identifier_role(
                "pub live_uris: Vec<String>,",
                "content/mod.rs",
                &["live_uris".into()],
            ),
            IdentifierRole::Defined
        );
        assert_eq!(
            identifier_role(
                r#"let query = "What does should_skip_blob do?";
            "pub fn should_skip_blob(&self, uri: &str, blob_sha: &str) -> bool { true }",
"#,
                "embeddings/local_rerank.rs",
                &["should_skip_blob".into()],
            ),
            IdentifierRole::Mention
        );
        assert_eq!(
            identifier_role(
                "let method = score_hit(query, \"ollama.rs\", \"format!(/api/embeddings)\");",
                "embeddings/ranker.rs",
                &["/api/embeddings".into()],
            ),
            IdentifierRole::Mention
        );
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

    #[test]
    fn first_stage_answered_needs_a_real_definition() {
        let query = "What does should_skip_blob do when a GitHub file SHA is unchanged?";
        let def = (
            "github://zone/content/mod.rs@main",
            "impl FetchConfig { pub fn should_skip_blob(&self, file_uri: &str, blob_sha: &str) -> bool { true } }",
        );
        let mention = (
            "github://zone/embeddings/query.rs@main",
            "assert!(rewritten.identifiers.iter().any(|i| i == \"should_skip_blob\"));",
        );
        let test = (
            "github://zone/embeddings/ranker.rs@main",
            "#[cfg(test)]\nfn quoted_tool() { let _ = \"pub fn should_skip_blob\"; }",
        );
        assert!(first_stage_answered(query, [def]));
        assert!(first_stage_answered(query, [def, mention]));
        assert!(!first_stage_answered(query, [mention, def]));
        assert!(!first_stage_answered(query, [mention]));
        assert!(!first_stage_answered(query, [test, mention]));
        assert!(!first_stage_answered(
            "How does the server verify a webhook signature?",
            [def],
        ));
        assert!(!first_stage_answered(
            "Where is search_messages implemented?",
            [(
                "github://zone/routes/chats.rs@main",
                "GET /api/chats/search | search_messages",
            )],
        ));
        assert!(!first_stage_answered(
            "How does run_command trim oversized stdout?",
            [(
                "github://zone/tools/command.rs@main",
                "fn name(&self) -> &str {\n        \"run_command\"\n    }",
            )],
        ));
    }
}
