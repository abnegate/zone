//! The audit retention routes and sweep: who may read and change the period, how the license
//! gates a change but not the sweep, and that a row is deleted only once it is past the period
//! and every enabled destination of its scope has delivered it.

use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::audit::{RETENTION_INVALID, Wait};
use zone_enterprise::{LICENSE_REQUIRED, Tier};
use zone_server::db::audit::position::Position;
use zone_server::db::audit::retentions;
use zone_server::db::audit::scope::Scope;
use zone_server::db::audit::{actions, resources};
use zone_server::db::organization_members::OrgRole;
use zone_server::routes::audit::access::FORBIDDEN;
use zone_server::routes::audit::refusal::BODY_INVALID;
use zone_server::services::audit::retention;

use crate::common::TestResponse;
use crate::common::audit::{Destination, UNVERIFIABLE, position, refused, settle};
use crate::common::policy::Fixture;
use crate::common::sso::{active, lapsed};

const PREFIX: &str = "audit_retention";
const SERVER: &str = "/api/audit-retention";
const EXPIRED_DAYS: i32 = 60;
const PERIOD: i32 = 30;
const UNDELIVERED: Position = Position {
    transaction_id: 0,
    sequence: 0,
};

fn path(organization: Uuid) -> String {
    format!("/api/organizations/{organization}/audit-retention")
}

async fn get(fixture: &Fixture, path: &str, token: &str) -> TestResponse {
    fixture.sso.client.get_auth(path, token).await
}

async fn put(fixture: &Fixture, path: &str, token: &str, body: Value) -> TestResponse {
    fixture.sso.client.put_json_auth(path, &body, token).await
}

fn ok(response: TestResponse) -> Value {
    response.assert_status(StatusCode::OK);
    response.json_value()
}

async fn row(pool: &PgPool, organization: Option<Uuid>, age_days: i32) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO audit_logs (organization_id, action, resource_type, created_at) \
         VALUES ($1, $2, $3, now() - make_interval(days => $4)) RETURNING id",
    )
    .bind(organization)
    .bind(actions::SETTINGS_UPDATED)
    .bind(resources::ORGANIZATION)
    .bind(age_days)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn exists(pool: &PgPool, id: Uuid) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM audit_logs WHERE id = $1)")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn destination(
    pool: &PgPool,
    organization: Option<Uuid>,
    name: &str,
    enabled: bool,
    delivered: Position,
) -> Uuid {
    Destination {
        name: name.to_owned(),
        enabled,
        ..Destination::new(organization)
    }
    .create(pool, delivered)
    .await
}

async fn deliver(pool: &PgPool, destination: Uuid, delivered: Position) {
    sqlx::query(
        "UPDATE audit_deliveries SET transaction_id = $2, sequence = $3, attempts = 0 \
         WHERE destination_id = $1",
    )
    .bind(destination)
    .bind(delivered.transaction_id)
    .bind(delivered.sequence)
    .execute(pool)
    .await
    .unwrap();
}

async fn fail(pool: &PgPool, destination: Uuid, attempts: i32) {
    sqlx::query("UPDATE audit_deliveries SET attempts = $2 WHERE destination_id = $1")
        .bind(destination)
        .bind(attempts)
        .execute(pool)
        .await
        .unwrap();
}

