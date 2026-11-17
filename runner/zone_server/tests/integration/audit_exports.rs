use axum::http::StatusCode;
use bytes::Bytes;
use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use futures_util::TryStreamExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::audit::{Event, Format, csv};
use zone_enterprise::{LICENSE_REQUIRED, Tier};
use zone_server::db::audit::events::Range;
use zone_server::db::audit::scope::Scope;
use zone_server::db::audit::{actions, resources};
use zone_server::db::organization_members::OrgRole;
use zone_server::services::audit::export;

use crate::common::policy::Fixture;
use crate::common::sso::{self, Account};

const PREFIX: &str = "audit_exports";
const SERVER_PATH: &str = "/api/audit-exports";
const WHOLE_YEAR: &str = "start_date=2020-01-01&end_date=2020-12-31";
const MANY: i64 = 2_500;
const RANGE_INVALID: &str = "audit_export_range_invalid";
const FORMAT_INVALID: &str = "audit_export_format_invalid";
const NDJSON: &str = "application/x-ndjson";
const CSV: &str = "text/csv; charset=utf-8";
const REFUSE_EXPORT_AUDIT: [&str; 2] = [
    "CREATE FUNCTION refuse_export_audit() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.action = 'audit_export.downloaded' THEN
        RAISE EXCEPTION 'refused';
    END IF;
    RETURN NEW;
END $$",
    "CREATE TRIGGER refuse_export_audit BEFORE INSERT ON audit_logs
    FOR EACH ROW EXECUTE FUNCTION refuse_export_audit()",
];

fn organization_path(organization: Uuid) -> String {
    format!("/api/organizations/{organization}/audit-exports")
}

fn year_start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap()
}

fn year() -> Range {
    Range {
        start: year_start(),
        end: Utc.with_ymd_and_hms(2020, 12, 31, 23, 59, 59).unwrap()
            + TimeDelta::microseconds(999_999),
    }
}

/// Writes `count` rows into `organization`, two to each second from `from`, so ties on
/// `created_at` are broken by id.
async fn seed(pool: &PgPool, organization: Option<Uuid>, from: DateTime<Utc>, count: i64) {
    sqlx::query(
        "INSERT INTO audit_logs (organization_id, actor_email, action, resource_type, ip_address, created_at)
         SELECT $1, 'actor' || n || '@example.com', 'member.added', 'member', '10.0.0.5'::inet,
                $2 + make_interval(secs => n / 2)
         FROM generate_series(0, $3 - 1) AS n",
    )
    .bind(organization)
    .bind(from)
    .bind(count)
    .execute(pool)
    .await
    .unwrap();
}

async fn write(pool: &PgPool, organization: Option<Uuid>, action: &str, at: DateTime<Utc>) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO audit_logs (organization_id, action, resource_type, created_at)
         VALUES ($1, $2, 'member', $3) RETURNING id",
    )
    .bind(organization)
    .bind(action)
    .bind(at)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn ordered(pool: &PgPool, organization: Option<Uuid>, range: Range) -> Vec<Uuid> {
    sqlx::query_scalar(
        "SELECT id FROM audit_logs
         WHERE organization_id IS NOT DISTINCT FROM $1 AND created_at BETWEEN $2 AND $3
         ORDER BY created_at, id",
    )
    .bind(organization)
    .bind(range.start)
    .bind(range.end)
    .fetch_all(pool)
    .await
    .unwrap()
}

fn events(body: &[u8]) -> Vec<Event> {
    serde_json::Deserializer::from_slice(body)
        .into_iter()
        .collect::<Result<_, _>>()
        .expect("every line is an event")
}

fn csv_ids(body: &str) -> Vec<Uuid> {
    let rows = body
        .strip_prefix(csv::HEADER)
        .expect("the header comes first");
    rows.split_terminator("\r\n")
        .map(|row| row.split(',').next().unwrap().parse().unwrap())
        .collect()
}

async fn exports(fixture: &Fixture) -> Vec<sso::Audit> {
    fixture.audits(actions::AUDIT_EXPORT_DOWNLOADED).await
}

async fn get(fixture: &Fixture, path: &str, account: &Account) -> crate::common::TestResponse {
    fixture.sso.client.get_auth(path, &account.token).await
}

