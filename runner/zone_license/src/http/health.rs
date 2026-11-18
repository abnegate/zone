use axum::Json;
use serde::Serialize;

pub const OK: &str = "ok";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Health {
    pub status: &'static str,
}

/// Answers while the process serves. It doesn't reach the mail relay, so a
/// relay outage never takes the signer out of a load balancer.
pub async fn check() -> Json<Health> {
    Json(Health { status: OK })
}
