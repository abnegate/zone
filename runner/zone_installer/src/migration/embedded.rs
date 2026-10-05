//! Migration SQL compiled into the binary from the server's migration directory.

/// Path from this crate's manifest directory to the SQL the server also migrates from.
/// The two paths share one set of files on purpose; `tests/embedded_migrations.rs`
/// fails the build if this list and that directory ever drift apart.
pub const MIGRATIONS_DIRECTORY: &str = "../zone_server/migrations";

#[derive(Clone, Copy, Debug)]
pub struct Source {
    pub file_name: &'static str,
    pub sql: &'static str,
}

macro_rules! source {
    ($file_name:literal) => {
        Source {
            file_name: $file_name,
            sql: include_str!(concat!("../../../zone_server/migrations/", $file_name)),
        }
    };
}

pub const EMBEDDED: &[Source] = &[
    source!("001_initial_schema.sql"),
    source!("002_chat_offline.sql"),
    source!("003_chat_context_tokens.sql"),
    source!("004_agent_login_usage.sql"),
    source!("005_agent_login_usage_validation.sql"),
    source!("006_runpod_api_key.sql"),
];
