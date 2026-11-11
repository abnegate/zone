use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use uuid::Uuid;
use zone_server::auth::{AuthUser, jwt::Claims};
use zone_server::db::tasks::{CANCELLED, COMPLETE, CREATED, IN_PROGRESS, PAUSED, QUEUED};
use zone_server::db::{migrations, tasks};
use zone_server::routes::tasks as routes;
use zone_server::state::AppState;

use crate::common::{self, Database};

const PREFIX: &str = "task_statuses";
const CHECK_VIOLATION: &str = "23514";
const CONSTRAINT: &str = "tasks_status_check";
const UNKNOWN: &str = "archived";

struct Fixture {
    database: Database,
    state: AppState,
    actor: Uuid,
    workspace: Uuid,
}

impl Fixture {
    async fn new() -> Self {
        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool).await.unwrap();
        let (_, workspace, actor) = common::setup_test_data(&database.pool).await;
        sqlx::query(
            "INSERT INTO workspace_members(workspace_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(workspace)
        .bind(actor)
        .execute(&database.pool)
        .await
        .unwrap();
        let state = common::create_test_state(common::test_config(), database.pool.clone());
        Self {
            database,
            state,
            actor,
            workspace,
        }
    }

    async fn task(&self) -> Uuid {
        tasks::create_task(
            &self.database.pool,
            self.workspace,
            &[],
            "Statuses",
            "Regression",
            None,
            None,
            true,
            None,
        )
        .await
        .unwrap()
        .id
    }

    fn auth(&self) -> AuthUser {
        AuthUser(Claims {
            sub: self.actor.to_string(),
            email: "member@example.com".into(),
            roles: vec![],
            permissions: vec![],
            exp: 0,
            iat: 0,
            jti: Uuid::new_v4().to_string(),
            is_admin: false,
        })
    }

    async fn put(&self, task: Uuid, status: &str) -> Response {
        let request = serde_json::from_value(json!({ "status": status })).unwrap();
        routes::update(
            State(self.state.clone()),
            self.auth(),
            Path(task),
            Json(request),
        )
        .await
        .into_response()
    }

    async fn get(&self, task: Uuid) -> Response {
        routes::get(State(self.state.clone()), self.auth(), Path(task))
            .await
            .into_response()
    }

    async fn status(&self, task: Uuid) -> String {
        tasks::get_task(&self.database.pool, task)
            .await
            .unwrap()
            .unwrap()
            .status
    }
}

async fn body(response: Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn the_constraint_admits_paused_and_cancelled_and_nothing_unknown() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let (_, workspace, _) = common::setup_test_data(&database.pool).await;
    let insert = "INSERT INTO tasks(workspace_id, title, description, status) VALUES ($1, 'Constraint', 'Regression', $2)";

    for status in tasks::STATUSES {
        sqlx::query(insert)
            .bind(workspace)
            .bind(status)
            .execute(&database.pool)
            .await
            .unwrap_or_else(|error| panic!("{CONSTRAINT} must admit {status}: {error}"));
    }

    let refused = sqlx::query(insert)
        .bind(workspace)
        .bind(UNKNOWN)
        .execute(&database.pool)
        .await
        .expect_err("the status check must remain a closed set");
    let refused = refused.as_database_error().unwrap();
    assert_eq!(refused.code().as_deref(), Some(CHECK_VIOLATION));
    assert_eq!(refused.constraint(), Some(CONSTRAINT));

    let (definition, validated): (String, bool) = sqlx::query_as(
        "SELECT pg_get_constraintdef(oid), convalidated FROM pg_constraint \
         WHERE conrelid = 'tasks'::regclass AND conname = $1",
    )
    .bind(CONSTRAINT)
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert!(validated, "{CONSTRAINT} must be validated");
    for status in tasks::STATUSES {
        assert!(
            definition.contains(&format!("'{status}'::text")),
            "{CONSTRAINT} must list {status}: {definition}"
        );
    }
    assert!(
        !definition.contains(UNKNOWN),
        "{CONSTRAINT} must not list {UNKNOWN}: {definition}"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn a_person_can_pause_and_cancel_a_task() {
    let fixture = Fixture::new().await;
    let task = fixture.task().await;

    for status in [PAUSED, CANCELLED] {
        let response = fixture.put(task, status).await;
        assert_eq!(response.status(), StatusCode::OK, "PUT {status}");
        assert_eq!(body(response).await["task"]["status"], status);

        let response = fixture.get(task).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            body(response).await["task"]["status"],
            status,
            "{status} must survive a round trip"
        );
    }

    fixture.database.cleanup().await;
}

#[tokio::test]
async fn an_unknown_status_is_a_bad_request() {
    let fixture = Fixture::new().await;
    let task = fixture.task().await;

    let response = fixture.put(task, UNKNOWN).await;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "an unknown status must be refused, not reach {CONSTRAINT} as a server error"
    );
    assert_eq!(body(response).await["error"], "Unknown task status");
    assert_eq!(fixture.status(task).await, CREATED);

    fixture.database.cleanup().await;
}

