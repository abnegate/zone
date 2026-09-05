//! First-stage retrieval sized for 100M-document serving.
//!
//! ANN and FTS run against a single index (quantized HNSW or GIN), take a
//! shortlist, then join document text and re-score with exact cosine. Joining
//! `content_chunks` before `ORDER BY vector` is what turns 100M rows into a
//! sequential scan.

use sqlx::PgConnection;

/// How many quantized ANN hits to pull before exact cosine re-rank.
pub fn ann_candidate_limit(limit: usize, extra_filters: bool) -> usize {
    let factor = if extra_filters { 12 } else { 8 };
    limit.saturating_mul(factor).clamp(32, 256)
}

/// How many GIN hits to pull before metadata filters.
pub fn keyword_candidate_limit(limit: usize, extra_filters: bool) -> usize {
    if extra_filters {
        limit.saturating_mul(4).clamp(24, 128)
    } else {
        limit.max(16)
    }
}

pub fn has_extra_filters(
    source_ids: bool,
    categories: bool,
    min_quality: bool,
    since: bool,
) -> bool {
    source_ids || categories || min_quality || since
}

/// Best-effort HNSW session knobs. Missing GUCs must not fail the pool.
pub async fn configure_ann_connection(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
    for (name, value) in [
        ("hnsw.iterative_scan", "relaxed_order"),
        ("hnsw.ef_search", "80"),
        ("hnsw.max_scan_tuples", "20000"),
    ] {
        if let Err(error) = sqlx::query("SELECT set_config($1, $2, false)")
            .bind(name)
            .bind(value)
            .execute(&mut *conn)
            .await
        {
            tracing::debug!(%error, setting = name, "ANN GUC skipped");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversamples_then_caps() {
        assert_eq!(ann_candidate_limit(10, false), 80);
        assert_eq!(ann_candidate_limit(10, true), 120);
        assert_eq!(ann_candidate_limit(1, false), 32);
        assert_eq!(ann_candidate_limit(100, true), 256);
        assert_eq!(keyword_candidate_limit(10, false), 16);
        assert_eq!(keyword_candidate_limit(10, true), 40);
    }
}
