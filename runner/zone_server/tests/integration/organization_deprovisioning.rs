//! A member who leaves an organization leaves its workspaces with it.
//!
//! Removal suspends every workspace membership the member held in the
//! organization, recording the roles so a suspension can be resumed, and seats
//! an owner in each workspace they leave without one. It also stops what they
//! have running there: chat turns (reminder and automation turns included,
//! since those register under the member too), task runs they triggered, and
//! invitations still waiting on their email.

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use zone_server::db::audit::actions;
use zone_server::db::{invitations, organization_members, suspensions};
use zone_server::services::members;

use crate::common::{TestClient, create_test_pool, test_email, test_password};

const PATIENCE: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(20);
const BLOCKED: &str =
    "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid)))";

struct Account {
    token: String,
    user: Uuid,
    email: String,
}

async fn account(client: &TestClient) -> Account {
    let email = test_email();
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": email, "password": test_password() }),
        )
        .await;
    let body = response.json_value();
    Account {
        token: body["access_token"].as_str().expect("token").to_string(),
        user: body["user"]["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .expect("user"),
        email,
    }
}

struct Tenant {
    organization: Uuid,
    workspace: Uuid,
}

async fn tenant(client: &TestClient, owner: &Account) -> Tenant {
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Tenant", "slug": Uuid::new_v4().to_string() }),
            &owner.token,
        )
        .await;
    let organization = response.json_value()["organization"]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .expect("organization");
    let workspace = workspace(client, organization, owner).await;
    Tenant {
        organization,
        workspace,
    }
}

async fn workspace(client: &TestClient, organization: Uuid, owner: &Account) -> Uuid {
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({ "name": "Workspace", "slug": Uuid::new_v4().to_string() }),
            &owner.token,
        )
        .await;
    response.json_value()["workspace"]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .expect("workspace")
}

async fn join(client: &TestClient, owner: &Account, organization: Uuid, member: &Account) {
    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": member.user, "role": "member" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);
}

