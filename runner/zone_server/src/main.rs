//! Zone Server - HTTP API server
//!
//! This is the main entry point for the Zone server, which provides:
//! - REST API for managing organizations, projects, tasks, chats, sources
//! - WebSocket endpoints for real-time task progress
//! - Authentication via JWT

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use zone_context::context::ContextService;
use zone_server::cache::Cache;
use zone_server::config::Config;
use zone_server::db::reminders::Sweep;
use zone_server::db::sealing::Options;
use zone_server::db::sealing::Sealer;
use zone_server::routes;
use zone_server::services::embedding::create_embedding_service;
use zone_server::services::embedding::default_embedding_model;
use zone_server::services::embedding::embedding_engine_from_env;
use zone_server::services::embedding::embedding_host;
use zone_server::services::login;
use zone_server::services::mail;
use zone_server::services::mail::AccountMail;
use zone_server::state::AppState;
use zone_server::state::default_adapter_registry;

#[tokio::main]
async fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mode = mode(&arguments).expect(USAGE);

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| DEFAULT_FILTER.into()))
        .with(tracing_subscriber::fmt::layer().with_writer(mode.log_writer()))
        .init();

    #[cfg(target_os = "linux")]
    forbid_dumping();

    match mode {
        Mode::Serve => {}
        Mode::MigrateOnly => {
            let url = std::env::var(DATABASE_URL).expect("DATABASE_URL is required");
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .connect(&url)
                .await
                .expect("Failed to connect to database");
            zone_server::db::migrations::run(&pool)
                .await
                .unwrap_or_else(|error| panic!("Failed to run migrations: {error}"));
            pool.close().await;
            tracing::info!("Migrations complete");
            return;
        }
        Mode::SealSecrets(options) => std::process::exit(seal_secrets(options).await),
    }

    tracing::info!("Starting Zone server...");

    abnegate_comfy::observe_requests(zone_server::metrics::record_comfyui);
    abnegate_search::observe_searches(zone_server::metrics::record_searxng);

    // Load config
    let config = Config::from_env().expect("Failed to load configuration");

    // Connect to database with ANN session knobs on every checkout.
    let db = zone_server::db::DbPool::connect(&config.database_url)
        .await
        .expect("Failed to connect to database")
        .inner()
        .clone();

    tracing::info!("Connected to database");

    zone_server::db::migrations::run(&db)
        .await
        .unwrap_or_else(|error| panic!("Failed to run migrations: {error}"));

    tracing::info!("Migrations complete");

    match zone_server::db::chat_subagents::abandon(&db).await {
        Ok(abandoned) => tracing::info!("Stopped {} subagents whose turns ended", abandoned.len()),
        Err(error) => tracing::warn!("Failed to stop abandoned subagents: {error}"),
    }

    // Connect to cache
    let cache = Cache::connect(&config.redis_url)
        .await
        .expect("Failed to connect to cache");

    tracing::info!("Connected to cache");

    // Initialize zone_context services
    // Note: These are optional and will only be initialized if we can get default settings
    // For now, we'll use config-based settings as a fallback
    let adapter_registry = {
        let registry = default_adapter_registry();
        tracing::info!(
            "Initialized adapter registry with {} adapters",
            registry.len()
        );
        Arc::new(registry)
    };

    // Try to initialize embedding service from config
    let embedding_service = {
        use zone_context::embeddings::providers::{EMBEDDING_ENGINE_OLLAMA, PROVIDER_SELF_HOSTED};
        use zone_server::db::ai_settings::EffectiveAiSettings;

        let engine = embedding_engine_from_env();
        let host = embedding_host(engine.as_deref(), &config.ollama_host, &config.litellm_host);
        let default_settings = EffectiveAiSettings {
            provider: PROVIDER_SELF_HOSTED.to_string(),
            litellm_host: Some(host.to_string()),
            litellm_key: Some(abnegate_secret::SecretValue::new(
                config.litellm_key.clone(),
            )),
            openai_api_key: None,
            openai_base_url: None,
            anthropic_api_key: None,
            anthropic_base_url: None,
            runpod_api_key: None,
            bedrock_region: None,
            bedrock_access_key: None,
            bedrock_secret_key: None,
            bedrock_use_iam_role: false,
            model_fast: None,
            model_reasoning: None,
            model_embedding: Some(default_embedding_model()),
            model_image: None,
            model_video: None,
            model_audio: None,
        };

        match create_embedding_service(&default_settings, engine.as_deref()) {
            Ok(service) => {
                tracing::info!(
                    "Initialized embedding service: engine={}, model={}, dimension={}",
                    engine.as_deref().unwrap_or(EMBEDDING_ENGINE_OLLAMA),
                    service.model(),
                    dimension_label(service.dimension())
                );
                Some(service)
            }
            Err(error) => {
                tracing::warn!("Failed to initialize embedding service: {}", error);
                tracing::warn!("Context features will be unavailable");
                None
            }
        }
    };

    let mail = match mail::Config::from_environment(mail::SenderPolicy::Default)
        .and_then(|config| AccountMail::from_config(&config))
    {
        Ok(mail) => {
            tracing::info!("Account mail configured");
            Some(Arc::new(mail))
        }
        Err(error) => {
            tracing::warn!(%error, "Account mail is not configured; verification and password reset emails will not be sent");
            None
        }
    };

    // Build app state
    let state = if let Some(embedding_service) = embedding_service {
        // Initialize context service
        let mut context_service = ContextService::new(
            db.clone(),
            adapter_registry.clone(),
            embedding_service.clone(),
        );
        if let Some(reranker) = zone_context::probe_cross_encoder(&config.ollama_host).await {
            tracing::info!("Initialized neural cross-encoder reranker");
            context_service.set_reranker(reranker);
        } else {
            tracing::info!("No neural reranker; using learned ranker + lexical cross-encoder");
        }
        let context_service = Arc::new(context_service);

        tracing::info!("Initialized context service");

        AppState::new_with_all_services(
            config.clone(),
            db,
            Some(cache),
            adapter_registry,
            embedding_service,
            context_service,
            mail,
        )
    } else {
        AppState::new_with_mail(config.clone(), db, Some(cache), mail)
    };

    // Start background workers
    zone_server::workers::housekeeping::spawn(state.clone());
    zone_server::workers::reminders::spawn(state.clone(), Sweep::All);
    if state.config().auto.enabled {
        zone_server::workers::auto_project::spawn(state.clone());
    }
    let recovery = zone_server::workers::task::spawn_recovery(state.clone());
    let training = tokio::spawn(zone_server::train_jobs::recover(
        state.config().comfyui.clone(),
        state.train_jobs().clone(),
    ));
    let callback = match state.config().agents.callback {
        Some(callback) => {
            let listener = login::callback::bind(&callback)
                .await
                .unwrap_or_else(|error| {
                    panic!(
                        "Could not listen for Claude sign-ins on {}: {error}. Free the port, or change or unset ZONE_AGENT_CALLBACK",
                        callback.bind
                    )
                });
            tracing::info!(address = %callback.bind, port = callback.port, "Listening for Claude sign-ins");
            if login::callback::exposed(&callback) {
                tracing::warn!(
                    address = %callback.bind,
                    "The Claude sign-in callback listens beyond loopback, where other machines can reach it. Set ZONE_AGENT_CALLBACK_BIND to 127.0.0.1 unless a port mapping needs more"
                );
            }
            Some(tokio::spawn(login::callback::serve(listener, callback)))
        }
        None => None,
    };

    // Every layer belongs to `create_router`, so what runs here is what the
    // router tests cover. A second CORS layer here answered every preflight
    // twice and under a different policy, and its wildcard header list could
    // not be combined with credentials at all.
    let app = routes::create_router(state);

    // Start server
    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .expect("Invalid address");

    tracing::info!("Listening on {}", addr);

    let listener = TcpListener::bind(addr).await.expect("Failed to bind");

    let result = axum::serve(listener, app).await;
    recovery.abort();
    let _ = recovery.await;
    training.abort();
    let _ = training.await;
    if let Some(callback) = callback {
        callback.abort();
        let _ = callback.await;
    }
    result.expect("Server error");
}

