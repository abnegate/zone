//! A review round is recorded whatever it ended in, including a reviewer model
//! that failed before it answered: an unrecorded failure leaves the round
//! where it was, and the next tick asks the same model again, forever.
use crate::common;

use sqlx::PgPool;
use uuid::Uuid;
use zone_server::db::auto_projects::{self, ReviewInsert, ReviewerKind, Verdict};
use zone_server::db::{projects, tasks};

const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const AUTHOR: &str = "author-model";
const CHECK_VIOLATION: &str = "23514";

struct Fixture {
    pool: PgPool,
    workspace: Uuid,
    user: Uuid,
    project: Uuid,
    task: Uuid,
}

impl Fixture {
    async fn new() -> Self {
        let pool = PgPool::connect(&common::context_database_url())
            .await
            .expect("the test database accepts a connection");
        let (_, workspace, user) = common::setup_test_data(&pool).await;
        let project = projects::create_project(&pool, "Reviews", None, Some(workspace))
            .await
            .expect("a project")
            .id;
        let task = tasks::create_task(
            &pool,
            workspace,
            &[project],
            "Ships a change",
            "Opens a pull request",
            None,
            None,
            true,
            None,
        )
        .await
        .expect("a task")
        .id;
        Self {
            pool,
            workspace,
            user,
            project,
            task,
        }
    }

    async fn record(&self, round: i32, reviewer: &str, verdict: Verdict) {
        auto_projects::record_review(
            &self.pool,
            ReviewInsert {
                task_id: self.task,
                run_id: None,
                round,
                head: HEAD,
                reviewer_kind: ReviewerKind::Model,
                reviewer,
                author_model: Some(AUTHOR),
                same_model: false,
                verdict,
                summary: "",
                findings: &[],
                addressed: &[],
                external_id: None,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("a {} round must be recorded: {error}", verdict.as_str()));
    }

    async fn distinct(&self) -> bool {
        auto_projects::distinct_review_on_head(&self.pool, self.task, HEAD)
            .await
            .expect("the distinct review is readable")
    }

    async fn reviewers(&self) -> Option<String> {
        auto_projects::project_tasks(&self.pool, self.project)
            .await
            .expect("the project's tasks are readable")
            .into_iter()
            .find(|task| task.task_id == self.task)
            .expect("the task is listed under its project")
            .reviewers
    }

    async fn discard(self) {
        common::discard(&self.pool, self.workspace, &[self.user]).await;
    }
}

#[tokio::test]
async fn every_verdict_the_server_records_is_one_the_table_accepts_and_reads_back() {
    let fixture = Fixture::new().await;

    for (round, verdict) in (1..).zip(Verdict::ALL) {
        fixture.record(round, "reviewer", verdict).await;
    }

    let read: Vec<Verdict> = auto_projects::reviews(&fixture.pool, fixture.task)
        .await
        .expect("the recorded rounds read back")
        .into_iter()
        .map(|row| row.verdict)
        .collect();
    assert_eq!(read, Verdict::ALL);
    fixture.discard().await;
}

#[tokio::test]
async fn a_verdict_no_variant_owns_is_refused_by_the_table() {
    let fixture = Fixture::new().await;

    let refused = sqlx::query(
        "INSERT INTO task_reviews (task_id, round, head, reviewer, verdict) \
         VALUES ($1, 1, $2, 'reviewer', 'approved')",
    )
    .bind(fixture.task)
    .bind(HEAD)
    .execute(&fixture.pool)
    .await
    .expect_err("049 widens the check; it must not remove it");
    let refused = refused.as_database_error().expect("a database error");
    assert_eq!(refused.code().as_deref(), Some(CHECK_VIOLATION));
    assert_eq!(refused.constraint(), Some("task_reviews_verdict_check"));
    fixture.discard().await;
}

#[tokio::test]
async fn a_failed_round_by_another_model_is_not_the_distinct_review_a_merge_needs() {
    let fixture = Fixture::new().await;

    fixture.record(1, "gemma3:27b", Verdict::Failed).await;
    assert!(
        !fixture.distinct().await,
        "a model that failed before answering reviewed nothing"
    );
    assert_eq!(
        fixture.reviewers().await,
        None,
        "the console names nobody as a reviewer for a failed round"
    );

    fixture.record(2, "tiny", Verdict::Unparseable).await;
    assert!(!fixture.distinct().await);

    fixture
        .record(3, "qwen3:32b", Verdict::RequestChanges)
        .await;
    assert!(fixture.distinct().await);
    assert_eq!(fixture.reviewers().await.as_deref(), Some("qwen3:32b"));
    fixture.discard().await;
}