#[tokio::test]
async fn thousands_of_events_stream_in_order_as_ndjson_and_csv() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license(Tier::Team).await;
    seed(
        fixture.pool(),
        Some(fixture.organization),
        year_start(),
        MANY,
    )
    .await;
    let expected = ordered(fixture.pool(), Some(fixture.organization), year()).await;
    assert_eq!(expected.len(), MANY as usize);
    let path = organization_path(fixture.organization);

    let response = get(&fixture, &format!("{path}?{WHOLE_YEAR}"), &fixture.owner).await;
    response.assert_status(StatusCode::OK);
    assert_eq!(response.header("content-type"), Some(NDJSON));
    assert_eq!(
        response.header("content-disposition"),
        Some(
            format!(
                "attachment; filename=\"audit-{}-2020-01-01-2020-12-31.ndjson\"",
                fixture.organization
            )
            .as_str()
        )
    );
    let parsed = events(response.bytes());
    assert_eq!(
        parsed.iter().map(|event| event.id).collect::<Vec<_>>(),
        expected
    );
    assert!(parsed.iter().all(|event| event.version == 1
        && event.organization_id == Some(fixture.organization)
        && event.ip_address.as_deref() == Some("10.0.0.5")));

    let response = get(
        &fixture,
        &format!("{path}?format=csv&{WHOLE_YEAR}"),
        &fixture.owner,
    )
    .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(response.header("content-type"), Some(CSV));
    assert!(
        response
            .header("content-disposition")
            .is_some_and(|value| value.ends_with("-2020-01-01-2020-12-31.csv\""))
    );
    assert_eq!(csv_ids(&response.text()), expected);
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_export_is_read_one_page_at_a_time() {
    let fixture = Fixture::new(PREFIX).await;
    seed(
        fixture.pool(),
        Some(fixture.organization),
        year_start(),
        MANY,
    )
    .await;
    let scope = Scope::Organization(fixture.organization);

    let chunks: Vec<Bytes> = export::stream(fixture.pool().clone(), scope, Format::Ndjson, year())
        .try_collect()
        .await
        .unwrap();
    let pages = (MANY as usize).div_ceil(export::PAGE);
    assert_eq!(chunks.len(), pages);
    assert_eq!(
        chunks
            .iter()
            .map(|chunk| events(chunk).len())
            .collect::<Vec<_>>(),
        [export::PAGE, export::PAGE, MANY as usize - 2 * export::PAGE]
    );

    let chunks: Vec<Bytes> = export::stream(fixture.pool().clone(), scope, Format::Csv, year())
        .try_collect()
        .await
        .unwrap();
    assert_eq!(chunks.len(), pages + 1);
    assert_eq!(chunks[0], csv::HEADER.as_bytes());
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_empty_range_is_a_header_or_nothing() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license(Tier::Team).await;
    let path = organization_path(fixture.organization);

    let response = get(&fixture, &format!("{path}?{WHOLE_YEAR}"), &fixture.owner).await;
    response.assert_status(StatusCode::OK);
    assert!(response.bytes().is_empty());

    let response = get(
        &fixture,
        &format!("{path}?format=csv&{WHOLE_YEAR}"),
        &fixture.owner,
    )
    .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(response.text(), csv::HEADER);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_date_range_covers_whole_days_and_an_instant_range_is_inclusive() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license(Tier::Team).await;
    let organization = Some(fixture.organization);
    let pool = fixture.pool();
    let start = Utc.with_ymd_and_hms(2020, 3, 1, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2020, 3, 2, 0, 0, 0).unwrap();
    let microsecond = TimeDelta::microseconds(1);
    write(pool, organization, "before", start - microsecond).await;
    let first = write(pool, organization, "first", start).await;
    let noon = write(pool, organization, "noon", start + TimeDelta::hours(12)).await;
    let last = write(pool, organization, "last", end - microsecond).await;
    let midnight = write(pool, organization, "midnight", end).await;
    let path = organization_path(fixture.organization);

    let response = get(
        &fixture,
        &format!("{path}?start_date=2020-03-01&end_date=2020-03-01"),
        &fixture.owner,
    )
    .await;
    response.assert_status(StatusCode::OK);
    let ids: Vec<Uuid> = events(response.bytes())
        .iter()
        .map(|event| event.id)
        .collect();
    assert_eq!(ids, [first, noon, last]);

    let response = get(
        &fixture,
        &format!("{path}?start_date=2020-03-01T12:00:00Z&end_date=2020-03-02T00:00:00Z"),
        &fixture.owner,
    )
    .await;
    response.assert_status(StatusCode::OK);
    let ids: Vec<Uuid> = events(response.bytes())
        .iter()
        .map(|event| event.id)
        .collect();
    assert_eq!(ids, [noon, last, midnight]);
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_organization_export_holds_only_its_own_rows_and_the_server_export_only_server_rows() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license(Tier::Enterprise).await;
    let stranger = fixture.stranger().await;
    let (other, _) = fixture.another_organization(&stranger).await;
    let at = year_start() + TimeDelta::days(10);
    let own = write(fixture.pool(), Some(fixture.organization), "own", at).await;
    let foreign = write(fixture.pool(), Some(other), "foreign", at).await;
    let server = write(fixture.pool(), None, "server", at).await;

    let response = get(
        &fixture,
        &format!("{}?{WHOLE_YEAR}", organization_path(fixture.organization)),
        &fixture.owner,
    )
    .await;
    response.assert_status(StatusCode::OK);
    let ids: Vec<Uuid> = events(response.bytes())
        .iter()
        .map(|event| event.id)
        .collect();
    assert_eq!(ids, [own]);

    let response = get(
        &fixture,
        &format!("{SERVER_PATH}?{WHOLE_YEAR}"),
        &fixture.owner,
    )
    .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.header("content-disposition"),
        Some("attachment; filename=\"audit-server-2020-01-01-2020-12-31.ndjson\"")
    );
    let ids: Vec<Uuid> = events(response.bytes())
        .iter()
        .map(|event| event.id)
        .collect();
    assert_eq!(ids, [server]);
    assert!(!ids.contains(&foreign));
    fixture.cleanup().await;
}

