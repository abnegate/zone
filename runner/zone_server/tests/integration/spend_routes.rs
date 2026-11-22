//! The spend routes: who may see reports and change budgets, how the license gates each, what a
//! report and an export contain, and what every change audits.

use std::time::Duration;

use axum::http::StatusCode;
use chrono::{DateTime, Datelike, NaiveDate, TimeDelta, Utc};
use serde_json::{Value, json};
use uuid::Uuid;
use zone_enterprise::spend::{
    BODY_INVALID, BUDGET_EXISTS, LIMIT_INVALID, Month, RANGE_INVALID, SCOPE_INVALID,
    THRESHOLD_INVALID,
};
use zone_server::auth::entitlement::TEAM_MESSAGE;
use zone_server::db::audit::{actions, resources};
use zone_server::db::organization_members::OrgRole;
use zone_server::db::spend::cursor;

use crate::common::TestResponse;
use crate::common::spend::{Fixture, Spend, budget_path, budgets_path, exports_path, reports_path};
use crate::common::sso::grace;

const PREFIX: &str = "spend_routes";
const LICENSE_REQUIRED: &str = "license_required";
const FORBIDDEN: &str = "Only organization owners and admins can see spend or change budgets";
const ORGANIZATION_MISSING: &str = "Organization not found";
const BUDGET_MISSING: &str = "Budget not found";
const LIMIT: i64 = 50_000_000;
const OCTOBER: &str = "from=2026-10-01&to=2026-10-03";
const CSV_HEADER: &str = "date,workspace_id,workspace,user_id,user_email,automated,model,provider,requests,prompt_tokens,completion_tokens,spend_usd\r\n";
const CSV_TYPE: &str = "text/csv; charset=utf-8";

fn at(text: &str) -> DateTime<Utc> {
    text.parse().unwrap()
}

fn organization_wide(limit: i64) -> Value {
    json!({ "scope": "organization", "limit_microdollars": limit })
}

fn report_of(fixture: &Fixture, query: &str) -> String {
    format!("{}?{query}", reports_path(fixture.policy.organization))
}

fn export_of(fixture: &Fixture, query: &str) -> String {
    format!("{}?{query}", exports_path(fixture.policy.organization))
}

fn usage(spend: i64, requests: i64) -> Value {
    json!({
        "spend_microdollars": spend,
        "requests": requests,
        "prompt_tokens": requests * 100,
        "completion_tokens": requests * 20,
    })
}

fn with_usage(mut fields: Value, spend: i64, requests: i64) -> Value {
    let usage = usage(spend, requests);
    for (key, value) in usage.as_object().unwrap() {
        fields[key] = value.clone();
    }
    fields
}

fn refused(response: &TestResponse, status: StatusCode, code: &str) {
    assert_eq!(response.status, status, "{}", response.text());
    let body = response.json_value();
    assert_eq!(body["code"], code, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
}

fn unentitled(response: &TestResponse) {
    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "{}",
        response.text()
    );
    assert_eq!(
        response.json_value(),
        json!({ "error": TEAM_MESSAGE, "code": LICENSE_REQUIRED })
    );
}

async fn created(fixture: &Fixture, token: &str, body: &Value) -> Value {
    let response = fixture
        .post(&budgets_path(fixture.policy.organization), token, body)
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text());
    response.json_value()
}

fn identifier(body: &Value) -> Uuid {
    body["id"].as_str().unwrap().parse().unwrap()
}

async fn listed(fixture: &Fixture, token: &str) -> Value {
    let response = fixture
        .get(&budgets_path(fixture.policy.organization), token)
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text());
    response.json_value()
}

async fn reported(fixture: &Fixture, query: &str) -> Value {
    let response = fixture
        .get(&report_of(fixture, query), &fixture.policy.owner.token)
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text());
    response.json_value()
}

async fn total(fixture: &Fixture, query: &str) -> Value {
    reported(fixture, query).await["report"]["total"].clone()
}

/// Every spend route for `budget`, as `token`.
async fn every_route(fixture: &Fixture, budget: Uuid, token: &str) -> Vec<TestResponse> {
    let organization = fixture.policy.organization;
    vec![
        fixture.get(&reports_path(organization), token).await,
        fixture.get(&exports_path(organization), token).await,
        fixture.get(&budgets_path(organization), token).await,
        fixture
            .post(
                &budgets_path(organization),
                token,
                &organization_wide(LIMIT),
            )
            .await,
        fixture
            .patch(
                &budget_path(organization, budget),
                token,
                &json!({ "hard": true }),
            )
            .await,
        fixture
            .delete(&budget_path(organization, budget), token)
            .await,
    ]
}

async fn email(fixture: &Fixture, user: Uuid) -> String {
    sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(fixture.pool())
        .await
        .unwrap()
}

