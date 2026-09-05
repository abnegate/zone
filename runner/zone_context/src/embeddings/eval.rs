//! Offline retrieval eval: identifier rewrite coverage for this repo.

use serde::Deserialize;

use super::rewrite_query;

#[derive(Debug, Deserialize)]
pub struct RetrievalCase {
    pub id: String,
    pub query: String,
    pub expect_identifiers: Vec<String>,
    pub expect_uri_contains: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RetrievalEvalSet {
    pub cases: Vec<RetrievalCase>,
}

pub fn load_retrieval_eval() -> RetrievalEvalSet {
    serde_json::from_str(include_str!("../../evals/retrieval.json"))
        .expect("evals/retrieval.json must parse")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retrieval_json_identifiers_survive_rewrite() {
        assert_rewrite_coverage(&load_retrieval_eval());
    }
}
