use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use zone_server::db::migrations;
use zone_server::db::policies::{self, Layers};

use crate::common::Database;

const PREFIX: &str = "policy_store";
const CHECK_VIOLATION: &str = "23514";
const LOCK_NOT_AVAILABLE: &str = "55P03";
const ORGANIZATION_RULES_CHECK: &str = "organization_policies_rules_check";
const WORKSPACE_RULES_CHECK: &str = "workspace_policies_rules_check";

struct Fixture {
    database: Database,
}

impl Fixture {
    async fn new() -> Self {
        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool).await.unwrap();
        Self { database }
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    async fn organization(&self) -> Uuid {
        let organization = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Policy', $1::text)")
            .bind(organization)
            .execute(self.pool())
            .await
            .unwrap();
        organization
    }

    async fn workspace(&self, organization: Uuid) -> Uuid {
        let workspace = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO workspaces (id, organization_id, name, slug) \
             VALUES ($1, $2, 'Policy', $1::text)",
        )
        .bind(workspace)
        .bind(organization)
        .execute(self.pool())
        .await
        .unwrap();
        workspace
    }

    async fn user(&self) -> Uuid {
        let user = Uuid::new_v4();
        sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $1::text, 'policy')")
            .bind(user)
            .execute(self.pool())
            .await
            .unwrap();
        user
    }

    async fn save_organization(
        &self,
        organization: Uuid,
        rules: &Value,
        actor: Uuid,
    ) -> Result<policies::Stored, sqlx::Error> {
        let mut transaction = self.pool().begin().await.unwrap();
        let stored =
            policies::save_organization(&mut transaction, organization, rules, actor).await?;
        transaction.commit().await.unwrap();
        Ok(stored)
    }

    async fn save_workspace(
        &self,
        workspace: Uuid,
        rules: &Value,
        actor: Uuid,
    ) -> Result<policies::Stored, sqlx::Error> {
        let mut transaction = self.pool().begin().await.unwrap();
        let stored = policies::save_workspace(&mut transaction, workspace, rules, actor).await?;
        transaction.commit().await.unwrap();
        Ok(stored)
    }

    async fn delete(&self, table: &str, id: Uuid) {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE id = $1"
        )))
        .bind(id)
        .execute(self.pool())
        .await
        .unwrap();
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

fn organization_rules() -> Value {
    json!({ "providers": { "allow": ["anthropic"] } })
}

fn workspace_rules() -> Value {
    json!({ "models": { "allow": ["claude-sonnet"] } })
}

fn replaced_rules() -> Value {
    json!({ "ceiling": "write" })
}

fn violated_constraint(error: &sqlx::Error, code: &str) -> Option<String> {
    let database = error.as_database_error()?;
    assert_eq!(database.code().as_deref(), Some(code), "{error}");
    database.constraint().map(str::to_owned)
}

