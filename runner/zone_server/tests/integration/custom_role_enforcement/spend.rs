//! Spend reports and budgets held by custom organization roles: `spend:read`
//! sees them, `spend:update` alongside it changes budgets, and the spend
//! license gate is independent of the custom roles one.

use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_server::auth::{Permission, Scope};

use super::{Call, describe, holding_organization, licensed, organization_holder, role};
use crate::common::TestResponse;
use crate::common::policy::Fixture;
use crate::common::spend::{budget_path, budgets_path, exports_path, reports_path};
use crate::common::sso::{self, Account};

const FORBIDDEN: &str = "Only organization owners and admins can see spend or change budgets";
const ORGANIZATION_MISSING: &str = "Organization not found";
const LIMIT: i64 = 50_000_000;
const STEWARD: [Permission; 2] = [Permission::SpendRead, Permission::SpendUpdate];

fn organization_wide(limit: i64) -> Value {
    json!({ "scope": "organization", "limit_microdollars": limit })
}

fn reads(fixture: &Fixture) -> [Call; 3] {
    let organization = fixture.organization;
    [
        Call::get(reports_path(organization)),
        Call::get(exports_path(organization)),
        Call::get(budgets_path(organization)),
    ]
}

fn changes(fixture: &Fixture, budget: Uuid) -> [Call; 3] {
    let organization = fixture.organization;
    [
        Call::post(budgets_path(organization), organization_wide(LIMIT)),
        Call::patch(budget_path(organization, budget), json!({ "hard": true })),
        Call::delete(budget_path(organization, budget)),
    ]
}

async fn budget(fixture: &Fixture) -> Uuid {
    let call = Call::post(budgets_path(fixture.organization), organization_wide(LIMIT));
    let response = call.send(fixture, &fixture.owner.token).await;
    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "{}",
        describe(&call, &response)
    );
    response.json_value()["id"]
        .as_str()
        .expect("a budget id")
        .parse()
        .expect("a budget id is a UUID")
}

fn refused(call: &Call, response: &TestResponse, status: StatusCode, message: &str) {
    assert_eq!(response.status, status, "{}", describe(call, response));
    assert_eq!(
        response.json_value(),
        json!({ "error": message }),
        "{}",
        describe(call, response)
    );
}

async fn editable(fixture: &Fixture, account: &Account) -> [Value; 2] {
    let organization = fixture.organization;
    let mut answers = Vec::new();
    for call in [
        Call::get(reports_path(organization)),
        Call::get(budgets_path(organization)),
    ] {
        let response = call.send(fixture, &account.token).await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{}",
            describe(&call, &response)
        );
        answers.push(response.json_value()["editable"].clone());
    }
    answers.try_into().expect("two answers")
}

