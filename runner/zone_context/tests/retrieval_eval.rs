use zone_context::embeddings::eval::{assert_rewrite_coverage, load_retrieval_eval};

#[test]
fn retrieval_eval_rewrite_coverage() {
    assert_rewrite_coverage(&load_retrieval_eval());
}