/// An embedding width for the startup log; a width of 0 is one the service
/// learns from its first embedding.
fn dimension_label(width: usize) -> String {
    match width {
        0 => DIMENSION_LEARNED_ON_FIRST_EMBED.to_string(),
        width => width.to_string(),
    }
}

const USAGE: &str =
    "Usage: zone-server [--migrate-only | --seal-secrets [--dry-run] --backup <path>]";
const DEFAULT_FILTER: &str =
    "zone_server=debug,abnegate_agent=debug,zone_core=debug,zone_context=debug,tower_http=debug";
const DATABASE_URL: &str = "DATABASE_URL";
const MIGRATE_ONLY: &str = "--migrate-only";
const SEAL_SECRETS: &str = "--seal-secrets";
const DRY_RUN: &str = "--dry-run";
const BACKUP: &str = "--backup";
const FLAG_PREFIX: &str = "--";
const UNKNOWN_ARGUMENTS: &str = "Unknown server arguments";
const SEALING_CONNECTIONS: u32 = 2;
const FAILURE: i32 = 1;
const SUCCESS: i32 = 0;
const DIMENSION_LEARNED_ON_FIRST_EMBED: &str = "learned on the first embed";

/// What the server was started to do.
#[derive(Debug, PartialEq)]
enum Mode {
    Serve,
    MigrateOnly,
    SealSecrets(Options),
}

