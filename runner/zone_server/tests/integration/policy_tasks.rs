//! Task runs, and the completions Zone runs on its own, under their workspace's agent policy: a
//! run on a model or agent the policy denies fails with the denial and is audited, an auto
//! project starts nothing on a denied route, and a title falls back to a model the policy admits
//! or is skipped, audited, without a completion being asked for.

use std::sync::Arc;
use std::time::Duration;

use abnegate_vcs::PullRequestService;
use dashmap::DashMap;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};
use zone_enterprise::policy::{self, Denial};
use zone_server::config::AutoProjectConfig;
use zone_server::db::auto_projects::{self, Kind, PlannedTask, ProjectPlan};
use zone_server::db::tasks::{self, Origin};
use zone_server::db::{licenses, migrations};
use zone_server::services::task_progress::{self, ProgressMessage};
use zone_server::state::AppState;
use zone_server::workers::auto_project::{self, Services};
use zone_server::workers::{task, titles};

use crate::common::sso::{Audit, active, keyring, lapsed};
use crate::common::{self, Database};

const PREFIX: &str = "policy_tasks";
const FAILED: &str = "failed";
const COMPLETED: &str = "completed";
const TURN_DENIED: &str = "agent_policy.turn_denied";
const COMPLETIONS: &str = "/chat/completions";
const CLAUDE: &str = "claude_code";
const SELF_HOSTED: &str = "self_hosted";
const FAST: &str = "llama3.2:3b";
const REASONING: &str = "qwen3:32b";
const REQUESTED: &str = "gpt-4";
const TITLE: &str = "Ship the shopping cart";
const SUMMARY: &str = "Finished the work.";
const DRIVES_SETTLE_WITHIN: Duration = Duration::from_secs(30);

struct Fixture {
    database: Database,
    state: AppState,
    provider: MockServer,
    workspace: Uuid,
    organization: Uuid,
    user: Uuid,
}

impl Fixture {
    /// A workspace member in an organization whose AI settings name `provider`, with Fast and
    /// Reasoning models saved, on an instance whose model host answers every completion.
    async fn new(provider: &str) -> Self {
        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool).await.unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(COMPLETIONS))
            .respond_with(answer)
            .mount(&server)
            .await;
        let config = zone_server::config::Config {
            litellm_host: server.uri(),
            ollama_host: server.uri(),
            ..common::test_config()
        };
        let state = common::create_test_state(config, database.pool.clone());
        state.install_keyring(keyring());
        let (organization, workspace, user) = common::setup_workspace_member(&database.pool).await;
        sqlx::query(
            "INSERT INTO organization_ai_settings (organization_id, provider, model_fast, model_reasoning) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(organization)
        .bind(provider)
        .bind(FAST)
        .bind(REASONING)
        .execute(&database.pool)
        .await
        .unwrap();
        Self {
            database,
            state,
            provider: server,
            workspace,
            organization,
            user,
        }
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    async fn license(&self, key: &str) {
        licenses::replace(self.pool(), key).await.unwrap();
        self.state.licenses().load(self.pool()).await.unwrap();
    }

    async fn restrict(&self, rules: Value) {
        sqlx::query("INSERT INTO organization_policies (organization_id, rules) VALUES ($1, $2)")
            .bind(self.organization)
            .bind(rules)
            .execute(self.pool())
            .await
            .unwrap();
    }

    /// An agentic task asking for `model`, with its title still to be generated.
    async fn task(&self, model: &str) -> Uuid {
        let task = tasks::create_task_as(
            self.pool(),
            self.workspace,
            &[],
            "Cart",
            "Let shoppers buy several items at once",
            None,
            None,
            true,
            None,
            Some(self.user),
        )
        .await
        .unwrap();
        sqlx::query("UPDATE tasks SET model_name = $2, title_pending = TRUE WHERE id = $1")
            .bind(task.id)
            .bind(model)
            .execute(self.pool())
            .await
            .unwrap();
        task.id
    }

    /// A run of `task`, started by the fixture's member, executed through to its end, and the
    /// progress it announced on the way.
    async fn run(&self, task: Uuid) -> (tasks::TaskRunRow, Vec<ProgressMessage>) {
        let run = tasks::create_task_run_as(self.pool(), task, Some(self.user))
            .await
            .unwrap();
        let mut progress = task_progress::progress().subscribe(run.id);
        task::execute_task_run(&self.state, run.id, task).await;
        let mut announced = Vec::new();
        while let Ok(message) = progress.try_recv() {
            announced.push(message);
        }
        let run = tasks::get_task_run(self.pool(), run.id)
            .await
            .unwrap()
            .expect("the run is readable");
        (run, announced)
    }

    async fn completions(&self) -> Vec<Value> {
        self.provider
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|request| request.url.path() == COMPLETIONS)
            .map(|request| {
                request
                    .body_json::<Value>()
                    .expect("a JSON completion request")
            })
            .collect()
    }

    async fn audits(&self) -> Vec<Audit> {
        sqlx::query_as(
            "SELECT actor_id, organization_id, resource_type, resource_id, old_values, new_values
             FROM audit_logs WHERE action = $1 ORDER BY created_at",
        )
        .bind(TURN_DENIED)
        .fetch_all(self.pool())
        .await
        .unwrap()
    }

    async fn title(&self, task: Uuid) -> (String, bool) {
        let row = tasks::get_task(self.pool(), task)
            .await
            .unwrap()
            .expect("the task is readable");
        (row.title, row.title_pending)
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

/// A completion as the request for it asks: streamed for a run's turn, whole for a title.
fn answer(request: &Request) -> ResponseTemplate {
    let body: Value = request.body_json().unwrap_or(Value::Null);
    if body["stream"] != json!(true) {
        return ResponseTemplate::new(200).set_body_json(json!({
            "id": "completion", "object": "chat.completion", "created": 0, "model": body["model"],
            "choices": [{"index": 0, "message": {"role": "assistant", "content": TITLE}, "finish_reason": "stop"}]
        }));
    }
    let content = json!({"id": "completion", "object": "chat.completion.chunk", "created": 0, "model": body["model"],
        "choices": [{"index": 0, "delta": {"content": SUMMARY}, "finish_reason": null}]});
    let end = json!({"id": "completion", "object": "chat.completion.chunk", "created": 0, "model": body["model"],
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]});
    ResponseTemplate::new(200)
        .insert_header("Content-Type", "text/event-stream")
        .set_body_string(format!(
            "data: {content}\n\ndata: {end}\n\ndata: [DONE]\n\n"
        ))
}

