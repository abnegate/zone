use std::sync::atomic::{AtomicBool, Ordering};

use chrono::NaiveDateTime;
use sqlx::PgPool;
use zone_server::workers::regression::{
    FixSubject, RegressionChecker, RegressionError, RegressionResult, Verdict,
};

/// A checker that finds every watched fix regressed, and deletes the first fix it is asked
/// about, so the alert for that one cannot be claimed.
pub struct Vanishing {
    pool: PgPool,
    deleted: AtomicBool,
}

impl Vanishing {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            deleted: AtomicBool::new(false),
        }
    }
}

#[async_trait::async_trait]
impl RegressionChecker for Vanishing {
    fn name(&self) -> &'static str {
        "vanishing"
    }

    async fn check(
        &self,
        subject: &FixSubject,
        now: NaiveDateTime,
    ) -> Result<RegressionResult, RegressionError> {
        if !self.deleted.swap(true, Ordering::SeqCst) {
            sqlx::query("DELETE FROM tasks WHERE id = $1")
                .bind(subject.task_id)
                .execute(&self.pool)
                .await
                .expect("the first fix is deleted");
        }
        Ok(RegressionResult::new(
            "vanishing",
            subject,
            Verdict::Regressed,
            0.9,
            Vec::new(),
            now,
        ))
    }
}
