//! A stand-in claude, and a workspace whose completions run on it, for the
//! stages that leave the model to the agent.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::json;
use sqlx::PgPool;
use tempfile::TempDir;
use uuid::Uuid;
use zone_context::embeddings::providers::PROVIDER_SELF_HOSTED;
use zone_core::llm::AgentKind;

use crate::config::{Config, ModelBackend};
use crate::db::{organizations, workspaces};
use crate::state::AppState;

/// An Ollama model no agent knows. As the workspace's Fast model it leaves
/// every stage to the agent's choice, whatever `OLLAMA_MODEL_FAST` says.
pub const UNKNOWN_TO_AGENTS: &str = "llama3.2:3b";

pub const MODEL_FLAG: &str = "--model";

const ARGUMENTS: &str = "arguments";
const REPLY: &str = "reply.jsonl";
const RUN_SEPARATOR: char = '\u{1e}';
const ATTEMPTS: usize = 100;
const PAUSE: Duration = Duration::from_millis(50);

/// A claude that records the arguments of every run and answers every prompt
/// with one reply.
pub struct StandIn {
    pub executable: PathBuf,
    directory: TempDir,
}

impl StandIn {
    pub fn answering(answer: &str) -> Self {
        let directory = TempDir::new().expect("a directory for the stand-in claude");
        let reply = [
            json!({"type": "assistant", "message": {"content": [{"type": "text", "text": answer}]}}),
            json!({"type": "result", "subtype": "success", "is_error": false}),
        ]
        .map(|event| event.to_string())
        .join("\n");
        std::fs::write(directory.path().join(REPLY), format!("{reply}\n"))
            .expect("the stand-in's reply");

        let executable = directory.path().join("claude");
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\n[ \"$#\" -gt 0 ] || exit 0\ncat > /dev/null\nprintf '%s\\0' \"$@\" >> '{arguments}'\nprintf '\\036' >> '{arguments}'\ncat '{reply}'\n",
                arguments = directory.path().join(ARGUMENTS).display(),
                reply = directory.path().join(REPLY).display(),
            ),
        )
        .expect("the stand-in claude");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .expect("the stand-in claude to be executable");
        wait_until_executable(&executable);
        Self {
            executable,
            directory,
        }
    }

    /// The arguments of each run, in the order they ran.
    pub fn runs(&self) -> Vec<Vec<String>> {
        std::fs::read(self.directory.path().join(ARGUMENTS))
            .map(|recorded| {
                String::from_utf8_lossy(&recorded)
                    .split_terminator(RUN_SEPARATOR)
                    .map(|run| run.split_terminator('\0').map(str::to_string).collect())
                    .collect()
            })
            .unwrap_or_default()
    }
}

pub struct AgentWorkspace {
    pub state: AppState,
    pub pool: PgPool,
    pub workspace: Uuid,
    organization: Uuid,
    claude: StandIn,
}

impl AgentWorkspace {
    /// A workspace on the instance's claude, which answers every prompt with
    /// `answer`.
    pub async fn answering(answer: &str) -> Self {
        let pool = PgPool::connect(
            &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
        )
        .await
        .expect("the test database");
        let suffix = Uuid::new_v4().simple().to_string();
        let organization = organizations::create_organization(&pool, "Agent stages", &suffix, None)
            .await
            .expect("an organization");
        let workspace =
            workspaces::create_workspace(&pool, organization.id, "Agent stages", &suffix, None)
                .await
                .expect("a workspace");
        sqlx::query(
            "INSERT INTO organization_ai_settings (organization_id, provider, model_fast) \
             VALUES ($1, $2, $3)",
        )
        .bind(organization.id)
        .bind(PROVIDER_SELF_HOSTED)
        .bind(UNKNOWN_TO_AGENTS)
        .execute(&pool)
        .await
        .expect("the organization's AI settings");

        let claude = StandIn::answering(answer);
        let state = AppState::new(
            Config {
                model_backend: ModelBackend::Cli {
                    agent: AgentKind::Claude,
                    executable: Some(claude.executable.clone()),
                },
                ..crate::state::test_config()
            },
            pool.clone(),
            None,
        );
        Self {
            state,
            pool,
            workspace: workspace.id,
            organization: organization.id,
            claude,
        }
    }

    /// Whether claude ran once and was left to choose its own model.
    pub fn chose_its_own_model(&self) -> bool {
        match self.claude.runs().as_slice() {
            [arguments] => !arguments.iter().any(|argument| argument == MODEL_FLAG),
            _ => false,
        }
    }

    pub async fn remove(&self) {
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(self.organization)
            .execute(&self.pool)
            .await
            .expect("the organization to be removed");
    }
}

/// Runs the stand-in once, before any deadline starts: Linux refuses to exec a
/// file a sibling test's fork still holds open for writing, and macOS
/// assesses a new executable on its first run. Run without arguments, it
/// exits at once.
fn wait_until_executable(path: &Path) {
    for _ in 0..ATTEMPTS {
        match std::process::Command::new(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Err(error) if error.raw_os_error() == Some(nix::errno::Errno::ETXTBSY as i32) => {
                std::thread::sleep(PAUSE);
            }
            _ => return,
        }
    }
}
