use chrono::{DateTime, SubsecRound, TimeDelta, Utc};
use sqlx::PgPool;
use zone_server::db::license_renewal::{self, LicenseRenewal};
use zone_server::db::{licenses, migrations};
use zone_server::services::license::renewal::Outcome;

use crate::common::Database;

const PREFIX: &str = "license_renewal_store";
const CHECK_VIOLATION: &str = "23514";
const OUTCOME_CHECK: &str = "licenses_renewal_outcome_check";
const STORED: &str = "stored";
const OTHER: &str = "other";
const RENEWED: &str = "renewed";
const UNKNOWN_OUTCOME: &str = "expired";

struct Fixture {
    database: Database,
}

impl Fixture {
    async fn new() -> Self {
        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool).await.unwrap();
        Self { database }
    }

    async fn stored() -> Self {
        let fixture = Self::new().await;
        licenses::replace(fixture.pool(), STORED).await.unwrap();
        fixture
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    async fn status(&self) -> Option<LicenseRenewal> {
        license_renewal::get(self.pool()).await.unwrap()
    }

    async fn columns(&self) -> Option<(Option<DateTime<Utc>>, Option<String>)> {
        sqlx::query_as("SELECT renewal_attempted_at, renewal_outcome FROM licenses")
            .fetch_optional(self.pool())
            .await
            .unwrap()
    }

    async fn rows(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM licenses")
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

fn attempted() -> DateTime<Utc> {
    (Utc::now() - TimeDelta::minutes(5)).trunc_subsecs(6)
}

#[tokio::test]
async fn nothing_is_reported_before_an_attempt() {
    let fixture = Fixture::new().await;
    assert_eq!(fixture.status().await, None);

    licenses::replace(fixture.pool(), STORED).await.unwrap();

    assert_eq!(fixture.status().await, None);
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_attempt_is_recorded_against_the_stored_key() {
    let fixture = Fixture::stored().await;
    let at = attempted();

    assert!(
        license_renewal::record(fixture.pool(), STORED, Outcome::Current, at)
            .await
            .unwrap()
    );

    assert_eq!(
        fixture.status().await,
        Some(LicenseRenewal {
            attempted_at: at,
            outcome: Outcome::Current.as_str().to_string(),
        })
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_attempt_against_another_key_records_nothing() {
    let fixture = Fixture::stored().await;

    assert!(
        !license_renewal::record(fixture.pool(), OTHER, Outcome::Ended, attempted())
            .await
            .unwrap()
    );

    assert_eq!(fixture.status().await, None);
    assert_eq!(
        licenses::get(fixture.pool()).await.unwrap().as_deref(),
        Some(STORED)
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_attempt_with_no_key_stored_records_nothing() {
    let fixture = Fixture::new().await;

    assert!(
        !license_renewal::record(fixture.pool(), STORED, Outcome::Ended, attempted())
            .await
            .unwrap()
    );

    assert_eq!(fixture.rows().await, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn replacing_the_key_clears_the_status() {
    let fixture = Fixture::stored().await;
    license_renewal::record(fixture.pool(), STORED, Outcome::Unreachable, attempted())
        .await
        .unwrap();

    licenses::replace(fixture.pool(), OTHER).await.unwrap();

    assert_eq!(fixture.status().await, None);
    assert_eq!(fixture.columns().await, Some((None, None)));
    fixture.cleanup().await;
}

#[tokio::test]
async fn removing_the_key_clears_the_status() {
    let fixture = Fixture::stored().await;
    license_renewal::record(fixture.pool(), STORED, Outcome::Refused, attempted())
        .await
        .unwrap();

    licenses::remove(fixture.pool()).await.unwrap();

    assert_eq!(fixture.status().await, None);
    licenses::replace(fixture.pool(), STORED).await.unwrap();
    assert_eq!(fixture.status().await, None);
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_unknown_outcome_violates_the_check() {
    let fixture = Fixture::stored().await;

    let error = sqlx::query(
        "UPDATE licenses SET renewal_attempted_at = $1, renewal_outcome = $2 WHERE key = $3",
    )
    .bind(attempted())
    .bind(UNKNOWN_OUTCOME)
    .bind(STORED)
    .execute(fixture.pool())
    .await
    .unwrap_err();

    let error = error.as_database_error().expect("a database error");
    assert_eq!(error.code().as_deref(), Some(CHECK_VIOLATION));
    assert_eq!(error.constraint(), Some(OUTCOME_CHECK));
    assert_eq!(fixture.status().await, None);
    fixture.cleanup().await;
}

#[tokio::test]
async fn every_outcome_is_accepted_by_the_check() {
    let fixture = Fixture::stored().await;

    for outcome in Outcome::ALL {
        let at = attempted();
        assert!(
            license_renewal::record(fixture.pool(), STORED, outcome, at)
                .await
                .unwrap(),
            "{outcome:?}"
        );
        let status = fixture.status().await.expect("the attempt was recorded");
        assert_eq!(Outcome::parse(&status.outcome), Some(outcome));
        assert_eq!(status.attempted_at, at);
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn swapping_from_the_stored_key_installs_and_clears_the_status() {
    let fixture = Fixture::stored().await;
    license_renewal::record(fixture.pool(), STORED, Outcome::Current, attempted())
        .await
        .unwrap();
    let before: DateTime<Utc> = sqlx::query_scalar("SELECT updated_at FROM licenses")
        .fetch_one(fixture.pool())
        .await
        .unwrap();

    assert!(
        licenses::replace_if(fixture.pool(), STORED, RENEWED)
            .await
            .unwrap()
    );

    let after: DateTime<Utc> = sqlx::query_scalar("SELECT updated_at FROM licenses")
        .fetch_one(fixture.pool())
        .await
        .unwrap();
    assert_eq!(
        licenses::get(fixture.pool()).await.unwrap().as_deref(),
        Some(RENEWED)
    );
    assert_eq!(fixture.columns().await, Some((None, None)));
    assert!(after > before, "{after} is not after {before}");
    assert_eq!(fixture.rows().await, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn swapping_from_another_key_leaves_the_key_and_status() {
    let fixture = Fixture::stored().await;
    let at = attempted();
    license_renewal::record(fixture.pool(), STORED, Outcome::Ended, at)
        .await
        .unwrap();

    assert!(
        !licenses::replace_if(fixture.pool(), OTHER, RENEWED)
            .await
            .unwrap()
    );

    assert_eq!(
        licenses::get(fixture.pool()).await.unwrap().as_deref(),
        Some(STORED)
    );
    assert_eq!(
        fixture.status().await,
        Some(LicenseRenewal {
            attempted_at: at,
            outcome: Outcome::Ended.as_str().to_string(),
        })
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn swapping_with_no_key_stored_inserts_nothing() {
    let fixture = Fixture::new().await;

    assert!(
        !licenses::replace_if(fixture.pool(), STORED, RENEWED)
            .await
            .unwrap()
    );

    assert_eq!(fixture.rows().await, 0);
    assert_eq!(licenses::get(fixture.pool()).await.unwrap(), None);
    fixture.cleanup().await;
}