impl Mode {
    /// Where the mode's logs go: stderr when stdout carries the sealing report.
    fn log_writer(&self) -> BoxMakeWriter {
        match self {
            Self::SealSecrets(_) => BoxMakeWriter::new(std::io::stderr),
            Self::Serve | Self::MigrateOnly => BoxMakeWriter::new(std::io::stdout),
        }
    }
}

fn mode(arguments: &[String]) -> Result<Mode, &'static str> {
    match arguments {
        [] => Ok(Mode::Serve),
        [argument] if argument == MIGRATE_ONLY => Ok(Mode::MigrateOnly),
        [first, rest @ ..] if first == SEAL_SECRETS => sealing(rest).map(Mode::SealSecrets),
        _ => Err(UNKNOWN_ARGUMENTS),
    }
}

fn sealing(arguments: &[String]) -> Result<Options, &'static str> {
    let mut dry_run = false;
    let mut backup: Option<PathBuf> = None;
    let mut remaining = arguments.iter();
    while let Some(argument) = remaining.next() {
        match argument.as_str() {
            DRY_RUN if !dry_run => dry_run = true,
            BACKUP if backup.is_none() => {
                let path = remaining
                    .next()
                    .filter(|path| !path.is_empty() && !path.starts_with(FLAG_PREFIX))
                    .ok_or(UNKNOWN_ARGUMENTS)?;
                backup = Some(PathBuf::from(path));
            }
            _ => return Err(UNKNOWN_ARGUMENTS),
        }
    }
    backup
        .map(|backup| Options::new(dry_run, backup))
        .ok_or(UNKNOWN_ARGUMENTS)
}

/// Seal stored credentials behind a verified backup, printing the report. The exit status
/// is non-zero when the run could not start or finish, or an envelope failed to reopen.
async fn seal_secrets(options: Options) -> i32 {
    let Ok(url) = std::env::var(DATABASE_URL) else {
        eprintln!("DATABASE_URL is required");
        return FAILURE;
    };
    let passphrase = match Config::encryption_key_from_env() {
        Ok(passphrase) => passphrase,
        Err(error) => {
            eprintln!("{error}");
            return FAILURE;
        }
    };
    let key = match zone_server::secret::derive(&passphrase) {
        Ok(key) => key,
        Err(error) => {
            eprintln!("ENCRYPTION_KEY does not derive a key: {error}");
            return FAILURE;
        }
    };
    let pool = match PgPoolOptions::new()
        .max_connections(SEALING_CONNECTIONS)
        .connect(&url)
        .await
    {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("Failed to connect to database: {error}");
            return FAILURE;
        }
    };

    let result = Sealer::new(pool.clone(), key, options).run().await;
    pool.close().await;
    match result {
        Ok(report) => {
            println!("{report}");
            if report.is_clean() {
                SUCCESS
            } else {
                eprintln!(
                    "Some envelopes did not reopen as their credential; those rows are unchanged"
                );
                FAILURE
            }
        }
        Err(error) => {
            eprintln!("{error}");
            FAILURE
        }
    }
}