async fn seat(client: &TestClient, owner: &Account, workspace: Uuid, member: &Account, role: &str) {
    client
        .post_json_auth(
            &format!("/api/workspaces/{workspace}/members"),
            &json!({ "user_id": member.user, "role": role }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);
}

async fn remove(client: &TestClient, owner: &Account, organization: Uuid, member: &Account) {
    client
        .delete_auth(
            &format!("/api/organizations/{organization}/members/{}", member.user),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::NO_CONTENT);
}

/// Makes `member` the workspace's only owner: the creator leaves the
/// workspace, though not the organization.
async fn hand_over(pool: &PgPool, workspace: Uuid, creator: &Account, member: &Account) {
    sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
        .bind(workspace)
        .bind(creator.user)
        .execute(pool)
        .await
        .expect("the creator leaves the workspace");
    sqlx::query(
        "UPDATE workspace_members SET role = 'owner' WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(member.user)
    .execute(pool)
    .await
    .expect("the member owns the workspace");
}

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct Held {
    role: String,
    custom_role_id: Option<Uuid>,
    is_active: bool,
}

async fn held(pool: &PgPool, workspace: Uuid, user: Uuid) -> Option<Held> {
    sqlx::query_as(
        "SELECT role, custom_role_id, is_active FROM workspace_members \
         WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(user)
    .fetch_optional(pool)
    .await
    .expect("the workspace membership")
}

async fn held_in_organization(pool: &PgPool, organization: Uuid, user: Uuid) -> Option<Held> {
    sqlx::query_as(
        "SELECT role, custom_role_id, is_active FROM organization_members \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization)
    .bind(user)
    .fetch_optional(pool)
    .await
    .expect("the organization membership")
}

async fn owners(pool: &PgPool, workspace: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar(
        "SELECT user_id FROM workspace_members \
         WHERE workspace_id = $1 AND is_active AND role = 'owner' ORDER BY user_id",
    )
    .bind(workspace)
    .fetch_all(pool)
    .await
    .expect("the workspace's owners")
}

async fn seated_audits(pool: &PgPool, workspace: Uuid) -> Vec<(Option<Uuid>, serde_json::Value)> {
    sqlx::query_as(
        "SELECT actor_id, new_values FROM audit_logs \
         WHERE action = $1 AND resource_id = $2 ORDER BY created_at",
    )
    .bind(actions::WORKSPACE_OWNER_SEATED)
    .bind(workspace)
    .fetch_all(pool)
    .await
    .expect("the seating audit rows")
}

async fn custom_role(pool: &PgPool, organization: Uuid, scope: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO custom_roles (organization_id, name, scope) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(organization)
    .bind(Uuid::new_v4().to_string())
    .bind(scope)
    .fetch_one(pool)
    .await
    .expect("a custom role")
}

#[tokio::test]
async fn a_member_removed_from_the_organization_loses_its_workspaces() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let member = account(&client).await;
    let tenant = tenant(&client, &owner).await;
    join(&client, &owner, tenant.organization, &member).await;
    seat(&client, &owner, tenant.workspace, &member, "member").await;
    client
        .get_auth(
            &format!("/api/workspaces/{}", tenant.workspace),
            &member.token,
        )
        .await
        .assert_status(StatusCode::OK);

    remove(&client, &owner, tenant.organization, &member).await;

    client
        .get_auth(
            &format!("/api/workspaces/{}", tenant.workspace),
            &member.token,
        )
        .await
        .assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_workspaces_only_owner_leaving_seats_a_successor_by_the_backfill_rule() {
    let client = TestClient::with_db().await;
    let pool = create_test_pool().await;
    let owner = account(&client).await;
    let leaving = account(&client).await;
    let member = account(&client).await;
    let admin = account(&client).await;
    let lapsed = account(&client).await;
    let tenant = tenant(&client, &owner).await;
    for joining in [&leaving, &member, &admin, &lapsed] {
        join(&client, &owner, tenant.organization, joining).await;
    }
    seat(&client, &owner, tenant.workspace, &lapsed, "admin").await;
    seat(&client, &owner, tenant.workspace, &leaving, "member").await;
    seat(&client, &owner, tenant.workspace, &member, "member").await;
    seat(&client, &owner, tenant.workspace, &admin, "admin").await;
    hand_over(&pool, tenant.workspace, &owner, &leaving).await;
    sqlx::query(
        "UPDATE organization_members SET is_active = FALSE \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(tenant.organization)
    .bind(lapsed.user)
    .execute(&pool)
    .await
    .expect("an admin who is no longer in the organization");

    remove(&client, &owner, tenant.organization, &leaving).await;

    assert_eq!(
        owners(&pool, tenant.workspace).await,
        vec![admin.user],
        "the active admin should be seated, not the member or the admin who left the organization"
    );
    assert_eq!(
        held(&pool, tenant.workspace, member.user)
            .await
            .map(|held| held.role),
        Some("member".to_string())
    );
    assert!(
        !held(&pool, tenant.workspace, leaving.user)
            .await
            .expect("the suspended row is kept")
            .is_active
    );
    let audits = seated_audits(&pool, tenant.workspace).await;
    assert_eq!(
        audits,
        vec![(Some(owner.user), json!({ "user_id": admin.user }))],
        "the seating was not audited against its actor"
    );
}

#[tokio::test]
async fn nobody_left_to_seat_leaves_the_workspace_ownerless_and_the_removal_stands() {
    let client = TestClient::with_db().await;
    let pool = create_test_pool().await;
    let owner = account(&client).await;
    let leaving = account(&client).await;
    let tenant = tenant(&client, &owner).await;
    join(&client, &owner, tenant.organization, &leaving).await;
    seat(&client, &owner, tenant.workspace, &leaving, "member").await;
    hand_over(&pool, tenant.workspace, &owner, &leaving).await;

    remove(&client, &owner, tenant.organization, &leaving).await;

    assert!(owners(&pool, tenant.workspace).await.is_empty());
    assert!(seated_audits(&pool, tenant.workspace).await.is_empty());
    assert!(
        !held_in_organization(&pool, tenant.organization, leaving.user)
            .await
            .expect("the membership row is kept")
            .is_active
    );
}

#[tokio::test]
async fn removing_a_member_revokes_the_invitations_waiting_on_their_email() {
    let client = TestClient::with_db().await;
    let pool = create_test_pool().await;
    let owner = account(&client).await;
    let leaving = account(&client).await;
    let tenant = tenant(&client, &owner).await;
    join(&client, &owner, tenant.organization, &leaving).await;
    let (revoked, _) = invitations::create_invitation(
        &pool,
        &leaving.email.to_uppercase(),
        tenant.organization,
        vec![tenant.workspace],
        "member",
        "member",
        owner.user,
    )
    .await
    .expect("an invitation to the leaving member");
    let (kept, _) = invitations::create_invitation(
        &pool,
        &test_email(),
        tenant.organization,
        vec![],
        "member",
        "member",
        owner.user,
    )
    .await
    .expect("an invitation to someone else");

    remove(&client, &owner, tenant.organization, &leaving).await;

    assert!(
        invitations::get_invitation_by_id(&pool, revoked.id)
            .await
            .unwrap()
            .is_none(),
        "the leaving member's invitation survived"
    );
    assert!(
        invitations::get_invitation_by_id(&pool, kept.id)
            .await
            .unwrap()
            .is_some(),
        "another person's invitation was revoked"
    );
}

#[tokio::test]
async fn removing_a_member_stops_the_task_runs_they_triggered() {
    let client = TestClient::with_db().await;
    let pool = create_test_pool().await;
    let owner = account(&client).await;
    let leaving = account(&client).await;
    let tenant = tenant(&client, &owner).await;
    join(&client, &owner, tenant.organization, &leaving).await;
    seat(&client, &owner, tenant.workspace, &leaving, "member").await;
    let task: Uuid = sqlx::query_scalar(
        "INSERT INTO tasks (workspace_id, title, description, status) \
         VALUES ($1, 'Running', 'Running', 'in_progress') RETURNING id",
    )
    .bind(tenant.workspace)
    .fetch_one(&pool)
    .await
    .expect("a task");
    let run: Uuid = sqlx::query_scalar(
        "INSERT INTO task_runs (task_id, status, triggered_by) VALUES ($1, 'running', $2) RETURNING id",
    )
    .bind(task)
    .bind(leaving.user)
    .fetch_one(&pool)
    .await
    .expect("a run the member triggered");
    sqlx::query("UPDATE tasks SET active_run_id = $2 WHERE id = $1")
        .bind(task)
        .bind(run)
        .execute(&pool)
        .await
        .expect("the run holds its task");

    remove(&client, &owner, tenant.organization, &leaving).await;

    let status: String = sqlx::query_scalar("SELECT status FROM task_runs WHERE id = $1")
        .bind(run)
        .fetch_one(&pool)
        .await
        .expect("the run");
    assert_eq!(status, "cancelled", "the removed member's run kept running");
}

#[tokio::test]
async fn suspending_then_resuming_gives_back_every_role_held() {
    let client = TestClient::with_db().await;
    let pool = create_test_pool().await;
    let owner = account(&client).await;
    let leaving = account(&client).await;
    let colleague = account(&client).await;
    let tenant = tenant(&client, &owner).await;
    let owned = workspace(&client, tenant.organization, &owner).await;
    join(&client, &owner, tenant.organization, &leaving).await;
    join(&client, &owner, tenant.organization, &colleague).await;
    seat(&client, &owner, tenant.workspace, &leaving, "viewer").await;
    seat(&client, &owner, owned, &leaving, "member").await;
    seat(&client, &owner, owned, &colleague, "member").await;
    hand_over(&pool, owned, &owner, &leaving).await;
    let organization_role = custom_role(&pool, tenant.organization, "organization").await;
    let workspace_role = custom_role(&pool, tenant.organization, "workspace").await;
    sqlx::query(
        "UPDATE organization_members SET custom_role_id = $3 \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(tenant.organization)
    .bind(leaving.user)
    .bind(organization_role)
    .execute(&pool)
    .await
    .expect("the member holds an organization role");
    sqlx::query(
        "UPDATE workspace_members SET custom_role_id = $3 WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(tenant.workspace)
    .bind(leaving.user)
    .bind(workspace_role)
    .execute(&pool)
    .await
    .expect("the member holds a workspace role");

    let mut transaction = pool.begin().await.unwrap();
    let suspension = suspensions::suspend(&mut transaction, tenant.organization, leaving.user)
        .await
        .expect("the member is suspended");
    transaction.commit().await.unwrap();

    let mut suspended = vec![tenant.workspace, owned];
    suspended.sort();
    assert_eq!(suspension.workspaces, suspended);
    assert_eq!(
        suspension.successors,
        vec![suspensions::Successor {
            workspace_id: owned,
            user_id: colleague.user,
        }]
    );
    assert_eq!(
        held(&pool, tenant.workspace, leaving.user).await,
        Some(Held {
            role: "viewer".into(),
            custom_role_id: None,
            is_active: false,
        })
    );

    let mut transaction = pool.begin().await.unwrap();
    let resumed = suspensions::resume(&mut transaction, tenant.organization, leaving.user)
        .await
        .expect("the member is resumed");
    transaction.commit().await.unwrap();

    assert!(resumed.organization);
    assert_eq!(resumed.workspaces, suspended);
    assert_eq!(
        held_in_organization(&pool, tenant.organization, leaving.user).await,
        Some(Held {
            role: "member".into(),
            custom_role_id: Some(organization_role),
            is_active: true,
        })
    );
    assert_eq!(
        held(&pool, tenant.workspace, leaving.user).await,
        Some(Held {
            role: "viewer".into(),
            custom_role_id: Some(workspace_role),
            is_active: true,
        })
    );
    let mut both = vec![leaving.user, colleague.user];
    both.sort();
    assert_eq!(
        owners(&pool, owned).await,
        both,
        "the returning owner should sit beside the successor"
    );
    assert_eq!(
        suspensions::forget(&pool, tenant.organization, leaving.user)
            .await
            .unwrap(),
        0,
        "the records outlived the resumption"
    );
}

#[tokio::test]
async fn a_resumed_member_whose_roles_are_gone_falls_back_to_the_built_in_ones() {
    let client = TestClient::with_db().await;
    let pool = create_test_pool().await;
    let owner = account(&client).await;
    let leaving = account(&client).await;
    let tenant = tenant(&client, &owner).await;
    join(&client, &owner, tenant.organization, &leaving).await;
    seat(&client, &owner, tenant.workspace, &leaving, "viewer").await;
    let organization_role = custom_role(&pool, tenant.organization, "organization").await;
    let workspace_role = custom_role(&pool, tenant.organization, "workspace").await;
    sqlx::query(
        "UPDATE organization_members SET custom_role_id = $3 \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(tenant.organization)
    .bind(leaving.user)
    .bind(organization_role)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE workspace_members SET custom_role_id = $3 WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(tenant.workspace)
    .bind(leaving.user)
    .bind(workspace_role)
    .execute(&pool)
    .await
    .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    suspensions::suspend(&mut transaction, tenant.organization, leaving.user)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    sqlx::query("UPDATE custom_roles SET scope = 'workspace' WHERE id = $1")
        .bind(organization_role)
        .execute(&pool)
        .await
        .expect("the organization role is no longer organization-scoped");
    sqlx::query("DELETE FROM custom_roles WHERE id = $1")
        .bind(workspace_role)
        .execute(&pool)
        .await
        .expect("the workspace role is deleted");

    let mut transaction = pool.begin().await.unwrap();
    suspensions::resume(&mut transaction, tenant.organization, leaving.user)
        .await
        .unwrap();
    transaction.commit().await.unwrap();

    assert_eq!(
        held_in_organization(&pool, tenant.organization, leaving.user).await,
        Some(Held {
            role: "member".into(),
            custom_role_id: None,
            is_active: true,
        })
    );
    assert_eq!(
        held(&pool, tenant.workspace, leaving.user).await,
        Some(Held {
            role: "viewer".into(),
            custom_role_id: None,
            is_active: true,
        })
    );
}

#[tokio::test]
async fn a_removed_member_added_back_starts_without_their_old_workspaces() {
    let client = TestClient::with_db().await;
    let pool = create_test_pool().await;
    let owner = account(&client).await;
    let member = account(&client).await;
    let tenant = tenant(&client, &owner).await;
    join(&client, &owner, tenant.organization, &member).await;
    seat(&client, &owner, tenant.workspace, &member, "member").await;
    remove(&client, &owner, tenant.organization, &member).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{}/members", tenant.organization),
            &json!({ "user_id": member.user, "role": "member" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);

    client
        .get_auth(
            &format!("/api/workspaces/{}", tenant.workspace),
            &member.token,
        )
        .await
        .assert_status(StatusCode::FORBIDDEN);
    assert_eq!(
        suspensions::forget(&pool, tenant.organization, member.user)
            .await
            .unwrap(),
        0,
        "re-adding the member kept their suspension records"
    );
}

#[tokio::test]
async fn a_removed_member_accepting_an_invitation_starts_without_their_old_workspaces() {
    let client = TestClient::with_db().await;
    let pool = create_test_pool().await;
    let owner = account(&client).await;
    let member = account(&client).await;
    let tenant = tenant(&client, &owner).await;
    let invited = workspace(&client, tenant.organization, &owner).await;
    join(&client, &owner, tenant.organization, &member).await;
    seat(&client, &owner, tenant.workspace, &member, "member").await;
    remove(&client, &owner, tenant.organization, &member).await;
    let (_, token) = invitations::create_invitation(
        &pool,
        &member.email,
        tenant.organization,
        vec![invited],
        "member",
        "member",
        owner.user,
    )
    .await
    .expect("an invitation back");

    client
        .post_json_auth(
            &format!("/api/invitations/{token}/accept"),
            &json!({}),
            &member.token,
        )
        .await
        .assert_status(StatusCode::OK);

    client
        .get_auth(&format!("/api/workspaces/{invited}"), &member.token)
        .await
        .assert_status(StatusCode::OK);
    client
        .get_auth(
            &format!("/api/workspaces/{}", tenant.workspace),
            &member.token,
        )
        .await
        .assert_status(StatusCode::FORBIDDEN);
    assert_eq!(
        suspensions::forget(&pool, tenant.organization, member.user)
            .await
            .unwrap(),
        0,
        "accepting the invitation kept the suspension records"
    );
}

#[tokio::test]
async fn deprovisioning_answers_the_members_token_with_unauthorized() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let member = account(&client).await;
    let tenant = tenant(&client, &owner).await;
    join(&client, &owner, tenant.organization, &member).await;
    client
        .get_auth("/api/organizations", &member.token)
        .await
        .assert_status(StatusCode::OK);

    let deprovisioned = members::deprovision(client.state(), tenant.organization, member.user)
        .await
        .expect("the member is deprovisioned");

    assert!(deprovisioned.sessions >= 1, "no session was revoked");
    client
        .get_auth("/api/organizations", &member.token)
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    client
        .get_auth("/api/organizations", &owner.token)
        .await
        .assert_status(StatusCode::OK);
}

/// A workspace removal holding the workspace's admin and owner rows makes a
/// suspension of the workspace's owner wait for it, and the suspension then
/// seats an owner from what the removal left, without either deadlocking.
#[tokio::test]
async fn a_suspension_waits_for_a_workspace_removal_and_still_seats_an_owner() {
    let client = TestClient::with_db().await;
    let pool = create_test_pool().await;
    let owner = account(&client).await;
    let leaving = account(&client).await;
    let admin = account(&client).await;
    let member = account(&client).await;
    let tenant = tenant(&client, &owner).await;
    for joining in [&leaving, &admin, &member] {
        join(&client, &owner, tenant.organization, joining).await;
    }
    seat(&client, &owner, tenant.workspace, &leaving, "member").await;
    seat(&client, &owner, tenant.workspace, &admin, "admin").await;
    seat(&client, &owner, tenant.workspace, &member, "member").await;
    hand_over(&pool, tenant.workspace, &owner, &leaving).await;

    let mut removal = pool.begin().await.unwrap();
    let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *removal)
        .await
        .unwrap();
    sqlx::query(
        "SELECT user_id FROM workspace_members \
         WHERE workspace_id = $1 AND is_active = TRUE AND (role = 'admin' OR role = 'owner') \
         FOR UPDATE",
    )
    .bind(tenant.workspace)
    .execute(&mut *removal)
    .await
    .expect("the removal holds the privileged rows");

    let suspending = tokio::spawn({
        let pool = pool.clone();
        let organization = tenant.organization;
        let user = leaving.user;
        async move { organization_members::remove_member(&pool, organization, user).await }
    });
    let waited = tokio::time::timeout(PATIENCE, async {
        loop {
            let blocked: bool = sqlx::query_scalar(BLOCKED)
                .bind(holder)
                .fetch_one(&pool)
                .await
                .unwrap();
            if blocked {
                break;
            }
            tokio::time::sleep(POLL).await;
        }
    })
    .await;
    assert!(
        waited.is_ok(),
        "the suspension did not wait for the removal"
    );

    sqlx::query(
        "UPDATE workspace_members SET is_active = FALSE, updated_at = NOW() \
         WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(tenant.workspace)
    .bind(admin.user)
    .execute(&mut *removal)
    .await
    .expect("the removal lands");
    removal.commit().await.unwrap();

    let suspended = tokio::time::timeout(PATIENCE, suspending)
        .await
        .expect("the suspension deadlocked")
        .expect("the suspension task")
        .expect("the suspension");
    assert!(suspended);
    assert_eq!(
        owners(&pool, tenant.workspace).await,
        vec![member.user],
        "the workspace was left without the member the removal spared as owner"
    );
}
