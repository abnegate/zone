//! Zone Server - HTTP API server
//!
//! This is the main entry point for the Zone server, which provides:
//! - REST API for managing organizations, projects, tasks, chats, sources
//! - WebSocket endpoints for real-time task progress
//! - Authentication via JWT

use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::{HeaderValue, Method};
use tokio::net::TcpListener;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use zone_context::adapters::{AdapterRegistry, FilesystemAdapter, GitHubAdapter, TextAdapter};
use zone_context::context::ContextService;
use zone_server::cache::Cache;
use zone_server::config::Config;
use zone_server::routes;
use zone_server::services::embedding::{create_embedding_service, embedding_engine_from_env};
use zone_server::state::AppState;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "zone_server=debug,zone_core=debug,zone_context=debug,tower_http=debug".into()
            }),
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

    // Initialize zone_context services
    // Note: These are optional and will only be initialized if we can get default settings
    // For now, we'll use config-based settings as a fallback
    let adapter_registry = {
        let mut registry = AdapterRegistry::new();
        registry.register(TextAdapter::new());
        registry.register(FilesystemAdapter::new());
        registry.register(GitHubAdapter::new());
        tracing::info!(
            "Initialized adapter registry with {} adapters",
            registry.len()
        );
        Arc::new(registry)
    };

    // Try to initialize embedding service from config
    // In production, this would fetch from organization AI settings
    let embedding_service = {
        use zone_context::embeddings::providers::PROVIDER_SELF_HOSTED;
        use zone_server::db::ai_settings::EffectiveAiSettings;

        let default_settings = EffectiveAiSettings {
            provider: PROVIDER_SELF_HOSTED.to_string(),
            litellm_host: Some(config.litellm_host.clone()),
            litellm_key: Some(config.litellm_key.clone()),
            openai_api_key: None,
            openai_base_url: None,
            anthropic_api_key: None,
            anthropic_base_url: None,
            bedrock_region: None,
            bedrock_access_key: None,
            bedrock_secret_key: None,
            bedrock_use_iam_role: false,
            model_fast: None,
            model_reasoning: None,
            model_embedding: Some("nomic-embed-text".to_string()),
            model_image: None,
        };

        let engine = embedding_engine_from_env();
        match create_embedding_service(&default_settings, engine.as_deref()) {
            Ok(service) => {
                tracing::info!(
                    "Initialized embedding service: engine={}, model={}, dimension={}",
                    engine.as_deref().unwrap_or("ollama"),
                    service.model(),
                    service.dimension()
                );
                Some(service)
            }
            Err(e) => {
                // SECURITY: Error messages are sanitized by ContextError Display impl
                // to prevent leaking API keys or other sensitive configuration data
                tracing::warn!("Failed to initialize embedding service: {}", e);
                tracing::warn!("Context features will be unavailable");
                None
            }
        }
    };

    // Try to initialize email service
    let email_service = match zone_server::services::email::EmailService::from_env() {
        Ok(service) => {
            tracing::info!("Email service initialized successfully");
            Some(Arc::new(service))
        }
        Err(e) => {
            tracing::warn!("Email service not configured: {}", e);
            tracing::warn!("Email features will be unavailable (verification, password reset)");
            None
        }
    };

    // Build app state
    let state = if let Some(embedding_service) = embedding_service {
        // Initialize context service
        let context_service = Arc::new(ContextService::new(
            db.clone(),
            adapter_registry.clone(),
            embedding_service.clone(),
        ));

        tracing::info!("Initialized context service");

        AppState::new_with_all_services(
            config.clone(),
            db,
            Some(cache),
            adapter_registry,
            embedding_service,
            context_service,
            email_service,
        )
    } else {
        // Fall back to basic state without context services
        AppState::new(config.clone(), db, Some(cache))
    };

    // Start background workers
    zone_server::workers::knowledge_refresh::start_refresh_worker(state.clone());
    zone_server::workers::reminders::spawn(state.clone());
    tracing::info!("Started knowledge refresh worker");

    // Configure CORS based on environment
    let cors_layer = if config.cors_origins.len() == 1 && config.cors_origins[0] == "*" {
        // SECURITY: When using wildcard origin (*), credentials MUST be disabled
        // to prevent CSRF attacks. This is enforced regardless of config.
        if config.cors_allow_credentials {
            tracing::warn!(
                "CORS: Wildcard origin (*) with credentials is a security risk - forcing credentials to false"
            );
        }
        tracing::info!("CORS: Allowing all origins (development mode)");
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
            .allow_credentials(false)
    } else {
        tracing::info!(
            "CORS: Restricting to specified origins: {:?}",
            config.cors_origins
        );
        let origins: Vec<HeaderValue> = config
            .cors_origins
            .iter()
            .filter_map(|origin| origin.parse().ok())
            .collect();

        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers(Any)
            .allow_credentials(config.cors_allow_credentials)
    };

    // Build router
    let app = routes::create_router(state)
        .layer(TraceLayer::new_for_http())
        .layer(cors_layer);

    // Start server
    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .expect("Invalid address");

    tracing::info!("Listening on {}", addr);

    let listener = TcpListener::bind(addr).await.expect("Failed to bind");

    axum::serve(listener, app).await.expect("Server error");
}