#[tokio::test]
async fn a_spend_reader_sees_reports_exports_and_budgets_and_changes_none() {
    let fixture = licensed().await;
    let budget = budget(&fixture).await;
    let reader = organization_holder(&fixture, &[Permission::SpendRead]).await;

    for call in reads(&fixture) {
        let response = call.send(&fixture, &reader.token).await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{}",
            describe(&call, &response)
        );
    }
    assert_eq!(
        editable(&fixture, &reader).await,
        [json!(false), json!(false)]
    );
    for call in changes(&fixture, budget) {
        let response = call.send(&fixture, &reader.token).await;
        refused(&call, &response, StatusCode::FORBIDDEN, FORBIDDEN);
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_budget_editor_who_sees_spend_creates_changes_and_deletes_budgets() {
    let fixture = licensed().await;
    let editor = organization_holder(&fixture, &STEWARD).await;
    let organization = fixture.organization;

    let creation = Call::post(budgets_path(organization), organization_wide(LIMIT));
    let created = creation.send(&fixture, &editor.token).await;
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "{}",
        describe(&creation, &created)
    );
    let budget = created.json_value()["id"]
        .as_str()
        .expect("a budget id")
        .to_string();
    let change = Call::patch(budget_path(organization, &budget), json!({ "hard": true }));
    let changed = change.send(&fixture, &editor.token).await;
    assert_eq!(
        changed.status,
        StatusCode::OK,
        "{}",
        describe(&change, &changed)
    );
    assert_eq!(changed.json_value()["hard"], true);
    let removal = Call::delete(budget_path(organization, &budget));
    let removed = removal.send(&fixture, &editor.token).await;
    assert_eq!(
        removed.status,
        StatusCode::NO_CONTENT,
        "{}",
        describe(&removal, &removed)
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_stored_budget_editor_without_spend_changes_and_reads_nothing() {
    let fixture = licensed().await;
    let budget = budget(&fixture).await;
    let held = role(
        fixture.pool(),
        fixture.organization,
        fixture.owner.id,
        Scope::Organization,
        &[],
    )
    .await;
    sqlx::query("INSERT INTO custom_role_permissions (role_id, permission) VALUES ($1, $2)")
        .bind(held)
        .bind(Permission::SpendUpdate.name())
        .execute(fixture.pool())
        .await
        .expect("a row from before spend:update needed spend:read");
    let editor = holding_organization(&fixture, held).await;

    for call in reads(&fixture).into_iter().chain(changes(&fixture, budget)) {
        let response = call.send(&fixture, &editor.token).await;
        refused(&call, &response, StatusCode::FORBIDDEN, FORBIDDEN);
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_holder_of_both_reads_spend_as_editable() {
    let fixture = licensed().await;
    let steward = organization_holder(&fixture, &STEWARD).await;

    assert_eq!(
        editable(&fixture, &steward).await,
        [json!(true), json!(true)]
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_holder_of_neither_is_refused_every_spend_route() {
    let fixture = licensed().await;
    let budget = budget(&fixture).await;
    let auditor = organization_holder(&fixture, &[Permission::AuditLogsRead]).await;

    for call in reads(&fixture).into_iter().chain(changes(&fixture, budget)) {
        let response = call.send(&fixture, &auditor.token).await;
        refused(&call, &response, StatusCode::FORBIDDEN, FORBIDDEN);
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_membership_whose_role_grants_nothing_is_told_the_organization_does_not_exist() {
    let fixture = licensed().await;
    let budget = budget(&fixture).await;
    let (elsewhere, _) = fixture.another_organization(&fixture.owner).await;
    let foreign = role(
        fixture.pool(),
        elsewhere,
        fixture.owner.id,
        Scope::Organization,
        &STEWARD,
    )
    .await;
    let mis_scoped = role(
        fixture.pool(),
        fixture.organization,
        fixture.owner.id,
        Scope::Workspace,
        &[],
    )
    .await;

    for pointed in [foreign, mis_scoped] {
        let member = organization_holder(&fixture, &[Permission::SpendRead]).await;
        sqlx::query(
            "UPDATE organization_members SET custom_role_id = $3 \
             WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(fixture.organization)
        .bind(member.id)
        .bind(pointed)
        .execute(fixture.pool())
        .await
        .expect("the membership points at a role that grants nothing here");
        for call in reads(&fixture).into_iter().chain(changes(&fixture, budget)) {
            let response = call.send(&fixture, &member.token).await;
            refused(
                &call,
                &response,
                StatusCode::NOT_FOUND,
                ORGANIZATION_MISSING,
            );
        }
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_spend_reader_keeps_spend_on_a_team_license_that_has_no_custom_roles() {
    let fixture = licensed().await;
    let reader = organization_holder(&fixture, &[Permission::SpendRead]).await;
    let editor = organization_holder(&fixture, &STEWARD).await;
    fixture.install(&sso::team()).await;

    for call in reads(&fixture) {
        let response = call.send(&fixture, &reader.token).await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{}",
            describe(&call, &response)
        );
    }
    let report = Call::get(reports_path(fixture.organization))
        .send(&fixture, &reader.token)
        .await
        .json_value();
    assert_eq!(report["edition"], "team");
    assert_eq!(report["entitled"], true);
    let creation = Call::post(budgets_path(fixture.organization), organization_wide(LIMIT));
    let created = creation.send(&fixture, &editor.token).await;
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "{}",
        describe(&creation, &created)
    );

    fixture.cleanup().await;
}
