use serde::Serialize;

use super::Readiness;

#[derive(Serialize)]
pub struct ReadinessResponse {
    status: Readiness,
}

impl From<Readiness> for ReadinessResponse {
    fn from(status: Readiness) -> Self {
        Self { status }
    }
}