fn failed_with(announced: &[ProgressMessage], message: &str) -> bool {
    announced
        .iter()
        .any(|announced| matches!(announced, ProgressMessage::Failed { error } if error == message))
}

#[tokio::test]
async fn a_run_on_a_model_the_policy_denies_fails_with_the_denial_and_is_audited() {
    let fixture = Fixture::new(SELF_HOSTED).await;
    fixture.license(&active()).await;
    fixture.restrict(json!({"models": ["llama*"]})).await;
    let task = fixture.task(REQUESTED).await;

    let (run, announced) = fixture.run(task).await;
    let completions = fixture.completions().await;
    let audits = fixture.audits().await;
    let user = fixture.user;
    let organization = fixture.organization;
    fixture.cleanup().await;

    let message = Denial::Model.to_string();
    assert_eq!(run.status, FAILED);
    assert_eq!(run.error_message.as_deref(), Some(message.as_str()));
    assert!(
        failed_with(&announced, &message),
        "the run's watchers were not told why it failed: {announced:?}"
    );
    assert!(
        completions.is_empty(),
        "a denied model was asked to run the task: {completions:?}"
    );
    let [audit] = audits.as_slice() else {
        panic!("the denial was audited {} times", audits.len());
    };
    assert_eq!(audit.resource_type, "task");
    assert_eq!(audit.resource_id, Some(task));
    assert_eq!(audit.actor_id, Some(user));
    assert_eq!(audit.organization_id, Some(organization));
    assert_eq!(
        audit.new_values,
        Some(json!({"code": policy::MODEL_DENIED, "detail": REQUESTED}))
    );
}

#[tokio::test]
async fn a_run_routed_to_an_agent_the_policy_denies_fails_with_the_denial_and_is_audited() {
    let fixture = Fixture::new(CLAUDE).await;
    fixture.license(&active()).await;
    fixture.restrict(json!({"agents": []})).await;
    let task = fixture.task("sonnet").await;

    let (run, announced) = fixture.run(task).await;
    let audits = fixture.audits().await;
    fixture.cleanup().await;

    let message = Denial::Agent(policy::Agent::Claude).to_string();
    assert_eq!(run.status, FAILED);
    assert_eq!(run.error_message.as_deref(), Some(message.as_str()));
    assert!(failed_with(&announced, &message), "{announced:?}");
    let [audit] = audits.as_slice() else {
        panic!("the denial was audited {} times", audits.len());
    };
    assert_eq!(
        audit.new_values,
        Some(json!({"code": policy::AGENT_DENIED, "detail": "sonnet"}))
    );
}

#[tokio::test]
async fn a_lapsed_license_still_refuses_a_run() {
    let fixture = Fixture::new(SELF_HOSTED).await;
    fixture.license(&lapsed()).await;
    fixture.restrict(json!({"models": ["llama*"]})).await;
    let task = fixture.task(REQUESTED).await;

    let (run, _) = fixture.run(task).await;
    let audits = fixture.audits().await;
    fixture.cleanup().await;

    assert_eq!(run.status, FAILED);
    assert_eq!(run.error_message, Some(Denial::Model.to_string()));
    assert_eq!(audits.len(), 1);
}