#[tokio::test]
async fn an_organization_policy_upserts_and_records_who_saved_it() {
    let fixture = Fixture::new().await;
    let organization = fixture.organization().await;
    let author = fixture.user().await;
    let editor = fixture.user().await;

    assert_eq!(
        policies::organization(fixture.pool(), organization)
            .await
            .unwrap(),
        None
    );

    let created = fixture
        .save_organization(organization, &organization_rules(), author)
        .await
        .unwrap();
    assert_eq!(created.rules, organization_rules());
    assert_eq!(created.updated_by, Some(author));
    assert_eq!(
        policies::organization(fixture.pool(), organization)
            .await
            .unwrap(),
        Some(created.clone())
    );

    let replaced = fixture
        .save_organization(organization, &replaced_rules(), editor)
        .await
        .unwrap();
    assert_eq!(replaced.rules, replaced_rules());
    assert_eq!(replaced.updated_by, Some(editor));
    assert!(replaced.updated_at >= created.updated_at);
    assert_eq!(
        policies::organization(fixture.pool(), organization)
            .await
            .unwrap(),
        Some(replaced)
    );

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM organization_policies")
        .fetch_one(fixture.pool())
        .await
        .unwrap();
    assert_eq!(rows, 1, "a second save updates the row in place");

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_workspace_policy_upserts_and_records_who_saved_it() {
    let fixture = Fixture::new().await;
    let organization = fixture.organization().await;
    let workspace = fixture.workspace(organization).await;
    let author = fixture.user().await;
    let editor = fixture.user().await;

    assert_eq!(
        policies::workspace(fixture.pool(), workspace)
            .await
            .unwrap(),
        None
    );

    let created = fixture
        .save_workspace(workspace, &workspace_rules(), author)
        .await
        .unwrap();
    assert_eq!(created.rules, workspace_rules());
    assert_eq!(created.updated_by, Some(author));

    let replaced = fixture
        .save_workspace(workspace, &replaced_rules(), editor)
        .await
        .unwrap();
    assert_eq!(replaced.rules, replaced_rules());
    assert_eq!(replaced.updated_by, Some(editor));
    assert!(replaced.updated_at >= created.updated_at);
    assert_eq!(
        policies::workspace(fixture.pool(), workspace)
            .await
            .unwrap(),
        Some(replaced)
    );
    assert_eq!(
        policies::organization(fixture.pool(), organization)
            .await
            .unwrap(),
        None,
        "a workspace policy never writes its organization's row"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn layers_read_whichever_policies_exist_in_one_call() {
    let fixture = Fixture::new().await;
    let organization = fixture.organization().await;
    let workspace = fixture.workspace(organization).await;
    let sibling = fixture.workspace(organization).await;
    let actor = fixture.user().await;

    assert_eq!(
        policies::layers(fixture.pool(), organization, Some(workspace))
            .await
            .unwrap(),
        Layers::default()
    );
    assert_eq!(
        policies::layers(fixture.pool(), organization, None)
            .await
            .unwrap(),
        Layers::default()
    );

    fixture
        .save_organization(organization, &organization_rules(), actor)
        .await
        .unwrap();
    let organization_only = Layers {
        organization: Some(organization_rules()),
        workspace: None,
    };
    assert_eq!(
        policies::layers(fixture.pool(), organization, Some(workspace))
            .await
            .unwrap(),
        organization_only
    );
    assert_eq!(
        policies::layers(fixture.pool(), organization, None)
            .await
            .unwrap(),
        organization_only
    );

    fixture
        .save_workspace(workspace, &workspace_rules(), actor)
        .await
        .unwrap();
    assert_eq!(
        policies::layers(fixture.pool(), organization, Some(workspace))
            .await
            .unwrap(),
        Layers {
            organization: Some(organization_rules()),
            workspace: Some(workspace_rules()),
        }
    );
    assert_eq!(
        policies::layers(fixture.pool(), organization, Some(sibling))
            .await
            .unwrap(),
        organization_only,
        "a workspace's policy never applies to its sibling"
    );
    assert_eq!(
        policies::layers(fixture.pool(), organization, None)
            .await
            .unwrap(),
        organization_only
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn deleting_a_workspace_removes_only_its_policy() {
    let fixture = Fixture::new().await;
    let organization = fixture.organization().await;
    let workspace = fixture.workspace(organization).await;
    let actor = fixture.user().await;
    fixture
        .save_organization(organization, &organization_rules(), actor)
        .await
        .unwrap();
    fixture
        .save_workspace(workspace, &workspace_rules(), actor)
        .await
        .unwrap();

    fixture.delete("workspaces", workspace).await;

    assert_eq!(
        policies::workspace(fixture.pool(), workspace)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        policies::organization(fixture.pool(), organization)
            .await
            .unwrap()
            .map(|stored| stored.rules),
        Some(organization_rules())
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn deleting_an_organization_removes_its_policy_and_its_workspaces_policies() {
    let fixture = Fixture::new().await;
    let organization = fixture.organization().await;
    let workspace = fixture.workspace(organization).await;
    let actor = fixture.user().await;
    fixture
        .save_organization(organization, &organization_rules(), actor)
        .await
        .unwrap();
    fixture
        .save_workspace(workspace, &workspace_rules(), actor)
        .await
        .unwrap();

    fixture.delete("organizations", organization).await;

    assert_eq!(
        policies::layers(fixture.pool(), organization, Some(workspace))
            .await
            .unwrap(),
        Layers::default()
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn locking_holds_the_policy_row_until_the_transaction_ends() {
    let fixture = Fixture::new().await;
    let organization = fixture.organization().await;
    let workspace = fixture.workspace(organization).await;
    let actor = fixture.user().await;

    let mut transaction = fixture.pool().begin().await.unwrap();
    assert_eq!(
        policies::lock_organization(&mut transaction, organization)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        policies::lock_workspace(&mut transaction, workspace)
            .await
            .unwrap(),
        None
    );
    transaction.rollback().await.unwrap();

    fixture
        .save_organization(organization, &organization_rules(), actor)
        .await
        .unwrap();
    fixture
        .save_workspace(workspace, &workspace_rules(), actor)
        .await
        .unwrap();

    let mut transaction = fixture.pool().begin().await.unwrap();
    let locked_organization = policies::lock_organization(&mut transaction, organization)
        .await
        .unwrap()
        .expect("the organization policy exists");
    assert_eq!(locked_organization.rules, organization_rules());
    let locked_workspace = policies::lock_workspace(&mut transaction, workspace)
        .await
        .unwrap()
        .expect("the workspace policy exists");
    assert_eq!(locked_workspace.rules, workspace_rules());

    let blocked = sqlx::query(
        "SELECT 1 FROM organization_policies WHERE organization_id = $1 FOR UPDATE NOWAIT",
    )
    .bind(organization)
    .execute(fixture.pool())
    .await
    .expect_err("the organization row is locked");
    assert_eq!(
        blocked
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some(LOCK_NOT_AVAILABLE),
        "{blocked}"
    );
    let blocked =
        sqlx::query("SELECT 1 FROM workspace_policies WHERE workspace_id = $1 FOR UPDATE NOWAIT")
            .bind(workspace)
            .execute(fixture.pool())
            .await
            .expect_err("the workspace row is locked");
    assert_eq!(
        blocked
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some(LOCK_NOT_AVAILABLE),
        "{blocked}"
    );

    let saved =
        policies::save_organization(&mut transaction, organization, &replaced_rules(), actor)
            .await
            .unwrap();
    assert_eq!(saved.rules, replaced_rules());
    transaction.commit().await.unwrap();

    sqlx::query("SELECT 1 FROM organization_policies WHERE organization_id = $1 FOR UPDATE NOWAIT")
        .bind(organization)
        .execute(fixture.pool())
        .await
        .expect("the commit releases the lock");
    assert_eq!(
        policies::organization(fixture.pool(), organization)
            .await
            .unwrap()
            .map(|stored| stored.rules),
        Some(replaced_rules())
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn deleting_the_editor_keeps_the_policy_without_an_author() {
    let fixture = Fixture::new().await;
    let organization = fixture.organization().await;
    let workspace = fixture.workspace(organization).await;
    let actor = fixture.user().await;
    fixture
        .save_organization(organization, &organization_rules(), actor)
        .await
        .unwrap();
    fixture
        .save_workspace(workspace, &workspace_rules(), actor)
        .await
        .unwrap();

    fixture.delete("users", actor).await;

    let organization_policy = policies::organization(fixture.pool(), organization)
        .await
        .unwrap()
        .expect("the organization policy survives its editor");
    assert_eq!(organization_policy.rules, organization_rules());
    assert_eq!(organization_policy.updated_by, None);
    let workspace_policy = policies::workspace(fixture.pool(), workspace)
        .await
        .unwrap()
        .expect("the workspace policy survives its editor");
    assert_eq!(workspace_policy.rules, workspace_rules());
    assert_eq!(workspace_policy.updated_by, None);

    fixture.cleanup().await;
}

#[tokio::test]
async fn rules_that_are_not_an_object_are_refused() {
    let fixture = Fixture::new().await;
    let organization = fixture.organization().await;
    let workspace = fixture.workspace(organization).await;
    let actor = fixture.user().await;

    for rules in [json!([]), json!("allow"), json!(1), Value::Null] {
        let error = fixture
            .save_organization(organization, &rules, actor)
            .await
            .expect_err("an organization's rules must be an object");
        assert_eq!(
            violated_constraint(&error, CHECK_VIOLATION).as_deref(),
            Some(ORGANIZATION_RULES_CHECK),
            "{rules}"
        );
        let error = fixture
            .save_workspace(workspace, &rules, actor)
            .await
            .expect_err("a workspace's rules must be an object");
        assert_eq!(
            violated_constraint(&error, CHECK_VIOLATION).as_deref(),
            Some(WORKSPACE_RULES_CHECK),
            "{rules}"
        );
    }

    assert_eq!(
        policies::layers(fixture.pool(), organization, Some(workspace))
            .await
            .unwrap(),
        Layers::default()
    );

    fixture.cleanup().await;
}
