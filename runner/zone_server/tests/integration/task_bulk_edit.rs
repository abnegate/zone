use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_server::db::workspace_members::{self, WorkspaceRole};
use zone_server::db::{migrations, tasks};

use crate::common::{Database, TestClient, TestResponse, test_email, test_password};

const PREFIX: &str = "task_bulk_edit";
const TASK_NOT_FOUND: &str = "Task not found";
const PROJECT_UNAVAILABLE: &str = "Project is not available in this workspace";
const ACTIVE_RUN: &str = "Task has an active run or is no longer available";
const NO_TASKS: &str = "Choose at least one task";
const NO_CHANGES: &str = "Choose something to change";
const UNKNOWN_STATUS: &str = "Unknown task status";

struct Tenant {
    user: Uuid,
    token: String,
    workspace: Uuid,
}

struct Board {
    database: Database,
    client: TestClient,
}

impl Board {
    async fn new() -> Self {
        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool).await.unwrap();
        let client = TestClient::with_pool(database.pool.clone());
        Self { database, client }
    }

    async fn tenant(&self) -> Tenant {
        let response = self
            .client
            .post_json(
                "/api/auth/register",
                &json!({ "email": test_email(), "password": test_password() }),
            )
            .await;
        response.assert_status(StatusCode::CREATED);
        let body = response.json_value();
        let user = uuid(&body["user"]["id"]);
        let token = body["access_token"]
            .as_str()
            .expect("access token is returned")
            .to_owned();

        let organization = self
            .client
            .post_json_auth(
                "/api/organizations",
                &json!({ "name": "Task bulk edit", "slug": Uuid::new_v4().to_string() }),
                &token,
            )
            .await;
        organization.assert_status(StatusCode::CREATED);
        let organization = uuid(&organization.json_value()["organization"]["id"]);

        let workspace = self
            .client
            .post_json_auth(
                &format!("/api/organizations/{organization}/workspaces"),
                &json!({ "name": "Task bulk edit", "slug": Uuid::new_v4().to_string() }),
                &token,
            )
            .await;
        workspace.assert_status(StatusCode::CREATED);
        let workspace = uuid(&workspace.json_value()["workspace"]["id"]);

        Tenant {
            user,
            token,
            workspace,
        }
    }

    async fn join(&self, tenant: &Tenant, user: Uuid, role: WorkspaceRole) {
        workspace_members::add_member(&self.database.pool, tenant.workspace, user, role, None)
            .await
            .expect("the workspace takes the member");
    }

    async fn project(&self, owner: &Tenant) -> Uuid {
        let response = self
            .client
            .post_json_auth(
                "/api/projects",
                &json!({
                    "workspace_id": owner.workspace,
                    "name": format!("Project {}", Uuid::new_v4()),
                }),
                &owner.token,
            )
            .await;
        response.assert_status(StatusCode::CREATED);
        uuid(&response.json_value()["project"]["id"])
    }

    async fn task(&self, owner: &Tenant, project: Uuid) -> Uuid {
        let response = self
            .client
            .post_json_auth(
                &format!("/api/workspaces/{}/tasks", owner.workspace),
                &json!({
                    "project_ids": [project],
                    "title": "Bulk edited task",
                    "description": "Edited alongside others",
                    "priority": 1,
                }),
                &owner.token,
            )
            .await;
        response.assert_status(StatusCode::CREATED);
        uuid(&response.json_value()["task"]["id"])
    }

    async fn read(&self, owner: &Tenant, task: Uuid) -> Value {
        let response = self
            .client
            .get_auth(&format!("/api/tasks/{task}"), &owner.token)
            .await;
        response.assert_status(StatusCode::OK);
        response.json_value()["task"].clone()
    }

    async fn snapshot(&self, owner: &Tenant, tasks: &[Uuid]) -> Vec<Value> {
        let mut snapshot = Vec::with_capacity(tasks.len());
        for task in tasks {
            snapshot.push(self.read(owner, *task).await);
        }
        snapshot
    }

    async fn move_to(&self, owner: &Tenant, task: Uuid, status: &str) {
        self.client
            .put_json_auth(
                &format!("/api/tasks/{task}"),
                &json!({ "status": status }),
                &owner.token,
            )
            .await
            .assert_status(StatusCode::OK);
    }

    async fn edit(&self, workspace: Uuid, token: &str, body: &Value) -> TestResponse {
        self.client
            .patch_json_auth(&format!("/api/workspaces/{workspace}/tasks"), body, token)
            .await
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

fn uuid(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().expect("id is a string")).expect("id is a UUID")
}

