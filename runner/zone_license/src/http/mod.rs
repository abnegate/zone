mod context;
mod health;
mod issued;
mod outcome;
mod renew;
mod renew_request;
mod renewal_outcome;
mod renewed;
mod renewer;
mod server;
mod skipped;
mod webhook;

use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};

pub use context::Context;
pub use health::{Health, OK};
pub use issued::Issued;
pub use outcome::Outcome;
pub use renew_request::RenewRequest;
pub use renewal_outcome::RenewalOutcome;
pub use renewed::Renewed;
pub use renewer::Renewer;
pub use server::Server;
pub use skipped::Skipped;

pub const WEBHOOK_PATH: &str = "/webhooks/polar";
pub const RENEW_PATH: &str = "/licenses/renew";
pub const HEALTH_PATH: &str = "/health";
pub const RENEW_BODY_LIMIT: usize = 16 * 1024;

/// Axum's default body limit of 2 MB applies to the webhook, far above any
/// Polar event. A renewal carries one license key, so its limit is far lower.
pub fn router(context: Context) -> Router {
    Router::new()
        .route(WEBHOOK_PATH, post(webhook::receive))
        .route(
            RENEW_PATH,
            post(renew::respond).layer(DefaultBodyLimit::max(RENEW_BODY_LIMIT)),
        )
        .route(HEALTH_PATH, get(health::check))
        .with_state(Arc::new(context))
}

#[cfg(test)]
mod tests;
