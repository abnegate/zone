//! Zone Server - HTTP API server
//!
//! This is the main entry point for the Zone server, which provides:
//! - REST API for managing organizations, projects, tasks, chats, sources
//! - WebSocket endpoints for real-time task progress
//! - Authentication via JWT

// Allow dead code in this crate - many components are defined but not yet wired up
#![allow(dead_code)]

mod auth;
mod cache;
mod config;
mod db;
mod error;
mod routes;
mod state;
mod ws;

use std::net::SocketAddr;

use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::cache::Cache;
use crate::config::Config;
use crate::state::AppState;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "zone_server=debug,zone_core=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Zone server...");

    // Load config
    let config = Config::from_env().expect("Failed to load configuration");

    // Connect to database
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await
        .expect("Failed to connect to database");

    tracing::info!("Connected to database");

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("Failed to run migrations");

    tracing::info!("Migrations complete");

    // Connect to cache
    let cache = Cache::connect(&config.redis_url)
        .await
        .expect("Failed to connect to cache");

    tracing::info!("Connected to cache");

    // Build app state
    let state = AppState::new(config.clone(), db, Some(cache));

    // Build router
    let app = routes::create_router(state)
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    // Start server
    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .expect("Invalid address");

    tracing::info!("Listening on {}", addr);

    let listener = TcpListener::bind(addr).await.expect("Failed to bind");

    axum::serve(listener, app).await.expect("Server error");
}