fn ascending(ids: &[Uuid]) -> Vec<Uuid> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn edited(response: &TestResponse) -> Vec<Value> {
    response.assert_status(StatusCode::OK);
    response.json_value()["tasks"]
        .as_array()
        .expect("the edited tasks are listed")
        .clone()
}

fn listed(tasks: &[Value]) -> Vec<Uuid> {
    tasks.iter().map(|task| uuid(&task["id"])).collect()
}

fn refused(response: &TestResponse, status: StatusCode, message: &str) {
    response.assert_status(status);
    assert_eq!(response.json_value(), json!({ "error": message }));
}

#[tokio::test]
async fn every_change_lands_on_every_listed_task_and_nowhere_else() {
    let board = Board::new().await;
    let owner = board.tenant().await;
    let first_project = board.project(&owner).await;
    let second_project = board.project(&owner).await;
    let first = board.task(&owner, first_project).await;
    let second = board.task(&owner, first_project).await;
    let bystander = board.task(&owner, first_project).await;
    let untouched = board.snapshot(&owner, &[bystander]).await;

    let response = board
        .edit(
            owner.workspace,
            &owner.token,
            &json!({
                "ids": [second, first],
                "status": "blocked",
                "priority": 5,
                "project_ids": [second_project],
                "is_agentic": true,
                "require_plan_approval": true,
            }),
        )
        .await;
    let tasks = edited(&response);

    assert_eq!(listed(&tasks), ascending(&[first, second]));
    for task in tasks
        .iter()
        .chain(&board.snapshot(&owner, &[first, second]).await)
    {
        assert_eq!(task["status"], "blocked");
        assert_eq!(task["priority"], 5);
        assert_eq!(task["project_ids"], json!([second_project]));
        assert_eq!(task["is_agentic"], true);
        assert_eq!(task["require_plan_approval"], true);
        assert_eq!(task["title"], "Bulk edited task");
    }
    assert_eq!(board.snapshot(&owner, &[bystander]).await, untouched);

    board.cleanup().await;
}

#[tokio::test]
async fn an_empty_project_list_clears_every_listed_task() {
    let board = Board::new().await;
    let owner = board.tenant().await;
    let project = board.project(&owner).await;
    let first = board.task(&owner, project).await;
    let second = board.task(&owner, project).await;

    let response = board
        .edit(
            owner.workspace,
            &owner.token,
            &json!({ "ids": [first, second], "project_ids": [] }),
        )
        .await;
    let tasks = edited(&response);

    assert_eq!(listed(&tasks), ascending(&[first, second]));
    for task in tasks
        .iter()
        .chain(&board.snapshot(&owner, &[first, second]).await)
    {
        assert_eq!(task["project_ids"], json!([]));
        assert_eq!(task["status"], "created");
        assert_eq!(task["priority"], 1);
    }

    board.cleanup().await;
}

#[tokio::test]
async fn a_task_listed_twice_is_edited_and_returned_once() {
    let board = Board::new().await;
    let owner = board.tenant().await;
    let project = board.project(&owner).await;
    let first = board.task(&owner, project).await;
    let second = board.task(&owner, project).await;

    let response = board
        .edit(
            owner.workspace,
            &owner.token,
            &json!({ "ids": [first, second, first, second, first], "priority": 2 }),
        )
        .await;
    let tasks = edited(&response);

    assert_eq!(listed(&tasks), ascending(&[first, second]));
    assert!(tasks.iter().all(|task| task["priority"] == 2));

    board.cleanup().await;
}

