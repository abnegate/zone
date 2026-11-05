use crate::common;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
use zone_server::{
    auth::{AuthUser, jwt::Claims},
    db::{actions, tasks},
    routes::tasks::{CreateTaskRequest, UpdateTaskRequest, create, update},
    state::AppState,
    workers::titles,
};

const DESCRIPTION: &str = "Users on Safari are sent back to the login page every time they sign in\n\nIt started after the cookie change.";
const GENERATED: &str = "Fix Safari login redirect loop";
const CLASSIFIER: &str = "llama3.2:3b";

struct Fixture {
    pool: PgPool,
    state: AppState,
    provider: MockServer,
    workspace: Uuid,
    user: Uuid,
}

impl Fixture {
    async fn new(reply: ResponseTemplate) -> Self {
        let provider = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(reply)
            .mount(&provider)
            .await;
        let pool = common::create_test_pool().await;
        let (_, workspace, user) = common::setup_workspace_member(&pool).await;
        let mut config = common::test_config();
        config.litellm_host = provider.uri();
        config.comfyui.classifier_model = CLASSIFIER.to_string();
        let state = common::create_test_state(config, pool.clone());
        Self {
            pool,
            state,
            provider,
            workspace,
            user,
        }
    }

    fn auth(&self) -> AuthUser {
        AuthUser(Claims {
            sub: self.user.to_string(),
            email: "test@example.com".into(),
            roles: vec![],
            permissions: vec![],
            exp: 0,
            iat: 0,
            jti: Uuid::new_v4().to_string(),
            is_admin: false,
        })
    }

    async fn create(&self, body: Value) -> (StatusCode, Value) {
        let request: CreateTaskRequest = serde_json::from_value(body).unwrap();
        let response = create(
            State(self.state.clone()),
            self.auth(),
            Path(self.workspace),
            Json(request),
        )
        .await
        .into_response();
        read(response).await
    }

    async fn update(&self, task: Uuid, body: Value) -> (StatusCode, Value) {
        let request: UpdateTaskRequest = serde_json::from_value(body).unwrap();
        let response = update(
            State(self.state.clone()),
            self.auth(),
            Path(task),
            Json(request),
        )
        .await
        .into_response();
        read(response).await
    }

    async fn task(&self, id: Uuid) -> tasks::TaskRow {
        tasks::get_task(&self.pool, id).await.unwrap().unwrap()
    }

    /// The task once nothing can replace its title any more.
    async fn settled(&self, id: Uuid) -> tasks::TaskRow {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let task = self.task(id).await;
                if !task.title_pending {
                    return task;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the title settles")
    }

    async fn requests(&self) -> Vec<Value> {
        self.provider
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| serde_json::from_slice(&request.body).unwrap())
            .collect()
    }
}

async fn read(response: Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn answering(title: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "completion", "object": "chat.completion", "created": 0, "model": CLASSIFIER,
        "choices": [{"index": 0, "message": {"role": "assistant", "content": title}, "finish_reason": "stop"}]
    }))
}

fn id(task: &Value) -> Uuid {
    Uuid::parse_str(task["id"].as_str().unwrap()).unwrap()
}

#[tokio::test]
async fn an_untitled_task_shows_a_placeholder_until_its_generated_title_arrives() {
    let fixture = Fixture::new(answering(&format!("\"{GENERATED}\"\n"))).await;

    let (status, body) = fixture
        .create(json!({"description": DESCRIPTION, "is_agentic": false}))
        .await;

    assert_eq!(status, StatusCode::CREATED);
    let placeholder = "Users on Safari are sent back to the login page every time…";
    assert_eq!(tasks::placeholder_title(DESCRIPTION), placeholder);
    assert_eq!(body["task"]["title"], placeholder);
    assert_eq!(body["task"]["title_pending"], true);
    let settled = fixture.settled(id(&body["task"])).await;
    assert_eq!(settled.title, GENERATED);
    let requests = fixture.requests().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["model"], CLASSIFIER);
    assert!(
        requests[0]["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("concise task title"),
        "{}",
        requests[0]
    );
    assert_eq!(requests[0]["messages"][1]["content"], DESCRIPTION);
}

#[tokio::test]
async fn a_title_the_user_typed_is_kept_and_never_generated() {
    let fixture = Fixture::new(answering(GENERATED)).await;

    let (status, body) = fixture
        .create(json!({"title": "  Safari sign-in ", "description": DESCRIPTION}))
        .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["task"]["title"], "Safari sign-in");
    assert_eq!(body["task"]["title_pending"], false);
    let task = id(&body["task"]);
    assert!(!titles::title_task(&fixture.state, task).await.unwrap());
    assert_eq!(fixture.task(task).await.title, "Safari sign-in");
    assert!(fixture.requests().await.is_empty());
}

