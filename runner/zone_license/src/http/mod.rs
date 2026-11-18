mod context;
mod health;
mod issued;
mod outcome;
mod server;
mod skipped;
mod webhook;

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};

pub use context::Context;
pub use health::{Health, OK};
pub use issued::Issued;
pub use outcome::Outcome;
pub use server::Server;
pub use skipped::Skipped;

pub const WEBHOOK_PATH: &str = "/webhooks/polar";
pub const HEALTH_PATH: &str = "/health";

/// Axum's default body limit of 2 MB applies, far above any Polar event.
pub fn router(context: Context) -> Router {
    Router::new()
        .route(WEBHOOK_PATH, post(webhook::receive))
        .route(HEALTH_PATH, get(health::check))
        .with_state(Arc::new(context))
}

#[cfg(test)]
mod tests;