fn holders(body: &Value) -> Vec<(String, String)> {
    body["holders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|holder| {
            (
                holder["name"].as_str().unwrap().to_owned(),
                holder["wait"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn waiting(name: &str, wait: Wait) -> Vec<(String, String)> {
    vec![(name.to_owned(), wait.to_string())]
}

#[tokio::test]
async fn admins_read_and_change_the_period_and_others_are_refused() {
    let fixture = Fixture::new(PREFIX).await;
    let owner = &fixture.owner;
    let organization = path(fixture.organization);

    let unset = ok(get(&fixture, &organization, &owner.token).await);
    assert_eq!(unset["days"], Value::Null);
    assert_eq!(unset["cutoff"], Value::Null);
    assert_eq!(unset["expired"], 0);
    assert_eq!(unset["held"], 0);
    assert_eq!(unset["holders"], json!([]));
    assert_eq!(unset["swept_at"], Value::Null);
    assert_eq!(unset["deleted_count"], 0);
    assert_eq!(unset["updated_at"], Value::Null);
    assert_eq!(unset["standing"]["applies"], false);
    assert_eq!(unset["standing"]["pause"], "unlicensed");

    refused(
        &put(
            &fixture,
            &organization,
            &owner.token,
            json!({ "days": PERIOD }),
        )
        .await,
        StatusCode::FORBIDDEN,
        LICENSE_REQUIRED,
    );
    refused(
        &put(&fixture, SERVER, &owner.token, json!({ "days": PERIOD })).await,
        StatusCode::FORBIDDEN,
        LICENSE_REQUIRED,
    );

    fixture.license(Tier::Enterprise).await;
    let member = fixture.member(OrgRole::Member, None).await;
    let stranger = fixture.stranger().await;

    let forbidden = get(&fixture, &organization, &member.token).await;
    forbidden.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(forbidden.json_value()["error"], FORBIDDEN);
    put(
        &fixture,
        &organization,
        &member.token,
        json!({ "days": PERIOD }),
    )
    .await
    .assert_status(StatusCode::FORBIDDEN);
    get(&fixture, &organization, &stranger.token)
        .await
        .assert_status(StatusCode::NOT_FOUND);
    put(
        &fixture,
        &organization,
        &stranger.token,
        json!({ "days": PERIOD }),
    )
    .await
    .assert_status(StatusCode::NOT_FOUND);
    get(&fixture, SERVER, &member.token)
        .await
        .assert_status(StatusCode::FORBIDDEN);
    put(&fixture, SERVER, &member.token, json!({ "days": PERIOD }))
        .await
        .assert_status(StatusCode::FORBIDDEN);

    for days in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!(i64::from(i32::MAX) + 1),
    ] {
        refused(
            &put(
                &fixture,
                &organization,
                &owner.token,
                json!({ "days": days }),
            )
            .await,
            StatusCode::BAD_REQUEST,
            RETENTION_INVALID,
        );
    }
    refused(
        &put(
            &fixture,
            &organization,
            &owner.token,
            json!({ "days": "thirty" }),
        )
        .await,
        StatusCode::BAD_REQUEST,
        BODY_INVALID,
    );
    refused(
        &put(
            &fixture,
            &organization,
            &owner.token,
            json!({ "days": PERIOD, "forever": false }),
        )
        .await,
        StatusCode::BAD_REQUEST,
        BODY_INVALID,
    );
    assert!(
        fixture
            .audits(actions::AUDIT_RETENTION_UPDATED)
            .await
            .is_empty(),
        "a refused change audits nothing"
    );

    let set = ok(put(
        &fixture,
        &organization,
        &owner.token,
        json!({ "days": PERIOD }),
    )
    .await);
    assert_eq!(set["days"], PERIOD);
    assert!(set["cutoff"].is_string());
    assert!(set["updated_at"].is_string());
    assert_eq!(set["standing"]["retention"], true);
    assert_eq!(
        ok(get(&fixture, &organization, &owner.token).await)["days"],
        PERIOD
    );

    ok(put(
        &fixture,
        &organization,
        &owner.token,
        json!({ "days": PERIOD }),
    )
    .await);
    let cleared = ok(put(&fixture, &organization, &owner.token, json!({})).await);
    assert_eq!(cleared["days"], Value::Null);
    assert_eq!(cleared["updated_at"], Value::Null);

    let audits = fixture.audits(actions::AUDIT_RETENTION_UPDATED).await;
    assert_eq!(audits.len(), 2, "an unchanged period audits nothing");
    for audit in &audits {
        assert_eq!(audit.actor_id, Some(owner.id));
        assert_eq!(audit.organization_id, Some(fixture.organization));
        assert_eq!(audit.resource_type, resources::AUDIT_RETENTION);
        assert_eq!(audit.resource_id, Some(fixture.organization));
    }
    assert_eq!(audits[0].old_values, Some(json!({ "days": null })));
    assert_eq!(audits[0].new_values, Some(json!({ "days": PERIOD })));
    assert_eq!(audits[1].old_values, Some(json!({ "days": PERIOD })));
    assert_eq!(audits[1].new_values, Some(json!({ "days": null })));
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_sweep_deletes_only_expired_rows_every_enabled_destination_delivered() {
    let fixture = Fixture::new(PREFIX).await;
    let pool = fixture.pool();
    let owner = &fixture.owner;
    let organization = path(fixture.organization);
    fixture.license(Tier::Enterprise).await;
    let (other, _) = fixture.another_organization(owner).await;

    let delivered = row(pool, Some(fixture.organization), EXPIRED_DAYS).await;
    let undelivered = row(pool, Some(fixture.organization), EXPIRED_DAYS).await;
    let fresh = row(pool, Some(fixture.organization), 0).await;
    let elsewhere = row(pool, Some(other), EXPIRED_DAYS).await;
    let server = row(pool, None, EXPIRED_DAYS).await;
    let siem = destination(
        pool,
        Some(fixture.organization),
        "SIEM",
        true,
        position(pool, delivered).await,
    )
    .await;
    destination(
        pool,
        Some(fixture.organization),
        "Archive",
        false,
        UNDELIVERED,
    )
    .await;
    retentions::set(pool, Scope::Organization(other), PERIOD, owner.id)
        .await
        .unwrap();
    destination(pool, Some(other), "Other", true, UNDELIVERED).await;
    settle(pool).await;

    let set = ok(put(
        &fixture,
        &organization,
        &owner.token,
        json!({ "days": PERIOD }),
    )
    .await);
    assert_eq!(set["expired"], 2);
    assert_eq!(set["held"], 1);
    assert_eq!(holders(&set), waiting("SIEM", Wait::Backlog));
    assert_eq!(set["holders"][0]["destination_id"], siem.to_string());

    assert_eq!(retention::sweep(fixture.state()).await.unwrap(), 1);
    assert!(!exists(pool, delivered).await);
    assert!(
        exists(pool, undelivered).await,
        "the SIEM has not delivered it"
    );
    assert!(exists(pool, fresh).await, "it is inside the period");
    assert!(
        exists(pool, elsewhere).await,
        "its organization's destination holds it"
    );
    assert!(exists(pool, server).await, "the server scope has no period");

    let swept = ok(get(&fixture, &organization, &owner.token).await);
    assert_eq!(swept["expired"], 1);
    assert_eq!(swept["held"], 1);
    assert_eq!(swept["deleted_count"], 1);
    assert!(swept["swept_at"].is_string());

    let applied = fixture.audits(actions::AUDIT_RETENTION_APPLIED).await;
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].actor_id, None);
    assert_eq!(applied[0].organization_id, Some(fixture.organization));
    assert_eq!(applied[0].resource_type, resources::AUDIT_RETENTION);
    let values = applied[0].new_values.as_ref().unwrap();
    assert_eq!(values["days"], PERIOD);
    assert_eq!(values["deleted_count"], 1);

    fail(pool, siem, 3).await;
    let failing = ok(get(&fixture, &organization, &owner.token).await);
    assert_eq!(holders(&failing), waiting("SIEM", Wait::Failing));

    deliver(pool, siem, position(pool, fresh).await).await;
    settle(pool).await;
    assert_eq!(retention::sweep(fixture.state()).await.unwrap(), 1);
    assert!(!exists(pool, undelivered).await);
    assert!(exists(pool, fresh).await);
    let caught_up = ok(get(&fixture, &organization, &owner.token).await);
    assert_eq!(caught_up["expired"], 0);
    assert_eq!(caught_up["held"], 0);
    assert_eq!(caught_up["holders"], json!([]));
    assert_eq!(caught_up["deleted_count"], 1);

    assert_eq!(retention::sweep(fixture.state()).await.unwrap(), 0);
    let idle = ok(get(&fixture, &organization, &owner.token).await);
    assert_eq!(idle["deleted_count"], 0, "the count is the latest sweep's");
    assert_eq!(
        fixture.audits(actions::AUDIT_RETENTION_APPLIED).await.len(),
        2,
        "a sweep that deleted nothing audits nothing"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn community_applies_no_period() {
    let fixture = Fixture::new(PREFIX).await;
    let pool = fixture.pool();
    let scope = Scope::Organization(fixture.organization);
    let expired = row(pool, Some(fixture.organization), EXPIRED_DAYS).await;
    retentions::set(pool, scope, PERIOD, fixture.owner.id)
        .await
        .unwrap();

    assert_eq!(retention::sweep(fixture.state()).await.unwrap(), 0);
    assert!(exists(pool, expired).await);
    let record = retentions::get(pool, scope).await.unwrap().unwrap();
    assert_eq!(record.swept_at, None);

    let status = ok(get(&fixture, &path(fixture.organization), &fixture.owner.token).await);
    assert_eq!(status["days"], PERIOD);
    assert_eq!(status["standing"]["applies"], false);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_downgraded_or_unverified_license_keeps_applying_but_cannot_change_the_period() {
    for key in [
        lapsed(Tier::Enterprise),
        active(Tier::Team),
        UNVERIFIABLE.to_owned(),
    ] {
        let fixture = Fixture::new(PREFIX).await;
        let pool = fixture.pool();
        let owner = &fixture.owner;
        let organization = path(fixture.organization);
        let scope = Scope::Organization(fixture.organization);
        retentions::set(pool, scope, PERIOD, owner.id)
            .await
            .unwrap();
        fixture.store_license(&key).await;
        let expired = row(pool, Some(fixture.organization), EXPIRED_DAYS).await;
        settle(pool).await;

        assert_eq!(retention::sweep(fixture.state()).await.unwrap(), 1);
        assert!(!exists(pool, expired).await);

        let held = row(pool, Some(fixture.organization), EXPIRED_DAYS).await;
        destination(pool, Some(fixture.organization), "SIEM", true, UNDELIVERED).await;
        settle(pool).await;
        assert_eq!(retention::sweep(fixture.state()).await.unwrap(), 0);
        assert!(exists(pool, held).await);

        let status = ok(get(&fixture, &organization, &owner.token).await);
        assert_eq!(status["standing"]["applies"], true);
        assert_eq!(status["standing"]["retention"], false);
        assert_eq!(status["held"], 1);
        assert_eq!(holders(&status), waiting("SIEM", Wait::Paused));

        for body in [json!({ "days": null }), json!({ "days": 7 }), json!({})] {
            refused(
                &put(&fixture, &organization, &owner.token, body).await,
                StatusCode::FORBIDDEN,
                LICENSE_REQUIRED,
            );
        }
        assert_eq!(
            retentions::get(pool, scope).await.unwrap().unwrap().days,
            PERIOD
        );
        fixture.cleanup().await;
    }
}

#[tokio::test]
async fn the_server_period_applies_only_to_rows_without_an_organization() {
    let fixture = Fixture::new(PREFIX).await;
    let pool = fixture.pool();
    let owner = &fixture.owner;
    fixture.license(Tier::Enterprise).await;
    let server = row(pool, None, EXPIRED_DAYS).await;
    let organization = row(pool, Some(fixture.organization), EXPIRED_DAYS).await;
    settle(pool).await;

    let set = ok(put(&fixture, SERVER, &owner.token, json!({ "days": PERIOD })).await);
    assert_eq!(set["days"], PERIOD);
    assert_eq!(set["expired"], 1);
    assert_eq!(set["held"], 0);
    assert_eq!(
        ok(get(&fixture, &path(fixture.organization), &owner.token).await)["days"],
        Value::Null
    );

    assert_eq!(retention::sweep(fixture.state()).await.unwrap(), 1);
    assert!(!exists(pool, server).await);
    assert!(exists(pool, organization).await);

    let swept = ok(get(&fixture, SERVER, &owner.token).await);
    assert_eq!(swept["deleted_count"], 1);
    let updated = fixture.audits(actions::AUDIT_RETENTION_UPDATED).await;
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].organization_id, None);
    assert_eq!(updated[0].resource_id, None);
    let applied = fixture.audits(actions::AUDIT_RETENTION_APPLIED).await;
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].organization_id, None);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_failed_rebase_aborts_the_sweep_before_anything_is_deleted() {
    let fixture = Fixture::new(PREFIX).await;
    let pool = fixture.pool();
    let scope = Scope::Organization(fixture.organization);
    fixture.license(Tier::Enterprise).await;
    let expired = row(pool, Some(fixture.organization), EXPIRED_DAYS).await;
    retentions::set(pool, scope, PERIOD, fixture.owner.id)
        .await
        .unwrap();
    destination(
        pool,
        Some(fixture.organization),
        "Restored",
        true,
        Position {
            transaction_id: i64::MAX - 1,
            sequence: i64::MAX,
        },
    )
    .await;
    sqlx::query(
        "CREATE FUNCTION refuse_rebase() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN RAISE EXCEPTION 'refused'; END $$",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER refuse_rebase BEFORE UPDATE ON audit_deliveries \
         FOR EACH ROW EXECUTE FUNCTION refuse_rebase()",
    )
    .execute(pool)
    .await
    .unwrap();
    settle(pool).await;

    retention::sweep(fixture.state())
        .await
        .expect_err("the rebase fails");
    assert!(
        exists(pool, expired).await,
        "a cursor from before a restore must not let the sweep delete"
    );
    assert_eq!(
        retentions::get(pool, scope)
            .await
            .unwrap()
            .unwrap()
            .swept_at,
        None
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_sweep_while_another_instance_sweeps_deletes_and_records_nothing() {
    let fixture = Fixture::new(PREFIX).await;
    let pool = fixture.pool();
    let scope = Scope::Organization(fixture.organization);
    fixture.license(Tier::Enterprise).await;
    let expired = row(pool, Some(fixture.organization), EXPIRED_DAYS).await;
    retentions::set(pool, scope, PERIOD, fixture.owner.id)
        .await
        .unwrap();
    settle(pool).await;
    let mut other = pool.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(retention::SWEEP_LOCK)
        .execute(&mut *other)
        .await
        .unwrap();

    assert_eq!(retention::sweep(fixture.state()).await.unwrap(), 0);
    assert!(
        exists(pool, expired).await,
        "the other instance owns the sweep"
    );
    assert_eq!(
        retentions::get(pool, scope)
            .await
            .unwrap()
            .unwrap()
            .swept_at,
        None,
        "only the sweeping instance records a sweep"
    );

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(retention::SWEEP_LOCK)
        .execute(&mut *other)
        .await
        .unwrap();
    assert_eq!(retention::sweep(fixture.state()).await.unwrap(), 1);
    assert!(!exists(pool, expired).await);
    let free: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_lock($1) AND pg_advisory_unlock($1)")
            .bind(retention::SWEEP_LOCK)
            .fetch_one(&mut *other)
            .await
            .unwrap();
    assert!(free, "a finished sweep releases the lock");
    drop(other);
    fixture.cleanup().await;
}