#[tokio::test]
async fn a_title_edited_while_one_is_generated_is_not_overwritten() {
    let fixture = Fixture::new(answering(GENERATED).set_delay(Duration::from_millis(500))).await;
    let task = match tasks::create_task_authorized(
        &fixture.pool,
        fixture.user,
        tasks::Create {
            workspace_id: fixture.workspace,
            project_ids: &[],
            title: None,
            description: DESCRIPTION,
            acceptance_criteria: None,
            priority: None,
            is_agentic: false,
            require_plan_approval: false,
            source_id: None,
            created_by: Some(fixture.user),
            chat_id: None,
        },
    )
    .await
    .unwrap()
    {
        tasks::Mutation::Applied(task) => task,
        tasks::Mutation::NotFound => panic!("the member may create tasks"),
    };
    assert!(task.title_pending);

    let (generated, (status, edited)) =
        tokio::join!(titles::title_task(&fixture.state, task.id), async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            fixture
                .update(task.id, json!({"title": "Safari cookie bug"}))
                .await
        });

    assert_eq!(status, StatusCode::OK);
    assert_eq!(edited["task"]["title_pending"], false);
    assert!(!generated.unwrap(), "the generated title replaced an edit");
    assert_eq!(
        fixture.requests().await.len(),
        1,
        "generation was in flight"
    );
    let task = fixture.task(task.id).await;
    assert_eq!(task.title, "Safari cookie bug");
    assert!(!task.title_pending);
}

#[tokio::test]
async fn a_failed_generation_keeps_the_placeholder() {
    let fixture = Fixture::new(ResponseTemplate::new(500)).await;

    let (status, body) = fixture.create(json!({"description": DESCRIPTION})).await;

    assert_eq!(status, StatusCode::CREATED);
    let settled = fixture.settled(id(&body["task"])).await;
    assert_eq!(settled.title, tasks::placeholder_title(DESCRIPTION));
    assert_eq!(fixture.requests().await.len(), 1);
}

#[tokio::test]
async fn a_placeholder_whose_generation_was_lost_is_kept() {
    let fixture = Fixture::new(answering(GENERATED)).await;
    let created = actions::create_task(
        &fixture.pool,
        fixture.workspace,
        fixture.user,
        None,
        actions::Task {
            title: None,
            description: DESCRIPTION.to_string(),
            assignee_id: None,
        },
    )
    .await
    .unwrap();
    let fresh = actions::create_task(
        &fixture.pool,
        fixture.workspace,
        fixture.user,
        None,
        actions::Task {
            title: None,
            description: DESCRIPTION.to_string(),
            assignee_id: None,
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE tasks SET created_at = NOW() - INTERVAL '10 minutes' WHERE id = $1")
        .bind(id(&created))
        .execute(&fixture.pool)
        .await
        .unwrap();

    assert!(tasks::settle_stale_titles(&fixture.pool).await.unwrap() >= 1);

    let abandoned = fixture.task(id(&created)).await;
    assert!(!abandoned.title_pending);
    assert_eq!(abandoned.title, tasks::placeholder_title(DESCRIPTION));
    assert!(fixture.task(id(&fresh)).await.title_pending);
    assert!(
        !titles::title_task(&fixture.state, id(&created))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn a_task_the_server_creates_keeps_the_title_it_was_given() {
    let fixture = Fixture::new(answering(GENERATED)).await;

    let task = tasks::create_task(
        &fixture.pool,
        fixture.workspace,
        &[],
        "",
        DESCRIPTION,
        None,
        None,
        false,
        None,
    )
    .await
    .unwrap();

    assert!(!task.title_pending);
    assert_eq!(task.title, "");
}

#[tokio::test]
async fn a_task_needs_a_title_or_a_description() {
    let fixture = Fixture::new(answering(GENERATED)).await;

    let (status, _) = fixture
        .create(json!({"title": "  ", "description": " "}))
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_update_route_sets_the_title() {
    let fixture = Fixture::new(answering(GENERATED)).await;
    let (_, body) = fixture
        .create(json!({"title": "Safari sign-in", "description": DESCRIPTION}))
        .await;
    let task = id(&body["task"]);

    let (status, updated) = fixture
        .update(task, json!({"title": "Safari login loop"}))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["task"]["title"], "Safari login loop");
    assert_eq!(fixture.task(task).await.title, "Safari login loop");
    let (status, _) = fixture.update(task, json!({"title": "   "})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(fixture.task(task).await.title, "Safari login loop");
}

#[tokio::test]
async fn a_task_an_agent_creates_without_a_title_is_titled_the_same_way() {
    let fixture = Fixture::new(answering(GENERATED)).await;

    let created = actions::create_task(
        &fixture.pool,
        fixture.workspace,
        fixture.user,
        None,
        actions::Task {
            title: None,
            description: DESCRIPTION.to_string(),
            assignee_id: None,
        },
    )
    .await
    .unwrap();
    let started = actions::start_task(
        &fixture.pool,
        fixture.workspace,
        fixture.user,
        None,
        serde_json::from_value(json!({"description": DESCRIPTION})).unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(created["title"], tasks::placeholder_title(DESCRIPTION));
    assert_eq!(created["title_pending"], true);
    assert_eq!(started["title"], tasks::placeholder_title(DESCRIPTION));
    for task in [
        id(&created),
        Uuid::parse_str(started["task_id"].as_str().unwrap()).unwrap(),
    ] {
        assert!(titles::title_task(&fixture.state, task).await.unwrap());
        assert_eq!(fixture.task(task).await.title, GENERATED);
    }
    let refused = actions::create_task(
        &fixture.pool,
        fixture.workspace,
        fixture.user,
        None,
        actions::Task {
            title: None,
            description: "  ".to_string(),
            assignee_id: None,
        },
    )
    .await;
    assert!(refused.is_err());
}
