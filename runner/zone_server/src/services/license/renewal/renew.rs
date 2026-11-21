use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::{License, Status};

use super::{Answer, Client, Outcome, accepts, due};
use crate::db::audit::{AuditContext, actions, log_action, resources};
use crate::db::{DbResult, license_renewal};
use crate::services::license::{Change, Licenses, summary};
use crate::state::AppState;

pub const INTERVAL_SECONDS: u64 = 86_400;
pub const FIRST_ATTEMPT_DELAY: Duration = Duration::from_secs(60);

pub async fn run(state: &AppState, client: &Client) -> DbResult<Option<Outcome>> {
    renew(state.licenses(), state.db(), client, Utc::now()).await
}

/// `None` when nothing was asked, or when the stored key changed while the answer was on its
/// way, so the answer no longer applies.
pub async fn renew(
    licenses: &Licenses,
    pool: &PgPool,
    client: &Client,
    now: DateTime<Utc>,
) -> DbResult<Option<Outcome>> {
    let Some(license) = licenses.settled(pool).await? else {
        return Ok(None);
    };
    if !due(&license, now) {
        return Ok(None);
    }
    let outcome = match client.renew(license.text()).await {
        Answer::Renewed(text) => return install(licenses, pool, &license, &text, now).await,
        Answer::Current => Outcome::Current,
        Answer::Ended => Outcome::Ended,
        Answer::Refused => Outcome::Refused,
        Answer::Unreachable => Outcome::Unreachable,
        Answer::Malformed => Outcome::Rejected,
    };
    settle(pool, &license, outcome, now).await
}

async fn install(
    licenses: &Licenses,
    pool: &PgPool,
    license: &License,
    text: &str,
    now: DateTime<Utc>,
) -> DbResult<Option<Outcome>> {
    let renewed = match licenses.keyring().verify(text) {
        Ok(renewed) => renewed,
        Err(error) => {
            tracing::warn!(
                license = %license.id(),
                code = error.code(),
                "The renewed license key does not verify"
            );
            return settle(pool, license, Outcome::Rejected, now).await;
        }
    };
    if !accepts(license, &renewed) {
        tracing::warn!(
            license = %license.id(),
            renewed = %renewed.id(),
            "The renewed license key does not continue the installed license"
        );
        return settle(pool, license, Outcome::Rejected, now).await;
    }
    if renewed.status(now) == Status::Lapsed {
        tracing::warn!(
            license = %license.id(),
            renewed = %renewed.id(),
            "The renewed license key has already lapsed"
        );
        return settle(pool, license, Outcome::Rejected, now).await;
    }
    let Some(change) = licenses
        .renew(pool, license.text(), renewed.clone())
        .await?
    else {
        tracing::info!(
            license = %license.id(),
            renewed = %renewed.id(),
            "The stored license changed while it was being renewed, so the renewed key was dropped"
        );
        return Ok(None);
    };
    audit(pool, &change).await;
    license_renewal::record(pool, change.current.text(), Outcome::Renewed, now).await?;
    report(Outcome::Renewed, license.id(), Some(change.current.id()));
    Ok(Some(Outcome::Renewed))
}

async fn settle(
    pool: &PgPool,
    license: &License,
    outcome: Outcome,
    now: DateTime<Utc>,
) -> DbResult<Option<Outcome>> {
    license_renewal::record(pool, license.text(), outcome, now).await?;
    report(outcome, license.id(), None);
    Ok(Some(outcome))
}

async fn audit(pool: &PgPool, change: &Change) {
    let context = AuditContext {
        org_id: None,
        workspace_id: None,
        actor_id: None,
        actor_email: None,
        ip_address: None,
        user_agent: None,
    };
    if let Err(error) = log_action(
        pool,
        &context,
        actions::LICENSE_RENEWED,
        resources::LICENSE,
        Some(change.current.id()),
        change.previous.license().map(summary),
        Some(summary(&change.current)),
    )
    .await
    {
        tracing::error!(%error, action = actions::LICENSE_RENEWED, "Failed to record audit log");
    }
}

fn report(outcome: Outcome, license: Uuid, renewed: Option<Uuid>) {
    let outcome = outcome.as_str();
    let renewed = renewed.map(tracing::field::display);
    tracing::info!(outcome, %license, renewed, "Asked the license server to renew the license");
}