#[tokio::test]
async fn export_needs_a_team_or_enterprise_license_that_is_active_or_in_grace() {
    let fixture = Fixture::new(PREFIX).await;
    let path = format!("{}?{WHOLE_YEAR}", organization_path(fixture.organization));
    let refused = |response: crate::common::TestResponse| {
        response.assert_status(StatusCode::FORBIDDEN);
        assert_eq!(response.json_value()["code"], LICENSE_REQUIRED);
    };

    refused(get(&fixture, &path, &fixture.owner).await);
    refused(
        get(
            &fixture,
            &format!("{SERVER_PATH}?{WHOLE_YEAR}"),
            &fixture.owner,
        )
        .await,
    );

    fixture.license(Tier::Team).await;
    get(&fixture, &path, &fixture.owner)
        .await
        .assert_status(StatusCode::OK);

    fixture.install(&sso::grace(Tier::Enterprise)).await;
    get(&fixture, &path, &fixture.owner)
        .await
        .assert_status(StatusCode::OK);

    fixture.store_license(&sso::lapsed(Tier::Enterprise)).await;
    refused(get(&fixture, &path, &fixture.owner).await);
    refused(
        get(
            &fixture,
            &format!("{SERVER_PATH}?{WHOLE_YEAR}"),
            &fixture.owner,
        )
        .await,
    );

    assert_eq!(exports(&fixture).await.len(), 2);
    fixture.cleanup().await;
}