#[tokio::test]
async fn a_run_on_a_model_the_policy_admits_runs() {
    let fixture = Fixture::new(SELF_HOSTED).await;
    fixture.license(&active()).await;
    fixture.restrict(json!({"models": ["gpt-*"]})).await;
    let task = fixture.task(REQUESTED).await;

    let (run, _) = fixture.run(task).await;
    let completions = fixture.completions().await;
    let audits = fixture.audits().await;
    fixture.cleanup().await;

    assert_eq!(run.status, COMPLETED, "{:?}", run.error_message);
    assert!(
        completions
            .iter()
            .any(|completion| completion["model"] == REQUESTED),
        "the run never asked its model: {completions:?}"
    );
    assert!(audits.is_empty());
}

#[tokio::test]
async fn a_title_whose_fast_model_the_policy_denies_falls_back_to_one_it_admits() {
    let fixture = Fixture::new(SELF_HOSTED).await;
    fixture.license(&active()).await;
    fixture.restrict(json!({"models": ["qwen*"]})).await;
    let task = fixture.task(REQUESTED).await;

    let titled = titles::title_task(&fixture.state, task).await.unwrap();
    let title = fixture.title(task).await;
    let completions = fixture.completions().await;
    let audits = fixture.audits().await;
    fixture.cleanup().await;

    assert!(titled);
    assert_eq!(title, (TITLE.to_string(), false));
    let [completion] = completions.as_slice() else {
        panic!("the title was asked for {} times", completions.len());
    };
    assert_eq!(
        completion["model"], REASONING,
        "the title ran on a model the policy denies"
    );
    assert!(audits.is_empty());
}

#[tokio::test]
async fn a_title_whose_fast_model_and_fallbacks_the_policy_denies_is_skipped_and_audited() {
    let fixture = Fixture::new(SELF_HOSTED).await;
    fixture.license(&active()).await;
    fixture.restrict(json!({"models": ["mistral*"]})).await;
    let task = fixture.task(REQUESTED).await;

    titles::title_task(&fixture.state, task).await.unwrap();
    let (title, pending) = fixture.title(task).await;
    let completions = fixture.completions().await;
    let audits = fixture.audits().await;
    let user = fixture.user;
    fixture.cleanup().await;

    assert_ne!(title, TITLE);
    assert!(
        !pending,
        "a title that can't be generated keeps its placeholder"
    );
    assert!(
        completions.is_empty(),
        "a completion was asked of a model the policy denies: {completions:?}"
    );
    let [audit] = audits.as_slice() else {
        panic!("the skipped title was audited {} times", audits.len());
    };
    assert_eq!(audit.resource_type, "task");
    assert_eq!(audit.resource_id, Some(task));
    assert_eq!(audit.actor_id, Some(user));
    assert_eq!(
        audit.new_values,
        Some(json!({"code": policy::MODEL_DENIED, "detail": FAST}))
    );
}

#[tokio::test]
async fn an_auto_project_on_a_route_the_policy_denies_starts_nothing() {
    let fixture = Fixture::new(CLAUDE).await;
    fixture.license(&active()).await;
    fixture.restrict(json!({"agents": []})).await;
    let finalized = auto_projects::finalize(
        fixture.pool(),
        fixture.workspace,
        fixture.user,
        None,
        Origin::Member,
        &ProjectPlan {
            name: "Cart",
            description: None,
            repository: None,
            brief: &json!({}),
            tasks: &[PlannedTask {
                kind: Kind::Feature,
                title: "Add a cart".into(),
                description: "Ships a cart.".into(),
                acceptance_criteria: None,
                depends_on: Vec::new(),
                priority: None,
            }],
        },
    )
    .await
    .expect("an auto project");
    let task = finalized.task_ids[0];
    let services = Arc::new(Services::standing_in_for(
        PullRequestService::new().expect("GitHub's own origin"),
    ));
    let config = Arc::new(AutoProjectConfig::default());
    let driving = Arc::new(DashMap::new());

    auto_project::tick(&fixture.state, &services, &config, &driving).await;
    tokio::time::timeout(DRIVES_SETTLE_WITHIN, async {
        while !driving.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the claimed project is driven and released");
    let runs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_runs WHERE task_id = $1")
        .bind(task)
        .fetch_one(fixture.pool())
        .await
        .unwrap();
    let paused: Option<String> = sqlx::query_scalar(
        "SELECT auto_paused_reason FROM projects \
         WHERE id = (SELECT project_id FROM task_projects WHERE task_id = $1)",
    )
    .bind(task)
    .fetch_one(fixture.pool())
    .await
    .unwrap();
    let audits = fixture.audits().await;
    fixture.cleanup().await;

    let message = Denial::Agent(policy::Agent::Claude).to_string();
    assert_eq!(runs, 0, "a run was started on a route the policy denies");
    assert_eq!(paused.as_deref(), Some(message.as_str()));
    let [audit] = audits.as_slice() else {
        panic!("the refused run was audited {} times", audits.len());
    };
    assert_eq!(audit.resource_id, Some(task));
    assert_eq!(
        audit.new_values,
        Some(json!({"code": policy::AGENT_DENIED, "detail": null}))
    );
}