#[tokio::test]
async fn a_task_from_another_tenant_hides_and_rolls_back_the_whole_edit() {
    let board = Board::new().await;
    let owner = board.tenant().await;
    let foreign = board.tenant().await;
    let project = board.project(&owner).await;
    let first = board.task(&owner, project).await;
    let second = board.task(&owner, project).await;
    let theirs = board.task(&foreign, board.project(&foreign).await).await;
    let own = board.snapshot(&owner, &[first, second]).await;
    let other = board.snapshot(&foreign, &[theirs]).await;

    let response = board
        .edit(
            owner.workspace,
            &owner.token,
            &json!({ "ids": [first, theirs, second], "status": "review", "priority": 4 }),
        )
        .await;

    refused(&response, StatusCode::NOT_FOUND, TASK_NOT_FOUND);
    assert_eq!(board.snapshot(&owner, &[first, second]).await, own);
    assert_eq!(board.snapshot(&foreign, &[theirs]).await, other);

    board.cleanup().await;
}

#[tokio::test]
async fn a_member_of_two_workspaces_cannot_edit_one_workspaces_task_through_the_other() {
    let board = Board::new().await;
    let first_owner = board.tenant().await;
    let second_owner = board.tenant().await;
    let member = board.tenant().await;
    board
        .join(&first_owner, member.user, WorkspaceRole::Member)
        .await;
    board
        .join(&second_owner, member.user, WorkspaceRole::Member)
        .await;
    let first_workspace_task = board
        .task(&first_owner, board.project(&first_owner).await)
        .await;
    let second_workspace_task = board
        .task(&second_owner, board.project(&second_owner).await)
        .await;
    let first_before = board.snapshot(&first_owner, &[first_workspace_task]).await;
    let second_before = board
        .snapshot(&second_owner, &[second_workspace_task])
        .await;

    let crossing = board
        .edit(
            first_owner.workspace,
            &member.token,
            &json!({ "ids": [second_workspace_task], "priority": 3 }),
        )
        .await;
    refused(&crossing, StatusCode::NOT_FOUND, TASK_NOT_FOUND);

    let mixed = board
        .edit(
            first_owner.workspace,
            &member.token,
            &json!({ "ids": [first_workspace_task, second_workspace_task], "priority": 3 }),
        )
        .await;
    refused(&mixed, StatusCode::NOT_FOUND, TASK_NOT_FOUND);

    assert_eq!(
        board.snapshot(&first_owner, &[first_workspace_task]).await,
        first_before
    );
    assert_eq!(
        board
            .snapshot(&second_owner, &[second_workspace_task])
            .await,
        second_before
    );

    let own = board
        .edit(
            first_owner.workspace,
            &member.token,
            &json!({ "ids": [first_workspace_task], "priority": 3 }),
        )
        .await;
    assert_eq!(edited(&own)[0]["priority"], 3);

    board.cleanup().await;
}

#[tokio::test]
async fn an_unknown_task_hides_and_rolls_back_the_whole_edit() {
    let board = Board::new().await;
    let owner = board.tenant().await;
    let project = board.project(&owner).await;
    let first = board.task(&owner, project).await;
    let second = board.task(&owner, project).await;
    let before = board.snapshot(&owner, &[first, second]).await;

    let response = board
        .edit(
            owner.workspace,
            &owner.token,
            &json!({ "ids": [first, Uuid::new_v4(), second], "status": "paused" }),
        )
        .await;

    refused(&response, StatusCode::NOT_FOUND, TASK_NOT_FOUND);
    assert_eq!(board.snapshot(&owner, &[first, second]).await, before);

    board.cleanup().await;
}

#[tokio::test]
async fn strangers_and_viewers_cannot_edit_many_tasks() {
    let board = Board::new().await;
    let owner = board.tenant().await;
    let stranger = board.tenant().await;
    let viewer = board.tenant().await;
    board.join(&owner, viewer.user, WorkspaceRole::Viewer).await;
    let project = board.project(&owner).await;
    let first = board.task(&owner, project).await;
    let second = board.task(&owner, project).await;
    let before = board.snapshot(&owner, &[first, second]).await;

    for outsider in [&stranger, &viewer] {
        let response = board
            .edit(
                owner.workspace,
                &outsider.token,
                &json!({ "ids": [first, second], "status": "cancelled", "priority": 4 }),
            )
            .await;
        refused(&response, StatusCode::NOT_FOUND, TASK_NOT_FOUND);
    }

    assert_eq!(board.snapshot(&owner, &[first, second]).await, before);

    board.cleanup().await;
}

