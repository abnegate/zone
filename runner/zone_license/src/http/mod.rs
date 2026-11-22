mod admitted;
mod context;
mod get_only;
mod health;
mod issued;
mod outcome;
mod registrar;
mod renew;
mod renew_request;
mod renewal_outcome;
mod renewed;
mod renewer;
mod server;
mod skipped;
mod token;
mod token_outcome;
mod token_response;
mod webhook;

use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};

pub use context::Context;
pub use get_only::{ALLOWED, get_only};
pub use health::{Health, OK};
pub use issued::Issued;
pub use outcome::Outcome;
pub use registrar::Registrar;
pub use renew_request::RenewRequest;
pub use renewal_outcome::RenewalOutcome;
pub use renewed::Renewed;
pub use renewer::Renewer;
pub use server::Server;
pub use skipped::Skipped;
pub use token::{SCOPE_PARAMETER, SERVICE_PARAMETER};
pub use token_outcome::TokenOutcome;
pub use token_response::TokenResponse;

pub const WEBHOOK_PATH: &str = "/webhooks/polar";
pub const RENEW_PATH: &str = "/licenses/renew";
pub const HEALTH_PATH: &str = "/health";
pub const TOKEN_PATH: &str = "/token";
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
        .route(TOKEN_PATH, get_only(token::respond))
        .with_state(Arc::new(context))
}

#[cfg(test)]
mod tests;
