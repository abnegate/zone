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
    source!("002_chat_agent.sql"),
    source!("003_chat_agent_sandbox.sql"),
    source!("004_ai_settings_model_image.sql"),
    source!("005_chat_title.sql"),
    source!("006_workspace_actions.sql"),
    source!("007_document_search.sql"),
    source!("008_embedding_dimension_1024.sql"),
    source!("009_chat_auto_approve.sql"),
    source!("010_message_search.sql"),
    source!("011_ann_serving.sql"),
    source!("012_ai_settings_model_video.sql"),
    source!("013_chat_character.sql"),
    source!("014_chat_context.sql"),
    source!("015_chat_reasoning_effort.sql"),
    source!("016_ai_settings_model_audio.sql"),
    source!("017_task_runs.sql"),
    source!("018_task_reconciliation.sql"),
    source!("019_task_admission_index.sql"),
    source!("020_task_heartbeat_index.sql"),
    source!("021_task_validation.sql"),
    source!("022_tenant_admin_permissions.sql"),
];