#[tokio::test]
async fn a_status_change_on_a_running_task_conflicts_and_changes_nothing() {
    let board = Board::new().await;
    let owner = board.tenant().await;
    let project = board.project(&owner).await;
    let idle = board.task(&owner, project).await;
    let running = board.task(&owner, project).await;
    tasks::create_task_run(&board.database.pool, running)
        .await
        .expect("the task gets a run");
    let before = board.snapshot(&owner, &[idle, running]).await;
    assert_eq!(before[1]["status"], "queued");

    let response = board
        .edit(
            owner.workspace,
            &owner.token,
            &json!({ "ids": [idle, running], "status": "paused", "priority": 5 }),
        )
        .await;

    refused(&response, StatusCode::CONFLICT, ACTIVE_RUN);
    assert_eq!(board.snapshot(&owner, &[idle, running]).await, before);

    let response = board
        .edit(
            owner.workspace,
            &owner.token,
            &json!({ "ids": [idle, running], "priority": 5 }),
        )
        .await;
    let tasks = edited(&response);

    assert_eq!(listed(&tasks), ascending(&[idle, running]));
    let after = board.snapshot(&owner, &[idle, running]).await;
    assert_eq!(after[0]["status"], "created");
    assert_eq!(after[1]["status"], "queued");
    assert!(after.iter().all(|task| task["priority"] == 5));

    board.cleanup().await;
}

#[tokio::test]
async fn a_project_from_another_workspace_is_refused_and_changes_nothing() {
    let board = Board::new().await;
    let owner = board.tenant().await;
    let foreign = board.tenant().await;
    let project = board.project(&owner).await;
    let foreign_project = board.project(&foreign).await;
    let first = board.task(&owner, project).await;
    let second = board.task(&owner, project).await;
    let before = board.snapshot(&owner, &[first, second]).await;

    for project_ids in [json!([foreign_project]), json!([project, Uuid::new_v4()])] {
        let response = board
            .edit(
                owner.workspace,
                &owner.token,
                &json!({ "ids": [first, second], "priority": 3, "project_ids": project_ids }),
            )
            .await;
        refused(&response, StatusCode::BAD_REQUEST, PROJECT_UNAVAILABLE);
    }

    assert_eq!(board.snapshot(&owner, &[first, second]).await, before);

    board.cleanup().await;
}

#[tokio::test]
async fn a_malformed_edit_is_refused_and_changes_nothing() {
    let board = Board::new().await;
    let owner = board.tenant().await;
    let task = board.task(&owner, board.project(&owner).await).await;
    let before = board.snapshot(&owner, &[task]).await;

    for (body, message) in [
        (json!({ "ids": [], "priority": 3 }), NO_TASKS),
        (
            json!({ "ids": [task], "status": "archived" }),
            UNKNOWN_STATUS,
        ),
        (json!({ "ids": [task] }), NO_CHANGES),
    ] {
        let response = board.edit(owner.workspace, &owner.token, &body).await;
        refused(&response, StatusCode::BAD_REQUEST, message);
    }

    board
        .edit(
            owner.workspace,
            &owner.token,
            &json!({ "ids": [task], "title": "Not editable together" }),
        )
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);

    assert_eq!(board.snapshot(&owner, &[task]).await, before);

    board.cleanup().await;
}

#[tokio::test]
async fn withheld_tasks_moved_back_to_created_read_back_created() {
    let board = Board::new().await;
    let owner = board.tenant().await;
    let project = board.project(&owner).await;
    let paused = board.task(&owner, project).await;
    let cancelled = board.task(&owner, project).await;
    board.move_to(&owner, paused, "paused").await;
    board.move_to(&owner, cancelled, "cancelled").await;

    let response = board
        .edit(
            owner.workspace,
            &owner.token,
            &json!({ "ids": [paused, cancelled], "status": "created" }),
        )
        .await;
    let tasks = edited(&response);

    assert_eq!(listed(&tasks), ascending(&[paused, cancelled]));
    for task in tasks
        .iter()
        .chain(&board.snapshot(&owner, &[paused, cancelled]).await)
    {
        assert_eq!(task["status"], "created");
    }

    board.cleanup().await;
}