#[tokio::test]
async fn an_unknown_status_is_refused_by_a_write_that_skips_the_route() {
    let fixture = Fixture::new().await;
    let task = fixture.task().await;

    let refused = tasks::update_task(
        &fixture.database.pool,
        task,
        None,
        None,
        None,
        Some(UNKNOWN),
        None,
        None,
    )
    .await;
    assert!(
        matches!(&refused, Err(sqlx::Error::Protocol(message)) if message == tasks::UNKNOWN_STATUS),
        "an unknown status must be refused by name, not reach {CONSTRAINT}: {refused:?}"
    );
    assert_eq!(tasks::Status::parse(UNKNOWN), None);
    for status in tasks::STATUSES {
        assert_eq!(
            tasks::Status::parse(status).map(tasks::Status::as_str),
            Some(status)
        );
    }
    assert_eq!(fixture.status(task).await, CREATED);

    fixture.database.cleanup().await;
}

#[tokio::test]
async fn completing_a_task_by_hand_stamps_it_and_moving_it_back_out_clears_the_stamp() {
    let fixture = Fixture::new().await;
    let task = fixture.task().await;

    let response = fixture.put(task, COMPLETE).await;
    assert_eq!(response.status(), StatusCode::OK);
    let completed = body(response).await["task"]["completed_at"].clone();
    assert!(
        completed.is_string(),
        "a task a person completes is recorded as completed: {completed}"
    );

    let response = fixture.put(task, COMPLETE).await;
    assert_eq!(
        body(response).await["task"]["completed_at"],
        completed,
        "restating the status moves nothing"
    );

    let response = fixture.put(task, IN_PROGRESS).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body(response).await["task"]["completed_at"],
        Value::Null,
        "a task moved back out is no longer completed"
    );

    fixture.database.cleanup().await;
}

#[tokio::test]
async fn an_active_run_still_owns_the_status() {
    let fixture = Fixture::new().await;
    let task = fixture.task().await;
    tasks::create_task_run(&fixture.database.pool, task)
        .await
        .unwrap();

    for status in [PAUSED, CANCELLED] {
        let response = fixture.put(task, status).await;
        assert_eq!(
            response.status(),
            StatusCode::CONFLICT,
            "{status} must not land underneath a live run"
        );
        assert_eq!(
            body(response).await["error"],
            "Task has an active run or is no longer available"
        );
    }
    assert_eq!(fixture.status(task).await, QUEUED);

    let request = serde_json::from_value(json!({
        "title": "Renamed mid-run",
        "description": "Edited mid-run",
    }))
    .unwrap();
    let response = routes::update(
        State(fixture.state.clone()),
        fixture.auth(),
        Path(task),
        Json(request),
    )
    .await
    .into_response();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a run owns only the status, so edits that leave it alone still land"
    );
    let edited = body(response).await;
    assert_eq!(edited["task"]["title"], "Renamed mid-run");
    assert_eq!(edited["task"]["description"], "Edited mid-run");
    assert_eq!(edited["task"]["status"], QUEUED);

    let request = serde_json::from_value(json!({
        "title": "Saved whole mid-run",
        "status": QUEUED,
    }))
    .unwrap();
    let response = routes::update(
        State(fixture.state.clone()),
        fixture.auth(),
        Path(task),
        Json(request),
    )
    .await
    .into_response();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a status the task already has changes nothing the run owns"
    );
    let restated = body(response).await;
    assert_eq!(restated["task"]["title"], "Saved whole mid-run");
    assert_eq!(restated["task"]["status"], QUEUED);

    fixture.database.cleanup().await;
}

#[tokio::test]
async fn queueing_or_running_a_paused_or_cancelled_task_queues_it_again() {
    let fixture = Fixture::new().await;

    for status in [PAUSED, CANCELLED] {
        let task = fixture.task().await;
        assert_eq!(fixture.put(task, status).await.status(), StatusCode::OK);

        let response = routes::queue(State(fixture.state.clone()), fixture.auth(), Path(task))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK, "queue a {status} task");
        assert_eq!(fixture.status(task).await, QUEUED);

        assert_eq!(fixture.put(task, status).await.status(), StatusCode::OK);
        let admitted =
            tasks::create_task_run_authorized(&fixture.database.pool, fixture.actor, task)
                .await
                .unwrap();
        assert!(
            matches!(
                admitted,
                tasks::Mutation::Applied(tasks::RunMutation::Created(_))
            ),
            "a {status} task must admit a run a person starts"
        );
        assert_eq!(fixture.status(task).await, QUEUED);
    }

    fixture.database.cleanup().await;
}