/// Make the server non-dumpable. The files under its `/proc/<pid>` then
/// belong to root, so the agent CLIs it runs as its own user cannot read its
/// environment.
#[cfg(target_os = "linux")]
fn forbid_dumping() {
    if let Err(error) = nix::sys::prctl::set_dumpable(false) {
        tracing::warn!(
            %error,
            "Could not make the server non-dumpable; processes running as its user can read its environment"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use zone_server::db::sealing::Options;

    use super::DIMENSION_LEARNED_ON_FIRST_EMBED;
    use super::Mode;
    use super::dimension_label;
    use super::mode;

    fn arguments(arguments: &[&str]) -> Vec<String> {
        arguments
            .iter()
            .map(|argument| argument.to_string())
            .collect()
    }

    fn sealing(dry_run: bool) -> Mode {
        Mode::SealSecrets(Options::new(dry_run, PathBuf::from("/backups/zone.dump")))
    }

    #[test]
    fn an_unknown_embedding_width_is_logged_as_learned_on_the_first_embed() {
        assert_eq!(dimension_label(0), DIMENSION_LEARNED_ON_FIRST_EMBED);
        assert_eq!(dimension_label(1024), "1024");
    }

    #[test]
    fn migration_arguments_are_exact() {
        assert_eq!(mode(&[]), Ok(Mode::Serve));
        assert_eq!(mode(&arguments(&["--migrate-only"])), Ok(Mode::MigrateOnly));
        assert!(mode(&arguments(&["--migrate-only", "extra"])).is_err());
        assert!(mode(&arguments(&["--migrate"])).is_err());

        assert_eq!(
            mode(&arguments(&[
                "--seal-secrets",
                "--backup",
                "/backups/zone.dump"
            ])),
            Ok(sealing(false))
        );
        assert_eq!(
            mode(&arguments(&[
                "--seal-secrets",
                "--dry-run",
                "--backup",
                "/backups/zone.dump"
            ])),
            Ok(sealing(true))
        );
        assert_eq!(
            mode(&arguments(&[
                "--seal-secrets",
                "--backup",
                "/backups/zone.dump",
                "--dry-run"
            ])),
            Ok(sealing(true))
        );
    }

    #[test]
    fn migration_arguments_refuse_a_seal_without_exactly_one_backup() {
        for refused in [
            &["--seal-secrets"][..],
            &["--seal-secrets", "--dry-run"],
            &["--seal-secrets", "--backup"],
            &["--seal-secrets", "--backup", ""],
            &["--seal-secrets", "--backup", "--dry-run"],
            &[
                "--seal-secrets",
                "--dry-run",
                "--dry-run",
                "--backup",
                "a.dump",
            ],
            &["--seal-secrets", "--backup", "a.dump", "--backup", "b.dump"],
            &["--seal-secrets", "--seal-secrets", "--backup", "a.dump"],
            &["--seal-secrets", "--backup", "a.dump", "--force"],
            &["--seal-secrets", "--backup", "a.dump", "extra"],
            &["--dry-run", "--seal-secrets", "--backup", "a.dump"],
            &["--migrate-only", "--seal-secrets", "--backup", "a.dump"],
        ] {
            assert!(mode(&arguments(refused)).is_err(), "{refused:?}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_server_cannot_be_dumped() {
        super::forbid_dumping();
        assert_eq!(nix::sys::prctl::get_dumpable(), Ok(false));
    }
}