async fn exhausted_month(fixture: &Fixture, budget: Uuid) -> Option<NaiveDate> {
    sqlx::query_scalar("SELECT exhausted_month FROM budgets WHERE id = $1")
        .bind(budget)
        .fetch_one(fixture.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn only_owners_and_admins_reach_spend_and_strangers_are_told_it_does_not_exist() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let owner = &fixture.policy.owner;
    let budget = identifier(&created(&fixture, &owner.token, &organization_wide(LIMIT)).await);
    let member = fixture.policy.member(OrgRole::Member, None).await;
    let admin = fixture.policy.member(OrgRole::Admin, None).await;
    let stranger = fixture.policy.stranger().await;

    for (token, status, message) in [
        (&member.token, StatusCode::FORBIDDEN, FORBIDDEN),
        (&stranger.token, StatusCode::NOT_FOUND, ORGANIZATION_MISSING),
    ] {
        for response in every_route(&fixture, budget, token).await {
            assert_eq!(response.status, status, "{}", response.text());
            assert_eq!(response.json_value(), json!({ "error": message }));
        }
    }
    let organization = fixture.policy.organization;
    for path in [
        reports_path(organization),
        exports_path(organization),
        budgets_path(organization),
    ] {
        let response = fixture.get(&path, &admin.token).await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{path}: {}",
            response.text()
        );
    }
    assert_eq!(
        listed(&fixture, &admin.token).await["budgets"][0]["id"],
        json!(budget)
    );

    let malformed = fixture
        .get(
            "/api/organizations/not-an-organization/spend-reports",
            &owner.token,
        )
        .await;
    assert_eq!(malformed.status, StatusCode::NOT_FOUND);
    assert_eq!(
        malformed.json_value(),
        json!({ "error": ORGANIZATION_MISSING })
    );
    for response in [
        fixture
            .patch(
                &budget_path(organization, "not-a-budget"),
                &owner.token,
                &json!({}),
            )
            .await,
        fixture
            .delete(&budget_path(organization, Uuid::new_v4()), &owner.token)
            .await,
    ] {
        assert_eq!(
            response.status,
            StatusCode::NOT_FOUND,
            "{}",
            response.text()
        );
        assert_eq!(response.json_value(), json!({ "error": BUDGET_MISSING }));
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_team_license_reports_spend_and_changes_budgets() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let owner = &fixture.policy.owner;
    fixture.record(fixture.spend()).await;

    let report = reported(&fixture, "").await;
    assert_eq!(report["edition"], "team");
    assert_eq!(report["status"], "active");
    assert_eq!(report["enforced"], true);
    assert_eq!(report["entitled"], true);
    assert_eq!(report["editable"], true);
    assert_eq!(report["report"]["total"], usage(1_000_000, 1));
    let budget = created(&fixture, &owner.token, &organization_wide(LIMIT)).await;
    assert_eq!(budget["limit_microdollars"], LIMIT);
    assert_eq!(budget["spent_microdollars"], 1_000_000);
    let budgets = listed(&fixture, &owner.token).await;
    assert_eq!(budgets["editable"], true);
    assert_eq!(budgets["budgets"][0]["id"], budget["id"]);
    fixture.cleanup().await;
}

#[tokio::test]
async fn without_a_license_nothing_is_reported_and_every_change_is_refused() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let owner = &fixture.policy.owner;
    let budget = identifier(&created(&fixture, &owner.token, &organization_wide(LIMIT)).await);
    fixture.policy.sso.remove_license(owner).await;
    let organization = fixture.policy.organization;

    assert_eq!(
        reported(&fixture, "").await,
        json!({
            "entitled": false,
            "enforced": false,
            "edition": "community",
            "status": "community",
            "editable": false,
            "report": null,
        })
    );
    unentitled(&fixture.get(&exports_path(organization), &owner.token).await);
    refused(
        &fixture
            .get(&export_of(&fixture, "from=October"), &owner.token)
            .await,
        StatusCode::BAD_REQUEST,
        RANGE_INVALID,
    );
    let budgets = listed(&fixture, &owner.token).await;
    assert_eq!(budgets["entitled"], false);
    assert_eq!(budgets["editable"], false);
    assert_eq!(budgets["month"], Month::current().label());
    assert_eq!(budgets["budgets"][0]["id"], json!(budget));
    unentitled(
        &fixture
            .post(
                &budgets_path(organization),
                &owner.token,
                &organization_wide(1),
            )
            .await,
    );
    unentitled(
        &fixture
            .patch(
                &budget_path(organization, budget),
                &owner.token,
                &json!({ "limit_microdollars": 1 }),
            )
            .await,
    );
    unentitled(
        &fixture
            .delete(&budget_path(organization, budget), &owner.token)
            .await,
    );
    let kept = listed(&fixture, &owner.token).await;
    assert_eq!(kept["budgets"].as_array().unwrap().len(), 1);
    assert_eq!(kept["budgets"][0]["limit_microdollars"], LIMIT);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_keeps_reports_and_exports_but_refuses_changes() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let owner = &fixture.policy.owner;
    let budget = identifier(&created(&fixture, &owner.token, &organization_wide(LIMIT)).await);
    fixture.record(fixture.spend()).await;
    fixture.lapsed().await;
    let organization = fixture.policy.organization;

    let report = reported(&fixture, "").await;
    assert_eq!(report["entitled"], false);
    assert_eq!(report["enforced"], true);
    assert_eq!(report["status"], "lapsed");
    assert_eq!(report["editable"], false);
    assert_eq!(report["report"]["total"], usage(1_000_000, 1));
    let export = fixture.get(&exports_path(organization), &owner.token).await;
    assert_eq!(export.status, StatusCode::OK, "{}", export.text());
    let csv = export.text();
    let rows: Vec<&str> = csv
        .strip_prefix(CSV_HEADER)
        .expect("the export starts with its header")
        .lines()
        .collect();
    assert_eq!(rows.len(), 1, "{csv}");
    assert!(
        rows[0].contains(&format!(
            ",{},{},false,gpt-4o,openai,1,100,20,1.000000",
            owner.id,
            email(&fixture, owner.id).await
        )),
        "{csv}"
    );
    let budgets = listed(&fixture, &owner.token).await;
    assert_eq!(budgets["editable"], false);
    assert_eq!(budgets["budgets"][0]["spent_microdollars"], 1_000_000);
    unentitled(
        &fixture
            .post(
                &budgets_path(organization),
                &owner.token,
                &organization_wide(1),
            )
            .await,
    );
    unentitled(
        &fixture
            .patch(
                &budget_path(organization, budget),
                &owner.token,
                &json!({ "hard": true }),
            )
            .await,
    );
    unentitled(
        &fixture
            .delete(&budget_path(organization, budget), &owner.token)
            .await,
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_license_in_its_grace_period_still_changes_budgets() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.policy.install(&grace()).await;
    let owner = &fixture.policy.owner;
    let organization = fixture.policy.organization;

    let budgets = listed(&fixture, &owner.token).await;
    assert_eq!(budgets["status"], "grace");
    assert_eq!(budgets["entitled"], true);
    assert_eq!(budgets["editable"], true);
    let budget = identifier(&created(&fixture, &owner.token, &organization_wide(LIMIT)).await);
    let patched = fixture
        .patch(
            &budget_path(organization, budget),
            &owner.token,
            &json!({ "hard": true }),
        )
        .await;
    assert_eq!(patched.status, StatusCode::OK, "{}", patched.text());
    let deleted = fixture
        .delete(&budget_path(organization, budget), &owner.token)
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_budget_is_created_changed_and_deleted_and_each_change_is_audited() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let owner = &fixture.policy.owner;
    let organization = fixture.policy.organization;
    let workspace = fixture.policy.workspace;
    fixture
        .record(Spend {
            microdollars: 250_000,
            ..fixture.spend()
        })
        .await;

    let budget = created(
        &fixture,
        &owner.token,
        &json!({
            "scope": "workspace",
            "workspace_id": workspace,
            "limit_microdollars": 1_000_000,
            "hard": true,
        }),
    )
    .await;
    let id = identifier(&budget);
    let updated_at = budget["updated_at"].clone();
    assert!(updated_at.as_str().is_some());
    assert_eq!(
        budget,
        json!({
            "id": id,
            "scope": "workspace",
            "workspace_id": workspace,
            "workspace_name": "Policy",
            "user_id": null,
            "user_name": null,
            "user_email": null,
            "limit_microdollars": 1_000_000,
            "threshold_percent": 80,
            "hard": true,
            "spent_microdollars": 250_000,
            "state": "under",
            "updated_at": updated_at,
        })
    );
    let creations = fixture.policy.audits(actions::BUDGET_CREATED).await;
    assert_eq!(creations.len(), 1);
    assert_eq!(creations[0].actor_id, Some(owner.id));
    assert_eq!(creations[0].organization_id, Some(organization));
    assert_eq!(creations[0].resource_type, resources::BUDGET);
    assert_eq!(creations[0].resource_id, Some(id));
    assert_eq!(creations[0].old_values, None);
    let settings = json!({
        "scope": "workspace",
        "workspace_id": workspace,
        "user_id": null,
        "limit_microdollars": 1_000_000,
        "threshold_percent": 80,
        "hard": true,
    });
    assert_eq!(creations[0].new_values, Some(settings.clone()));
    assert_eq!(
        listed(&fixture, &owner.token).await["budgets"],
        json!([budget])
    );

    let path = budget_path(organization, id);
    let warned = fixture
        .patch(&path, &owner.token, &json!({ "threshold_percent": 20 }))
        .await;
    assert_eq!(warned.status, StatusCode::OK, "{}", warned.text());
    let warned = warned.json_value();
    assert_eq!(warned["threshold_percent"], 20);
    assert_eq!(warned["state"], "warning");
    let updates = fixture.policy.audits(actions::BUDGET_UPDATED).await;
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].old_values, Some(settings.clone()));
    let mut lowered = settings;
    lowered["threshold_percent"] = json!(20);
    assert_eq!(updates[0].new_values, Some(lowered.clone()));

    for unchanged in [json!({ "threshold_percent": 20, "hard": true }), json!({})] {
        let response = fixture.patch(&path, &owner.token, &unchanged).await;
        assert_eq!(response.status, StatusCode::OK, "{}", response.text());
        assert_eq!(response.json_value()["updated_at"], warned["updated_at"]);
    }
    assert_eq!(
        fixture.policy.audits(actions::BUDGET_UPDATED).await.len(),
        1
    );

    let deleted = fixture.delete(&path, &owner.token).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    let deletions = fixture.policy.audits(actions::BUDGET_DELETED).await;
    assert_eq!(deletions.len(), 1);
    assert_eq!(deletions[0].resource_id, Some(id));
    assert_eq!(deletions[0].old_values, Some(lowered));
    assert_eq!(deletions[0].new_values, None);
    assert_eq!(listed(&fixture, &owner.token).await["budgets"], json!([]));
    for response in [
        fixture.patch(&path, &owner.token, &json!({})).await,
        fixture.delete(&path, &owner.token).await,
    ] {
        assert_eq!(response.status, StatusCode::NOT_FOUND);
        assert_eq!(response.json_value(), json!({ "error": BUDGET_MISSING }));
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn every_invalid_budget_is_refused_with_its_code() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let owner = &fixture.policy.owner;
    let organization = fixture.policy.organization;
    let workspace = fixture.policy.workspace;
    let stranger = fixture.policy.stranger().await;
    let (_, foreign) = fixture.policy.another_organization(&stranger).await;
    let member = fixture.policy.member(OrgRole::Member, None).await;
    let path = budgets_path(organization);

    for (body, code) in [
        (
            json!({ "scope": "team", "limit_microdollars": 1 }),
            BODY_INVALID,
        ),
        (json!({ "scope": "organization" }), BODY_INVALID),
        (
            json!({ "scope": "organization", "limit_microdollars": 1, "color": "red" }),
            BODY_INVALID,
        ),
        (
            json!({ "scope": "organization", "limit_microdollars": "ten" }),
            BODY_INVALID,
        ),
        (organization_wide(0), LIMIT_INVALID),
        (organization_wide(-5), LIMIT_INVALID),
        (
            json!({ "scope": "organization", "limit_microdollars": 1, "threshold_percent": 0 }),
            THRESHOLD_INVALID,
        ),
        (
            json!({ "scope": "organization", "limit_microdollars": 1, "threshold_percent": 101 }),
            THRESHOLD_INVALID,
        ),
        (
            json!({ "scope": "organization", "limit_microdollars": 1, "workspace_id": workspace }),
            SCOPE_INVALID,
        ),
        (
            json!({ "scope": "organization", "limit_microdollars": 1, "user_id": owner.id }),
            SCOPE_INVALID,
        ),
        (
            json!({ "scope": "workspace", "limit_microdollars": 1 }),
            SCOPE_INVALID,
        ),
        (
            json!({
                "scope": "workspace",
                "limit_microdollars": 1,
                "workspace_id": workspace,
                "user_id": owner.id,
            }),
            SCOPE_INVALID,
        ),
        (
            json!({ "scope": "workspace", "limit_microdollars": 1, "workspace_id": foreign }),
            SCOPE_INVALID,
        ),
        (
            json!({ "scope": "workspace", "limit_microdollars": 1, "workspace_id": Uuid::new_v4() }),
            SCOPE_INVALID,
        ),
        (
            json!({ "scope": "user", "limit_microdollars": 1 }),
            SCOPE_INVALID,
        ),
        (
            json!({
                "scope": "user",
                "limit_microdollars": 1,
                "user_id": owner.id,
                "workspace_id": workspace,
            }),
            SCOPE_INVALID,
        ),
        (
            json!({ "scope": "user", "limit_microdollars": 1, "user_id": stranger.id }),
            SCOPE_INVALID,
        ),
    ] {
        let response = fixture.post(&path, &owner.token, &body).await;
        refused(&response, StatusCode::BAD_REQUEST, code);
    }
    refused(
        &fixture.post_raw(&path, &owner.token, "{\"scope\":").await,
        StatusCode::BAD_REQUEST,
        BODY_INVALID,
    );
    assert_eq!(listed(&fixture, &owner.token).await["budgets"], json!([]));
    assert!(
        fixture
            .policy
            .audits(actions::BUDGET_CREATED)
            .await
            .is_empty()
    );

    let budget = identifier(&created(&fixture, &owner.token, &organization_wide(LIMIT)).await);
    refused(
        &fixture
            .post(&path, &owner.token, &organization_wide(LIMIT * 2))
            .await,
        StatusCode::CONFLICT,
        BUDGET_EXISTS,
    );
    created(
        &fixture,
        &owner.token,
        &json!({ "scope": "user", "limit_microdollars": 1, "user_id": member.id }),
    )
    .await;

    let patched = budget_path(organization, budget);
    for (body, code) in [
        (json!({ "limit_microdollars": 0 }), LIMIT_INVALID),
        (json!({ "threshold_percent": 101 }), THRESHOLD_INVALID),
        (json!({ "scope": "user" }), BODY_INVALID),
        (json!({ "limit_microdollars": "ten" }), BODY_INVALID),
    ] {
        let response = fixture.patch(&patched, &owner.token, &body).await;
        refused(&response, StatusCode::BAD_REQUEST, code);
    }
    assert!(
        fixture
            .policy
            .audits(actions::BUDGET_UPDATED)
            .await
            .is_empty()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_budget_created_past_its_limit_is_exhausted_until_it_is_raised() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let owner = &fixture.policy.owner;
    fixture
        .record(Spend {
            microdollars: 5_000_000,
            ..fixture.spend()
        })
        .await;

    let exhausted = created(
        &fixture,
        &owner.token,
        &json!({
            "scope": "workspace",
            "workspace_id": fixture.policy.workspace,
            "limit_microdollars": 1_000_000,
            "hard": true,
        }),
    )
    .await;
    let id = identifier(&exhausted);
    assert_eq!(exhausted["state"], "exhausted");
    assert_eq!(exhausted["spent_microdollars"], 5_000_000);
    assert_eq!(
        exhausted_month(&fixture, id).await,
        Some(Month::current().date())
    );

    let raised = fixture
        .patch(
            &budget_path(fixture.policy.organization, id),
            &owner.token,
            &json!({ "limit_microdollars": 10_000_000 }),
        )
        .await;
    assert_eq!(raised.json_value()["state"], "under");
    assert_eq!(exhausted_month(&fixture, id).await, None);

    let warned = created(
        &fixture,
        &owner.token,
        &json!({ "scope": "user", "user_id": owner.id, "limit_microdollars": 6_000_000 }),
    )
    .await;
    assert_eq!(warned["state"], "warning");
    assert_eq!(warned["user_email"], email(&fixture, owner.id).await);
    assert_eq!(exhausted_month(&fixture, identifier(&warned)).await, None);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_report_totals_and_splits_the_organizations_spend_over_its_days() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let owner = &fixture.policy.owner;
    let first = fixture.policy.workspace;
    let second = fixture.policy.another_workspace().await;
    let member = fixture.policy.member(OrgRole::Member, None).await;
    let stranger = fixture.policy.stranger().await;
    let (foreign, foreign_workspace) = fixture.policy.another_organization(&stranger).await;
    let spend = fixture.spend();
    for recorded in [
        Spend {
            microdollars: 3_000_000,
            occurred_at: at("2026-10-01T00:00:00Z"),
            ..spend.clone()
        },
        Spend {
            user: None,
            automated: true,
            model: "claude-sonnet".to_string(),
            provider: "anthropic".to_string(),
            microdollars: 500_000,
            occurred_at: at("2026-10-01T12:00:00Z"),
            ..spend.clone()
        },
        Spend {
            workspace: Some(second),
            user: Some(member.id),
            occurred_at: at("2026-10-03T23:59:59.999Z"),
            ..spend.clone()
        },
        Spend {
            microdollars: 9_000_000,
            occurred_at: at("2026-09-30T23:59:59.999Z"),
            ..spend.clone()
        },
        Spend {
            microdollars: 9_000_000,
            occurred_at: at("2026-10-04T00:00:00Z"),
            ..spend.clone()
        },
        Spend {
            organization: foreign,
            workspace: Some(foreign_workspace),
            user: Some(stranger.id),
            microdollars: 7_000_000,
            occurred_at: at("2026-10-02T09:00:00Z"),
            ..spend.clone()
        },
    ] {
        fixture.record(recorded).await;
    }

    let response = reported(&fixture, OCTOBER).await;
    assert_eq!(response["entitled"], true);
    assert_eq!(response["enforced"], true);
    assert_eq!(response["editable"], true);
    assert_eq!(response["status"], "active");
    let report = &response["report"];
    assert_eq!(report["from"], "2026-10-01");
    assert_eq!(report["to"], "2026-10-03");
    assert_eq!(report["ingested_through"], Value::Null);
    assert_eq!(report["total"], usage(4_500_000, 3));
    assert_eq!(
        report["daily"],
        json!([
            with_usage(json!({ "date": "2026-10-01" }), 3_500_000, 2),
            with_usage(json!({ "date": "2026-10-02" }), 0, 0),
            with_usage(json!({ "date": "2026-10-03" }), 1_000_000, 1),
        ])
    );
    assert_eq!(
        report["workspaces"],
        json!([
            with_usage(json!({ "id": first, "name": "Policy" }), 3_500_000, 2),
            with_usage(json!({ "id": second, "name": "Policy" }), 1_000_000, 1),
        ])
    );
    let users = report["users"].as_array().unwrap();
    assert_eq!(users.len(), 3);
    assert_eq!(users[0]["id"], json!(owner.id));
    assert_eq!(users[0]["email"], email(&fixture, owner.id).await);
    assert_eq!(users[0]["automated"], false);
    assert_eq!(users[0]["spend_microdollars"], 3_000_000);
    assert_eq!(users[1]["id"], json!(member.id));
    assert_eq!(users[1]["spend_microdollars"], 1_000_000);
    assert_eq!(
        users[2],
        with_usage(
            json!({ "id": null, "name": null, "email": null, "automated": true }),
            500_000,
            1
        )
    );
    assert_eq!(
        report["models"],
        json!([
            with_usage(json!({ "name": "gpt-4o" }), 4_000_000, 2),
            with_usage(json!({ "name": "claude-sonnet" }), 500_000, 1),
        ])
    );
    assert_eq!(
        report["providers"],
        json!([
            with_usage(json!({ "name": "openai" }), 4_000_000, 2),
            with_usage(json!({ "name": "anthropic" }), 500_000, 1),
        ])
    );
    assert_eq!(report["uncounted"], json!([]));

    assert_eq!(
        total(&fixture, &format!("{OCTOBER}&workspace_id={second}")).await,
        usage(1_000_000, 1)
    );
    assert_eq!(
        total(&fixture, &format!("{OCTOBER}&user_id={}", owner.id)).await,
        usage(3_000_000, 1)
    );
    assert_eq!(
        total(
            &fixture,
            &format!("{OCTOBER}&workspace_id={first}&user_id={}", member.id)
        )
        .await,
        usage(0, 0)
    );

    let through = at("2026-10-03T10:04:00Z");
    cursor::advance(fixture.pool(), through).await.unwrap();
    let ingested = reported(&fixture, OCTOBER).await["report"]["ingested_through"].clone();
    assert_eq!(at(ingested.as_str().unwrap()), through);

    sqlx::query("DELETE FROM workspaces WHERE id = $1")
        .bind(second)
        .execute(fixture.pool())
        .await
        .unwrap();
    let report = reported(&fixture, OCTOBER).await["report"].clone();
    assert_eq!(report["total"], usage(4_500_000, 3));
    assert_eq!(
        report["workspaces"][1],
        with_usage(json!({ "id": null, "name": null }), 1_000_000, 1)
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_report_without_dates_covers_every_day_of_the_month_so_far() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let today = Utc::now().date_naive();

    let report = reported(&fixture, "").await["report"].clone();
    assert_eq!(report["from"], json!(Month::current().date()));
    assert_eq!(report["to"], json!(today));
    let daily = report["daily"].as_array().unwrap();
    assert_eq!(daily.len(), today.day() as usize);
    assert!(daily.iter().all(|day| day["requests"] == 0));
    assert_eq!(daily.last().unwrap()["date"], json!(today));
    fixture.cleanup().await;
}

#[tokio::test]
async fn filtering_on_another_organizations_workspace_reports_nothing() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let stranger = fixture.policy.stranger().await;
    let (foreign, foreign_workspace) = fixture.policy.another_organization(&stranger).await;
    fixture
        .record(Spend {
            occurred_at: at("2026-10-02T09:00:00Z"),
            ..fixture.spend()
        })
        .await;
    fixture
        .record(Spend {
            organization: foreign,
            workspace: Some(foreign_workspace),
            user: Some(stranger.id),
            occurred_at: at("2026-10-02T09:00:00Z"),
            ..fixture.spend()
        })
        .await;

    for filter in [
        format!("workspace_id={foreign_workspace}"),
        format!("user_id={}", stranger.id),
    ] {
        let report = reported(&fixture, &format!("{OCTOBER}&{filter}")).await["report"].clone();
        assert_eq!(report["total"], usage(0, 0), "{filter}");
        for breakdown in ["workspaces", "users", "models", "providers"] {
            assert_eq!(report[breakdown], json!([]), "{filter}: {breakdown}");
        }
        let daily = report["daily"].as_array().unwrap();
        assert_eq!(daily.len(), 3);
        assert!(daily.iter().all(|day| day["spend_microdollars"] == 0));
        let export = fixture
            .get(
                &export_of(&fixture, &format!("{OCTOBER}&{filter}")),
                &fixture.policy.owner.token,
            )
            .await;
        assert_eq!(export.text(), CSV_HEADER, "{filter}");
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_bad_range_is_refused_and_nothing_is_exported() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let token = &fixture.policy.owner.token;

    for query in [
        "from=2026-10-04&to=2026-10-03",
        "from=October",
        "workspace_id=not-a-workspace",
        "from=2026-10-01&from=2026-10-02",
        "from=2025-10-02&to=2026-10-03",
    ] {
        refused(
            &fixture.get(&report_of(&fixture, query), token).await,
            StatusCode::BAD_REQUEST,
            RANGE_INVALID,
        );
        refused(
            &fixture.get(&export_of(&fixture, query), token).await,
            StatusCode::BAD_REQUEST,
            RANGE_INVALID,
        );
    }
    assert!(
        fixture
            .policy
            .audits(actions::SPEND_EXPORT_DOWNLOADED)
            .await
            .is_empty()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_export_is_a_csv_attachment_with_neutralised_fields_and_is_audited() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let owner = &fixture.policy.owner;
    let organization = fixture.policy.organization;
    let workspace = fixture.policy.workspace;
    let spend = fixture.spend();
    fixture
        .record(Spend {
            model: "=HYPERLINK(\"http://example.com\")".to_string(),
            microdollars: 1_234_567,
            occurred_at: at("2026-10-01T08:00:00Z"),
            ..spend.clone()
        })
        .await;
    for occurred_at in ["2026-10-03T08:00:00Z", "2026-10-03T09:00:00Z"] {
        fixture
            .record(Spend {
                user: None,
                automated: true,
                microdollars: 500_000,
                occurred_at: at(occurred_at),
                ..spend.clone()
            })
            .await;
    }
    fixture
        .record(Spend {
            occurred_at: at("2026-10-04T00:00:00Z"),
            ..spend.clone()
        })
        .await;

    let response = fixture
        .get(&export_of(&fixture, OCTOBER), &owner.token)
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text());
    assert_eq!(response.header("content-type"), Some(CSV_TYPE));
    assert_eq!(
        response.header("content-disposition"),
        Some("attachment; filename=\"zone-spend-2026-10-01-2026-10-03.csv\"")
    );
    let owner_email = email(&fixture, owner.id).await;
    assert_eq!(
        response.text(),
        format!(
            "{CSV_HEADER}\
             2026-10-01,{workspace},Policy,{},{owner_email},false,\"'=HYPERLINK(\"\"http://example.com\"\")\",openai,1,100,20,1.234567\r\n\
             2026-10-03,{workspace},Policy,,,true,gpt-4o,openai,2,200,40,1.000000\r\n",
            owner.id
        )
    );
    let exports = fixture
        .policy
        .audits(actions::SPEND_EXPORT_DOWNLOADED)
        .await;
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].actor_id, Some(owner.id));
    assert_eq!(exports[0].organization_id, Some(organization));
    assert_eq!(exports[0].resource_type, resources::SPEND_EXPORT);
    assert_eq!(exports[0].resource_id, Some(organization));
    assert_eq!(
        exports[0].new_values,
        Some(json!({
            "from": "2026-10-01",
            "to": "2026-10-03",
            "workspace_id": null,
            "user_id": null,
        }))
    );

    let empty = fixture
        .get(
            &export_of(&fixture, &format!("{OCTOBER}&user_id={}", Uuid::new_v4())),
            &owner.token,
        )
        .await;
    assert_eq!(empty.text(), CSV_HEADER);
    assert_eq!(
        fixture
            .policy
            .audits(actions::SPEND_EXPORT_DOWNLOADED)
            .await
            .len(),
        2
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_report_names_a_workspace_on_a_coding_agent_as_uncounted() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    assert_eq!(
        reported(&fixture, "").await["report"]["uncounted"],
        json!([])
    );

    sqlx::query(
        "INSERT INTO workspace_ai_settings (workspace_id, provider) VALUES ($1, 'claude_code')",
    )
    .bind(fixture.policy.workspace)
    .execute(fixture.pool())
    .await
    .unwrap();

    assert_eq!(
        reported(&fixture, "").await["report"]["uncounted"],
        json!(["coding_agent"])
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn spend_recorded_before_the_month_counts_toward_no_budget() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    fixture
        .record(Spend {
            occurred_at: Month::current().start() - TimeDelta::seconds(1),
            ..fixture.spend()
        })
        .await;

    let budget = created(
        &fixture,
        &fixture.policy.owner.token,
        &organization_wide(LIMIT),
    )
    .await;
    assert_eq!(budget["spent_microdollars"], 0);
    assert_eq!(budget["state"], "under");
    fixture.cleanup().await;
}

async fn name_workspace(fixture: &Fixture, workspace: Uuid, name: &str) {
    sqlx::query("UPDATE workspaces SET name = $2 WHERE id = $1")
        .bind(workspace)
        .bind(name)
        .execute(fixture.pool())
        .await
        .unwrap();
}

async fn name_user(fixture: &Fixture, user: Uuid, name: &str) {
    sqlx::query("UPDATE users SET display_name = $2 WHERE id = $1")
        .bind(user)
        .bind(name)
        .execute(fixture.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn a_former_members_spend_is_reported_and_exported_apart_from_automation() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let workspace = fixture.policy.workspace;
    let departed = fixture.policy.member(OrgRole::Member, None).await;
    let spend = Spend {
        occurred_at: at("2026-10-02T09:00:00Z"),
        ..fixture.spend()
    };
    fixture
        .record(Spend {
            user: None,
            automated: true,
            microdollars: 2_000_000,
            ..spend.clone()
        })
        .await;
    fixture
        .record(Spend {
            user: Some(departed.id),
            microdollars: 500_000,
            ..spend.clone()
        })
        .await;
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(departed.id)
        .execute(fixture.pool())
        .await
        .unwrap();

    let report = reported(&fixture, OCTOBER).await["report"].clone();
    assert_eq!(report["total"], usage(2_500_000, 2));
    assert_eq!(
        report["users"],
        json!([
            with_usage(
                json!({ "id": null, "name": null, "email": null, "automated": true }),
                2_000_000,
                1
            ),
            with_usage(
                json!({ "id": null, "name": null, "email": null, "automated": false }),
                500_000,
                1
            ),
        ])
    );
    let export = fixture
        .get(&export_of(&fixture, OCTOBER), &fixture.policy.owner.token)
        .await;
    assert_eq!(
        export.text(),
        format!(
            "{CSV_HEADER}\
             2026-10-02,{workspace},Policy,,,true,gpt-4o,openai,1,100,20,2.000000\r\n\
             2026-10-02,{workspace},Policy,,,false,gpt-4o,openai,1,100,20,0.500000\r\n"
        )
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn breakdowns_with_the_same_spend_are_sorted_by_name() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let owner = &fixture.policy.owner;
    let first = fixture.policy.workspace;
    let second = fixture.policy.another_workspace().await;
    let member = fixture.policy.member(OrgRole::Member, None).await;
    name_workspace(&fixture, second, "Alpha").await;
    name_user(&fixture, owner.id, "Zed").await;
    name_user(&fixture, member.id, "Ada").await;
    let spend = Spend {
        occurred_at: at("2026-10-02T09:00:00Z"),
        ..fixture.spend()
    };
    fixture
        .record(Spend {
            workspace: Some(second),
            user: Some(member.id),
            model: "a-model".to_string(),
            provider: "anthropic".to_string(),
            ..spend.clone()
        })
        .await;
    fixture.record(spend).await;

    let report = reported(&fixture, OCTOBER).await["report"].clone();
    let names = |breakdown: &str| -> Vec<Value> {
        report[breakdown]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["name"].clone())
            .collect()
    };
    assert_eq!(names("workspaces"), [json!("Alpha"), json!("Policy")]);
    assert_eq!(names("users"), [json!("Ada"), json!("Zed")]);
    assert_eq!(names("models"), [json!("a-model"), json!("gpt-4o")]);
    assert_eq!(names("providers"), [json!("anthropic"), json!("openai")]);
    assert_eq!(report["workspaces"][0]["id"], json!(second));
    assert_eq!(report["workspaces"][1]["id"], json!(first));
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_admin_who_is_not_the_owner_creates_changes_and_deletes_budgets() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let admin = fixture.policy.member(OrgRole::Admin, None).await;
    let organization = fixture.policy.organization;

    let budget = identifier(&created(&fixture, &admin.token, &organization_wide(LIMIT)).await);
    let path = budget_path(organization, budget);
    let patched = fixture
        .patch(&path, &admin.token, &json!({ "hard": true }))
        .await;
    assert_eq!(patched.status, StatusCode::OK, "{}", patched.text());
    assert_eq!(patched.json_value()["hard"], true);
    let deleted = fixture.delete(&path, &admin.token).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);

    for action in [
        actions::BUDGET_CREATED,
        actions::BUDGET_UPDATED,
        actions::BUDGET_DELETED,
    ] {
        let entries = fixture.policy.audits(action).await;
        assert_eq!(entries.len(), 1, "{action}");
        assert_eq!(entries[0].actor_id, Some(admin.id), "{action}");
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_budget_of_another_organization_is_not_found_through_this_ones_path() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let owner = &fixture.policy.owner;
    let stranger = fixture.policy.stranger().await;
    let (foreign, _) = fixture.policy.another_organization(&stranger).await;
    let response = fixture
        .post(
            &budgets_path(foreign),
            &stranger.token,
            &organization_wide(LIMIT),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text());
    let budget = identifier(&response.json_value());
    let path = budget_path(fixture.policy.organization, budget);

    for response in [
        fixture
            .patch(&path, &owner.token, &json!({ "limit_microdollars": 1 }))
            .await,
        fixture.delete(&path, &owner.token).await,
    ] {
        assert_eq!(
            response.status,
            StatusCode::NOT_FOUND,
            "{}",
            response.text()
        );
        assert_eq!(response.json_value(), json!({ "error": BUDGET_MISSING }));
    }
    let kept = fixture.get(&budgets_path(foreign), &stranger.token).await;
    assert_eq!(kept.json_value()["budgets"][0]["limit_microdollars"], LIMIT);
    assert!(
        fixture
            .policy
            .audits(actions::BUDGET_UPDATED)
            .await
            .is_empty()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn reading_spend_waits_on_no_lock_held_on_the_membership() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.licensed().await;
    let owner = &fixture.policy.owner;
    let organization = fixture.policy.organization;
    let mut holder = fixture.pool().begin().await.unwrap();
    sqlx::query(
        "SELECT 1 FROM organization_members WHERE organization_id = $1 AND user_id = $2 \
         FOR UPDATE",
    )
    .bind(organization)
    .bind(owner.id)
    .execute(&mut *holder)
    .await
    .unwrap();

    for path in [
        reports_path(organization),
        exports_path(organization),
        budgets_path(organization),
    ] {
        let response =
            tokio::time::timeout(Duration::from_secs(5), fixture.get(&path, &owner.token))
                .await
                .unwrap_or_else(|_| panic!("{path} waited on the membership lock"));
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{path}: {}",
            response.text()
        );
    }
    holder.rollback().await.unwrap();
    fixture.cleanup().await;
}