#[tokio::test]
async fn only_owners_and_admins_export_and_outsiders_are_told_nothing() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license(Tier::Team).await;
    let path = format!("{}?{WHOLE_YEAR}", organization_path(fixture.organization));
    let admin = fixture.member(OrgRole::Admin, None).await;
    let member = fixture.member(OrgRole::Member, None).await;
    let outsider = fixture.stranger().await;

    get(&fixture, &path, &admin)
        .await
        .assert_status(StatusCode::OK);
    get(&fixture, &path, &member)
        .await
        .assert_status(StatusCode::FORBIDDEN);
    get(&fixture, &path, &outsider)
        .await
        .assert_status(StatusCode::NOT_FOUND);
    get(&fixture, &format!("{SERVER_PATH}?{WHOLE_YEAR}"), &admin)
        .await
        .assert_status(StatusCode::FORBIDDEN);
    fixture
        .sso
        .client
        .get(&path)
        .await
        .assert_status(StatusCode::UNAUTHORIZED);

    assert_eq!(exports(&fixture).await.len(), 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_invalid_format_or_range_is_refused_with_its_code() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license(Tier::Team).await;
    let path = organization_path(fixture.organization);
    let cases = [
        (
            "format=xml&start_date=2020-01-01&end_date=2020-01-02",
            FORMAT_INVALID,
        ),
        (
            "format=&start_date=2020-01-01&end_date=2020-01-02",
            FORMAT_INVALID,
        ),
        ("end_date=2020-01-02", RANGE_INVALID),
        ("start_date=2020-01-01", RANGE_INVALID),
        ("start_date=soon&end_date=2020-01-02", RANGE_INVALID),
        ("start_date=2020-01-03&end_date=2020-01-02", RANGE_INVALID),
    ];
    for (query, code) in cases {
        let response = get(&fixture, &format!("{path}?{query}"), &fixture.owner).await;
        response.assert_status(StatusCode::BAD_REQUEST);
        assert_eq!(response.json_value()["code"], code, "{query}");
    }
    assert!(exports(&fixture).await.is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn every_export_is_audited_before_it_streams() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license(Tier::Enterprise).await;

    get(
        &fixture,
        &format!(
            "{}?format=csv&start_date=2020-01-01&end_date=2020-01-31T12:00:00Z",
            organization_path(fixture.organization)
        ),
        &fixture.owner,
    )
    .await
    .assert_status(StatusCode::OK);
    get(
        &fixture,
        &format!("{SERVER_PATH}?{WHOLE_YEAR}"),
        &fixture.owner,
    )
    .await
    .assert_status(StatusCode::OK);

    let audits = exports(&fixture).await;
    assert_eq!(audits.len(), 2);
    let [organization, server] = &audits[..] else {
        unreachable!()
    };
    assert_eq!(organization.organization_id, Some(fixture.organization));
    assert_eq!(organization.actor_id, Some(fixture.owner.id));
    assert_eq!(organization.resource_type, resources::AUDIT_EXPORT);
    assert_eq!(organization.old_values, None);
    assert_eq!(
        organization.new_values,
        Some(json!({
            "format": "csv",
            "start_date": "2020-01-01T00:00:00Z",
            "end_date": "2020-01-31T12:00:00Z",
        }))
    );
    assert_eq!(server.organization_id, None);
    assert_eq!(server.actor_id, Some(fixture.owner.id));
    assert_eq!(
        server
            .new_values
            .as_ref()
            .map(|values| values["format"].clone()),
        Some(Value::from("ndjson"))
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_export_whose_audit_row_cannot_be_written_is_refused_without_a_body() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license(Tier::Team).await;
    seed(fixture.pool(), Some(fixture.organization), year_start(), 3).await;
    for statement in REFUSE_EXPORT_AUDIT {
        sqlx::query(statement)
            .execute(fixture.pool())
            .await
            .unwrap();
    }

    for query in [WHOLE_YEAR.to_owned(), format!("format=csv&{WHOLE_YEAR}")] {
        let response = get(
            &fixture,
            &format!("{}?{query}", organization_path(fixture.organization)),
            &fixture.owner,
        )
        .await;
        response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
        assert!(response.bytes().is_empty(), "{}", response.text());
    }
    let response = get(
        &fixture,
        &format!("{SERVER_PATH}?{WHOLE_YEAR}"),
        &fixture.owner,
    )
    .await;
    response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert!(response.bytes().is_empty(), "{}", response.text());
    fixture.cleanup().await;
}

#[tokio::test]
async fn csv_neutralises_formulas() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license(Tier::Team).await;
    sqlx::query(
        "INSERT INTO audit_logs (organization_id, actor_email, action, resource_type, user_agent, created_at)
         VALUES ($1, '+1@example.com', '=HYPERLINK(\"http://evil\")', 'member', '@SUM(A1)', $2)",
    )
    .bind(fixture.organization)
    .bind(year_start())
    .execute(fixture.pool())
    .await
    .unwrap();

    let response = get(
        &fixture,
        &format!(
            "{}?format=csv&{WHOLE_YEAR}",
            organization_path(fixture.organization)
        ),
        &fixture.owner,
    )
    .await;
    response.assert_status(StatusCode::OK);
    let text = response.text();
    assert!(text.contains(",\"'+1@example.com\","), "{text}");
    assert!(
        text.contains(",\"'=HYPERLINK(\"\"http://evil\"\")\","),
        "{text}"
    );
    assert!(text.contains(",\"'@SUM(A1)\","), "{text}");
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_community_csv_export_stays_ungated() {
    let fixture = Fixture::new(PREFIX).await;
    let at = year_start() + TimeDelta::days(1);
    let id = write(
        fixture.pool(),
        Some(fixture.organization),
        "member.added",
        at,
    )
    .await;

    let response = get(
        &fixture,
        &format!(
            "/api/organizations/{}/audit-logs/export?{WHOLE_YEAR}",
            fixture.organization
        ),
        &fixture.owner,
    )
    .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(response.header("content-type"), Some("text/csv"));
    assert!(response.text().contains(&id.to_string()));
    assert!(exports(&fixture).await.is_empty());
    fixture.cleanup().await;
}
