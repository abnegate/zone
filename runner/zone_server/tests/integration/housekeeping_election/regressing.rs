use chrono::NaiveDateTime;
use zone_server::workers::regression::{
    FixSubject, RegressionChecker, RegressionError, RegressionResult, Verdict,
};

/// A checker that finds every watched fix regressed.
pub struct Regressing;

#[async_trait::async_trait]
impl RegressionChecker for Regressing {
    fn name(&self) -> &'static str {
        "regressing"
    }

    async fn check(
        &self,
        subject: &FixSubject,
        now: NaiveDateTime,
    ) -> Result<RegressionResult, RegressionError> {
        Ok(RegressionResult::new(
            "regressing",
            subject,
            Verdict::Regressed,
            0.9,
            Vec::new(),
            now,
        ))
    }
}
