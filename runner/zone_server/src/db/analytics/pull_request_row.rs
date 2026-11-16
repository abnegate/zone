use uuid::Uuid;

/// One task's pull request, with how it was received when its run recorded that.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct PullRequestRow {
    pub task_id: Uuid,
    pub source_id: Option<Uuid>,
    pub pr_status: Option<String>,
    pub minutes_to_merge: Option<f64>,
    pub review_cycles: Option<i64>,
    pub approvals: Option<i64>,
    pub quality_band: Option<String>,
}
