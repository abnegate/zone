use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::*;
use crate::db::actions::{self, StartTask};
use crate::db::github_installations;
use crate::db::sources::testing;
use crate::db::tasks::Origin;
use crate::db::workspace_members::{self, WorkspaceRole};

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
        last_used(&self.pool, self.workspace)
            .await
            .expect("when the projects were used")
            .get(&project)
            .copied()
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

    async fn task(&self, project: Uuid, origin: Origin, at: &str) -> Uuid {
        let task: Uuid = sqlx::query_scalar(
            "INSERT INTO tasks (workspace_id, title, description, created_by, origin, created_at) VALUES ($1, 'Task', 'Do it', $2, $3, $4::timestamp) RETURNING id",
        )
        .bind(self.workspace)
        .bind(self.user)
        .bind(origin.as_str())
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

    async fn start(&self, project: Uuid, origin: Origin) -> Uuid {
        let started = actions::start_task(
            &self.pool,
            self.workspace,
            self.user,
            None,
            origin,
            StartTask {
                title: Some("Task".into()),
                description: "Do it".into(),
                acceptance_criteria: None,
                project_ids: vec![project],
                source_id: None,
                priority: None,
            },
        )
        .await
        .expect("a started task");
        started["task_id"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok())
            .expect("the started task's id")
    }

    async fn created(&self, task: Uuid) -> NaiveDateTime {
        sqlx::query_scalar("SELECT created_at FROM tasks WHERE id = $1")
            .bind(task)
            .fetch_one(&self.pool)
            .await
            .expect("the task's creation time")
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
async fn a_reorder_cannot_pull_in_another_workspaces_project() {
    let other = fixture().await;
    let fixture = fixture().await;
    let alpha = fixture.project("Alpha").await;
    let beta = fixture.project("Beta").await;
    let outsider = other.project("Outsider").await;

    let outcome = reorder(&fixture.pool, fixture.workspace, &[alpha, outsider, beta])
        .await
        .expect("the reorder");

    assert_eq!(outcome, Reorder::Reordered);
    assert_eq!(fixture.names().await, ["Alpha", "Beta"]);
    assert_eq!(other.names().await, ["Outsider"]);
}

#[tokio::test]
async fn a_reorder_naming_a_project_twice_changes_nothing() {
    let fixture = fixture().await;
    let alpha = fixture.project("Alpha").await;
    fixture.project("Beta").await;

    let outcome = reorder(&fixture.pool, fixture.workspace, &[alpha, alpha])
        .await
        .expect("the reorder");

    assert_eq!(outcome, Reorder::Duplicated);
    assert_eq!(fixture.names().await, ["Beta", "Alpha"]);
}

#[tokio::test]
async fn concurrent_reorders_each_keep_what_they_moved() {
    let fixture = fixture().await;
    let alpha = fixture.project("Alpha").await;
    let beta = fixture.project("Beta").await;
    let gamma = fixture.project("Gamma").await;
    let delta = fixture.project("Delta").await;
    reorder(
        &fixture.pool,
        fixture.workspace,
        &[alpha, beta, gamma, delta],
    )
    .await
    .expect("a known order");

    let mut holder = fixture.pool.begin().await.expect("a transaction");
    lock_order(&mut holder, fixture.workspace)
        .await
        .expect("the order lock");
    sqlx::query(r#"UPDATE projects SET "position" = 5 - "position" WHERE workspace_id = $1"#)
        .bind(fixture.workspace)
        .execute(&mut *holder)
        .await
        .expect("the first reorder reverses the list");
    let second = tokio::spawn({
        let pool = fixture.pool.clone();
        let workspace = fixture.workspace;
        async move { reorder(&pool, workspace, &[alpha, beta]).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    holder.commit().await.expect("the first reorder lands");

    second
        .await
        .expect("the second reorder ran")
        .expect("the second reorder");
    assert_eq!(
        fixture.names().await,
        ["Delta", "Gamma", "Alpha", "Beta"],
        "the second reorder swaps Alpha and Beta within the order the first one left"
    );
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
async fn a_task_a_member_added_marks_the_project_used_but_an_automated_one_does_not() {
    let fixture = fixture().await;
    let project = fixture.project("Busy").await;
    let chat = fixture.chat(project).await;
    fixture
        .message(chat, "user", serde_json::json!({}), "2026-11-01 09:00:00")
        .await;
    fixture
        .task(project, Origin::Member, "2026-11-02 09:00:00")
        .await;
    fixture
        .task(project, Origin::Automation, "2026-11-03 09:00:00")
        .await;
    fixture
        .task(project, Origin::Reminder, "2026-11-04 09:00:00")
        .await;

    assert_eq!(fixture.last_used(project).await, at("2026-11-02 09:00:00"));
}

#[tokio::test]
async fn a_task_a_reminder_started_does_not_mark_the_project_used() {
    let fixture = fixture().await;
    workspace_members::add_member(
        &fixture.pool,
        fixture.workspace,
        fixture.user,
        WorkspaceRole::Member,
        None,
    )
    .await
    .expect("a member of the workspace");
    let reminded = fixture.project("Reminded").await;
    let worked = fixture.project("Worked").await;

    fixture.start(reminded, Origin::Reminder).await;
    let started = fixture.start(worked, Origin::Member).await;

    assert_eq!(
        fixture.last_used(reminded).await,
        None,
        "a task a reminder turn started is not a member using the project"
    );
    assert_eq!(
        fixture.last_used(worked).await,
        Some(fixture.created(started).await),
        "a task a member's turn started marks the project used"
    );
}

#[tokio::test]
async fn the_latest_typed_message_in_any_project_chat_wins() {
    let fixture = fixture().await;
    let project = fixture.project("Busy").await;
    let first = fixture.chat(project).await;
    let second = fixture.chat(project).await;
    for (chat, at) in [
        (first, "2026-11-01 09:00:00"),
        (first, "2026-11-03 09:00:00"),
        (second, "2026-11-02 09:00:00"),
        (second, "2026-11-04 09:00:00"),
        (first, "2026-11-02 12:00:00"),
    ] {
        fixture
            .message(chat, "user", serde_json::json!({}), at)
            .await;
    }

    assert_eq!(fixture.last_used(project).await, at("2026-11-04 09:00:00"));
}

#[tokio::test]
async fn metadata_that_is_not_an_object_does_not_hide_a_typed_message() {
    let fixture = fixture().await;
    let project = fixture.project("Listed").await;
    let chat = fixture.chat(project).await;
    fixture
        .message(
            chat,
            "user",
            serde_json::json!(["source"]),
            "2026-11-05 09:00:00",
        )
        .await;

    assert_eq!(fixture.last_used(project).await, at("2026-11-05 09:00:00"));
}

#[tokio::test]
async fn an_auto_project_goes_first_in_its_workspaces_order() {
    let fixture = fixture().await;
    fixture.project("Manual").await;
    fixture.project("Other").await;
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'owner')",
    )
    .bind(fixture.workspace)
    .bind(fixture.user)
    .execute(&fixture.pool)
    .await
    .expect("an owner");

    crate::db::auto_projects::finalize(
        &fixture.pool,
        fixture.workspace,
        fixture.user,
        None,
        Origin::Member,
        &crate::db::auto_projects::ProjectPlan {
            name: "Planned",
            description: None,
            repository: None,
            brief: &serde_json::json!({}),
            tasks: &[],
        },
    )
    .await
    .expect("an auto project");

    assert_eq!(fixture.names().await, ["Planned", "Other", "Manual"]);
}

async fn widgets(pool: &PgPool, workspace_id: Uuid) -> ProjectRow {
    create(
        pool,
        NewProject {
            name: "Widgets",
            description: None,
            workspace_id: Some(workspace_id),
            source_id: None,
            status: None,
        },
    )
    .await
    .expect("a project")
}

#[tokio::test]
async fn linking_with_an_installation_stores_it_and_unlinking_clears_it() {
    let pool = testing::pool().await;
    let installation = testing::installation(&pool).await;
    let project = widgets(&pool, installation.workspace).await;
    assert_eq!(project.github_installation_id, None);

    let linked = link_github(
        &pool,
        project.id,
        "https://github.com/acme/widgets",
        None,
        Some(installation.id),
    )
    .await
    .expect("the link")
    .expect("the project");
    assert_eq!(linked.github_installation_id, Some(installation.id));
    assert_eq!(linked.github_access_token, None);

    let unlinked = unlink_github(&pool, project.id)
        .await
        .expect("the unlink")
        .expect("the project");
    assert_eq!(unlinked.github_installation_id, None);
    assert_eq!(unlinked.github_repo_url, None);
}

#[tokio::test]
async fn an_installation_lost_after_the_check_links_nothing() {
    let pool = testing::pool().await;
    let revoked = testing::installation(&pool).await;
    let deleted = testing::installation(&pool).await;
    let projects = [
        widgets(&pool, revoked.workspace).await,
        widgets(&pool, deleted.workspace).await,
    ];
    for installation in [&revoked, &deleted] {
        assert!(
            github_installations::attachable(&pool, installation.workspace, installation.id)
                .await
                .expect("the check")
                .is_some()
        );
    }
    sqlx::query("UPDATE github_installations SET revoked_at = NOW() WHERE id = $1")
        .bind(revoked.id)
        .execute(&pool)
        .await
        .expect("the installation is revoked");
    sqlx::query("DELETE FROM github_installations WHERE id = $1")
        .bind(deleted.id)
        .execute(&pool)
        .await
        .expect("the installation is deleted");

    for (project, installation, reason) in [
        (&projects[0], &revoked, "a revoked"),
        (&projects[1], &deleted, "a deleted"),
    ] {
        let linked = link_github(
            &pool,
            project.id,
            "https://github.com/acme/widgets",
            None,
            Some(installation.id),
        )
        .await;

        assert!(
            matches!(linked, Err(LinkError::Unattachable)),
            "{reason} installation must be refused, got {linked:?}"
        );
        let unchanged = get_project(&pool, project.id)
            .await
            .expect("the read")
            .expect("the project");
        assert_eq!(unchanged.github_installation_id, None, "{reason}");
        assert_eq!(unchanged.github_repo_url, None, "{reason}");
    }
}

#[tokio::test]
async fn linking_a_missing_project_finds_nothing_rather_than_refusing() {
    let pool = testing::pool().await;
    let installation = testing::installation(&pool).await;

    let linked = link_github(
        &pool,
        Uuid::new_v4(),
        "https://github.com/acme/widgets",
        None,
        Some(installation.id),
    )
    .await;

    assert!(matches!(linked, Ok(None)), "{linked:?}");
}

#[tokio::test]
async fn every_project_read_and_write_returns_the_installation() {
    let pool = testing::pool().await;
    let installation = testing::installation(&pool).await;
    let project = widgets(&pool, installation.workspace).await;
    link_github(
        &pool,
        project.id,
        "https://github.com/acme/widgets",
        None,
        Some(installation.id),
    )
    .await
    .expect("the link");
    let expected = Some(installation.id);

    let in_workspace = get_project_in_workspace(&pool, project.id, installation.workspace)
        .await
        .expect("the read")
        .expect("the project");
    assert_eq!(in_workspace.github_installation_id, expected);
    let by_id = get_project(&pool, project.id)
        .await
        .expect("the read")
        .expect("the project");
    assert_eq!(by_id.github_installation_id, expected);
    for status in [None, Some("active")] {
        let listed = list_projects(&pool, installation.workspace, status)
            .await
            .expect("the listing");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].github_installation_id, expected);
    }
    let renamed = update_project(&pool, project.id, Some("Gadgets"), None, None)
        .await
        .expect("the update")
        .expect("the project");
    assert_eq!(renamed.github_installation_id, expected);
    let unsourced = set_source(&pool, project.id, None)
        .await
        .expect("the update")
        .expect("the project");
    assert_eq!(unsourced.github_installation_id, expected);
}

#[test]
fn debug_redacts_the_access_token() {
    let row = ProjectRow {
        id: Uuid::new_v4(),
        workspace_id: None,
        source_id: None,
        name: "Widgets".to_string(),
        description: None,
        status: "active".to_string(),
        github_repo_url: Some("https://github.com/acme/widgets".to_string()),
        github_access_token: Some("ghp_legacy_plaintext".to_string()),
        created_at: None,
        updated_at: None,
        github_installation_id: None,
        position: 0,
    };

    let rendered = format!("{row:?}");

    assert!(!rendered.contains("ghp_legacy_plaintext"), "{rendered}");
    assert!(rendered.contains(REDACTED), "{rendered}");
    assert!(rendered.contains("acme/widgets"), "{rendered}");
}
