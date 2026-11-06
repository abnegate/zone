use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::*;

async fn pool() -> PgPool {
    PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
        .await
        .expect("the test database accepts connections")
}

struct Fixture {
    pool: PgPool,
    workspace: Uuid,
    user: Uuid,
}

async fn fixture() -> Fixture {
    let pool = pool().await;
    let organization = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    let user = Uuid::new_v4();
    sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Ordering', $1::text)")
        .bind(organization)
        .execute(&pool)
        .await
        .expect("an organization");
    sqlx::query(
        "INSERT INTO workspaces (id, organization_id, name, slug) VALUES ($1, $2, 'Ordering', $1::text)",
    )
    .bind(workspace)
    .bind(organization)
    .execute(&pool)
    .await
    .expect("a workspace");
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name) VALUES ($1, $1::text, 'test-only', 'Ada')",
    )
    .bind(user)
    .execute(&pool)
    .await
    .expect("a user");
    Fixture {
        pool,
        workspace,
        user,
    }
}

impl Fixture {
    async fn project(&self, name: &str) -> Uuid {
        create_project(&self.pool, name, None, Some(self.workspace))
            .await
            .expect("a project")
            .id
    }

    async fn names(&self) -> Vec<String> {
        list_projects(&self.pool, self.workspace, None)
            .await
            .expect("the projects")
            .into_iter()
            .map(|project| project.name)
            .collect()
    }

    async fn last_used(&self, project: Uuid) -> Option<NaiveDateTime> {
        get_project(&self.pool, project)
            .await
            .expect("the project")
            .expect("a stored project")
            .last_used_at
    }

    async fn chat(&self, project: Uuid) -> Uuid {
        sqlx::query_scalar(
            "INSERT INTO chats (workspace_id, title, model_name, project_id) VALUES ($1, 'Work', 'test', $2) RETURNING id",
        )
        .bind(self.workspace)
        .bind(project)
        .fetch_one(&self.pool)
        .await
        .expect("a chat")
    }

    async fn message(&self, chat: Uuid, role: &str, metadata: serde_json::Value, at: &str) {
        sqlx::query(
            "INSERT INTO messages (chat_id, role, content, metadata, created_at) VALUES ($1, $2, 'Hello', $3, $4::timestamp)",
        )
        .bind(chat)
        .bind(role)
        .bind(metadata)
        .bind(at)
        .execute(&self.pool)
        .await
        .expect("a message");
    }

    async fn task(&self, project: Uuid, created_by: Option<Uuid>, at: &str) -> Uuid {
        let task: Uuid = sqlx::query_scalar(
            "INSERT INTO tasks (workspace_id, title, description, created_by, created_at) VALUES ($1, 'Task', 'Do it', $2, $3::timestamp) RETURNING id",
        )
        .bind(self.workspace)
        .bind(created_by)
        .bind(at)
        .fetch_one(&self.pool)
        .await
        .expect("a task");
        sqlx::query("INSERT INTO task_projects (task_id, project_id) VALUES ($1, $2)")
            .bind(task)
            .bind(project)
            .execute(&self.pool)
            .await
            .expect("a task in the project");
        task
    }
}

fn at(value: &str) -> Option<NaiveDateTime> {
    Some(NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").expect("a timestamp"))
}

#[tokio::test]
async fn a_new_project_goes_first_in_its_workspaces_order() {
    let fixture = fixture().await;
    fixture.project("Older").await;
    fixture.project("Newer").await;

    assert_eq!(fixture.names().await, ["Newer", "Older"]);
}

#[tokio::test]
async fn a_reorder_is_kept_and_listed() {
    let fixture = fixture().await;
    let alpha = fixture.project("Alpha").await;
    let beta = fixture.project("Beta").await;
    let gamma = fixture.project("Gamma").await;

    let outcome = reorder(&fixture.pool, fixture.workspace, &[alpha, gamma, beta])
        .await
        .expect("the reorder");

    assert_eq!(outcome, Reorder::Reordered);
    assert_eq!(fixture.names().await, ["Alpha", "Gamma", "Beta"]);
    fixture.project("Delta").await;
    assert_eq!(
        fixture.names().await,
        ["Delta", "Alpha", "Gamma", "Beta"],
        "a project created after a reorder still goes first"
    );
}

#[tokio::test]
async fn a_reorder_of_some_projects_leaves_the_rest_in_place() {
    let fixture = fixture().await;
    let alpha = fixture.project("Alpha").await;
    fixture.project("Beta").await;
    let gamma = fixture.project("Gamma").await;

    reorder(&fixture.pool, fixture.workspace, &[alpha, gamma])
        .await
        .expect("the reorder");

    assert_eq!(fixture.names().await, ["Alpha", "Beta", "Gamma"]);
}

#[tokio::test]
async fn a_reorder_naming_another_workspaces_project_changes_nothing() {
    let other = fixture().await;
    let fixture = fixture().await;
    let alpha = fixture.project("Alpha").await;
    fixture.project("Beta").await;
    let outsider = other.project("Outsider").await;

    let outcome = reorder(&fixture.pool, fixture.workspace, &[alpha, outsider])
        .await
        .expect("the reorder");

    assert_eq!(outcome, Reorder::Unknown);
    assert_eq!(fixture.names().await, ["Beta", "Alpha"]);
}

#[tokio::test]
async fn a_project_nobody_worked_in_was_never_used() {
    let fixture = fixture().await;
    let project = fixture.project("Quiet").await;

    assert_eq!(fixture.last_used(project).await, None);
}

#[tokio::test]
async fn a_message_a_member_typed_marks_the_project_used() {
    let fixture = fixture().await;
    let project = fixture.project("Chatty").await;
    let chat = fixture.chat(project).await;
    fixture
        .message(chat, "user", serde_json::json!({}), "2026-11-01 09:00:00")
        .await;
    fixture
        .message(
            chat,
            "assistant",
            serde_json::json!({}),
            "2026-11-02 09:00:00",
        )
        .await;
    fixture
        .message(
            chat,
            "user",
            serde_json::json!({"source": "reminder"}),
            "2026-11-03 09:00:00",
        )
        .await;

    assert_eq!(
        fixture.last_used(project).await,
        at("2026-11-01 09:00:00"),
        "replies and reminder posts are not a member using the project"
    );
}

#[tokio::test]
async fn a_task_a_member_added_marks_the_project_used_but_a_planned_one_does_not() {
    let fixture = fixture().await;
    let project = fixture.project("Busy").await;
    let chat = fixture.chat(project).await;
    fixture
        .message(chat, "user", serde_json::json!({}), "2026-11-01 09:00:00")
        .await;
    fixture
        .task(project, Some(fixture.user), "2026-11-02 09:00:00")
        .await;
    let planned = fixture
        .task(project, Some(fixture.user), "2026-11-03 09:00:00")
        .await;
    sqlx::query(
        "INSERT INTO task_automation (task_id, project_id, auto_created) VALUES ($1, $2, TRUE)",
    )
    .bind(planned)
    .bind(project)
    .execute(&fixture.pool)
    .await
    .expect("a planned task");
    fixture.task(project, None, "2026-11-04 09:00:00").await;

    assert_eq!(fixture.last_used(project).await, at("2026-11-02 09:00:00"));
}
