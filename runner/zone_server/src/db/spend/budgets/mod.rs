mod announcement;
mod budget_row;
mod create_error;
mod crossing;
mod record;

pub use announcement::Announcement;
pub use budget_row::BudgetRow;
pub use create_error::CreateError;
pub use crossing::Crossing;

use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;
use zone_enterprise::spend::{Budget, Month, Scope, State, Target};

use self::record::Record;
use crate::db::DbResult;

pub const TARGET_KEY: &str = "budgets_target_key";

macro_rules! spending {
    ($filter:literal) => {
        concat!(
            "spending AS (SELECT organization_id, workspace_id, user_id, \
             SUM(spend_microdollars) AS spent FROM spend_records \
             WHERE occurred_at >= $1 AND occurred_at < $2",
            $filter,
            " GROUP BY organization_id, workspace_id, user_id)"
        )
    };
}

macro_rules! spent {
    () => {
        "COALESCE((SELECT SUM(spending.spent) FROM spending \
         WHERE spending.organization_id = budget.organization_id \
         AND (budget.workspace_id IS NULL OR spending.workspace_id = budget.workspace_id) \
         AND (budget.user_id IS NULL OR spending.user_id = budget.user_id)), 0)::bigint"
    };
}

macro_rules! selected {
    ($source:literal, $tail:literal) => {
        concat!(
            " SELECT budget.id, budget.organization_id, budget.workspace_id, budget.user_id, \
             budget.limit_microdollars, budget.threshold_percent, budget.hard, budget.updated_at, \
             workspace.name AS workspace_name, account.display_name AS user_name, \
             account.email AS user_email, ",
            spent!(),
            " AS spent_microdollars FROM ",
            $source,
            " budget \
             LEFT JOIN workspaces workspace ON workspace.id = budget.workspace_id \
             LEFT JOIN users account ON account.id = budget.user_id ",
            $tail
        )
    };
}

macro_rules! settled {
    ($filter:literal) => {
        concat!(
            "measured AS (SELECT budget.id, budget.limit_microdollars, budget.threshold_percent, \
             budget.warning_sent_month, budget.exhaustion_sent_month, ",
            spent!(),
            " AS spent FROM budgets budget",
            $filter,
            "), settled AS (SELECT id, spent, limit_microdollars, threshold_percent, \
             CASE WHEN spent >= limit_microdollars THEN $3::date END AS exhausted_month, \
             CASE WHEN spent::numeric * 100 >= limit_microdollars::numeric * threshold_percent \
             THEN warning_sent_month END AS warning_sent_month, \
             CASE WHEN spent >= limit_microdollars THEN exhaustion_sent_month END \
             AS exhaustion_sent_month FROM measured)"
        )
    };
}

macro_rules! written {
    () => {
        " UPDATE budgets budget SET exhausted_month = settled.exhausted_month, \
         warning_sent_month = settled.warning_sent_month, \
         exhaustion_sent_month = settled.exhaustion_sent_month \
         FROM settled WHERE budget.id = settled.id \
         AND budget.limit_microdollars = settled.limit_microdollars \
         AND budget.threshold_percent = settled.threshold_percent \
         AND (budget.exhausted_month, budget.warning_sent_month, budget.exhaustion_sent_month) \
         IS DISTINCT FROM \
         (settled.exhausted_month, settled.warning_sent_month, settled.exhaustion_sent_month)"
    };
}

pub async fn list(
    executor: impl PgExecutor<'_>,
    organization: Uuid,
    month: Month,
) -> DbResult<Vec<BudgetRow>> {
    let records: Vec<Record> = sqlx::query_as(concat!(
        "WITH ",
        spending!(" AND organization_id = $3"),
        selected!(
            "budgets",
            "WHERE budget.organization_id = $3 \
             ORDER BY budget.workspace_id IS NOT NULL OR budget.user_id IS NOT NULL, \
             budget.user_id IS NOT NULL, workspace.name, account.email, budget.id"
        )
    ))
    .bind(month.start())
    .bind(month.end())
    .bind(organization)
    .fetch_all(executor)
    .await?;
    records.into_iter().map(BudgetRow::try_from).collect()
}

pub async fn get(
    executor: impl PgExecutor<'_>,
    organization: Uuid,
    id: Uuid,
    month: Month,
) -> DbResult<Option<BudgetRow>> {
    let record: Option<Record> = sqlx::query_as(concat!(
        "WITH ",
        spending!(" AND organization_id = $3"),
        selected!(
            "budgets",
            "WHERE budget.organization_id = $3 AND budget.id = $4"
        )
    ))
    .bind(month.start())
    .bind(month.end())
    .bind(organization)
    .bind(id)
    .fetch_optional(executor)
    .await?;
    record.map(BudgetRow::try_from).transpose()
}

/// Locks the budget until the transaction ends; `spent` is this month's.
pub async fn lock(
    connection: &mut PgConnection,
    organization: Uuid,
    id: Uuid,
) -> DbResult<Option<BudgetRow>> {
    let month = Month::current();
    let record: Option<Record> = sqlx::query_as(concat!(
        "WITH ",
        spending!(" AND organization_id = $3"),
        selected!(
            "budgets",
            "WHERE budget.organization_id = $3 AND budget.id = $4 FOR UPDATE OF budget"
        )
    ))
    .bind(month.start())
    .bind(month.end())
    .bind(organization)
    .bind(id)
    .fetch_optional(connection)
    .await?;
    record.map(BudgetRow::try_from).transpose()
}

pub async fn create(
    connection: &mut PgConnection,
    organization: Uuid,
    target: &Target,
    budget: &Budget,
    actor: Uuid,
    month: Month,
) -> Result<BudgetRow, CreateError> {
    let record: Record = sqlx::query_as(concat!(
        "WITH budget AS (INSERT INTO budgets (organization_id, workspace_id, user_id, \
         limit_microdollars, threshold_percent, hard, created_by, updated_by) \
         VALUES ($3, $4, $5, $6, $7, $8, $9, $9) RETURNING *), ",
        spending!(" AND organization_id = $3"),
        selected!("budget", "")
    ))
    .bind(month.start())
    .bind(month.end())
    .bind(organization)
    .bind(target.workspace)
    .bind(target.user)
    .bind(budget.limit.0)
    .bind(i32::from(budget.threshold_percent))
    .bind(budget.hard)
    .bind(actor)
    .fetch_one(connection)
    .await
    .map_err(classify)?;
    Ok(BudgetRow::try_from(record)?)
}

/// Writes `budget` over the stored one, keeping `updated_at` and `updated_by` when it changes
/// nothing.
pub async fn update(
    connection: &mut PgConnection,
    id: Uuid,
    budget: &Budget,
    actor: Uuid,
    month: Month,
) -> DbResult<BudgetRow> {
    let record: Record = sqlx::query_as(concat!(
        "WITH budget AS (UPDATE budgets SET limit_microdollars = $4, threshold_percent = $5, \
         hard = $6, \
         updated_by = CASE WHEN (limit_microdollars, threshold_percent, hard) \
         IS DISTINCT FROM ($4, $5, $6) THEN $7 ELSE updated_by END, \
         updated_at = CASE WHEN (limit_microdollars, threshold_percent, hard) \
         IS DISTINCT FROM ($4, $5, $6) THEN now() ELSE updated_at END \
         WHERE id = $3 RETURNING *), ",
        spending!(" AND organization_id = (SELECT organization_id FROM budget)"),
        selected!("budget", "")
    ))
    .bind(month.start())
    .bind(month.end())
    .bind(id)
    .bind(budget.limit.0)
    .bind(i32::from(budget.threshold_percent))
    .bind(budget.hard)
    .bind(actor)
    .fetch_one(connection)
    .await?;
    BudgetRow::try_from(record)
}

pub async fn delete(connection: &mut PgConnection, id: Uuid) -> DbResult<bool> {
    let deleted = sqlx::query("DELETE FROM budgets WHERE id = $1")
        .bind(id)
        .execute(connection)
        .await?
        .rows_affected();
    Ok(deleted > 0)
}

/// Marks the budget exhausted for `month` while its spend is at the limit, and re-arms each
/// notice whose condition no longer holds.
pub async fn refresh(connection: &mut PgConnection, id: Uuid, month: Month) -> DbResult<()> {
    sqlx::query(concat!(
        "WITH ",
        spending!(" AND organization_id = (SELECT organization_id FROM budgets WHERE id = $4)"),
        ", ",
        settled!(" WHERE budget.id = $4"),
        written!()
    ))
    .bind(month.start())
    .bind(month.end())
    .bind(month.date())
    .bind(id)
    .execute(connection)
    .await?;
    Ok(())
}

/// The broadest exhausted hard budget that covers the request: organization, then workspace,
/// then user.
pub async fn exhausted(
    executor: impl PgExecutor<'_>,
    organization: Uuid,
    workspace: Uuid,
    user: Option<Uuid>,
    month: Month,
) -> DbResult<Option<Scope>> {
    let covering: Option<(Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        "SELECT workspace_id, user_id FROM budgets \
         WHERE organization_id = $1 AND hard AND exhausted_month = $2 \
         AND ((workspace_id IS NULL AND user_id IS NULL) OR workspace_id = $3 OR user_id = $4) \
         ORDER BY workspace_id IS NOT NULL OR user_id IS NOT NULL, user_id IS NOT NULL \
         LIMIT 1",
    )
    .bind(organization)
    .bind(month.date())
    .bind(workspace)
    .bind(user)
    .fetch_optional(executor)
    .await?;
    Ok(covering.map(|(workspace, user)| Target::of(workspace, user).scope))
}

/// Refreshes every budget for `month` in one statement, then answers those past their threshold
/// or limit whose notice hasn't gone out this month. An exhausted budget answers only its
/// exhaustion.
pub async fn settle(executor: impl PgExecutor<'_>, month: Month) -> DbResult<Vec<Crossing>> {
    let records: Vec<Record> = sqlx::query_as(concat!(
        "WITH ",
        spending!(""),
        ", ",
        settled!(""),
        ", written AS (",
        written!(),
        ")",
        selected!(
            "budgets",
            "JOIN settled ON settled.id = budget.id \
             WHERE (settled.spent >= budget.limit_microdollars \
             AND settled.exhaustion_sent_month IS DISTINCT FROM $3) \
             OR (settled.spent < budget.limit_microdollars \
             AND settled.spent::numeric * 100 >= \
             budget.limit_microdollars::numeric * budget.threshold_percent \
             AND settled.warning_sent_month IS DISTINCT FROM $3) \
             ORDER BY budget.organization_id, budget.id"
        )
    ))
    .bind(month.start())
    .bind(month.end())
    .bind(month.date())
    .fetch_all(executor)
    .await?;
    let mut crossings = Vec::with_capacity(records.len());
    for record in records {
        let row = BudgetRow::try_from(record)?;
        let announcement = match row.state() {
            State::Under => continue,
            State::Warning => Announcement::Warning,
            State::Exhausted => Announcement::Exhaustion,
        };
        crossings.push(Crossing { row, announcement });
    }
    Ok(crossings)
}

/// Records that the budget's notice went out for `month`.
pub async fn announce(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    announcement: Announcement,
    month: Month,
) -> DbResult<()> {
    let statement = match announcement {
        Announcement::Warning => "UPDATE budgets SET warning_sent_month = $2 WHERE id = $1",
        Announcement::Exhaustion => "UPDATE budgets SET exhaustion_sent_month = $2 WHERE id = $1",
    };
    sqlx::query(statement)
        .bind(id)
        .bind(month.date())
        .execute(executor)
        .await?;
    Ok(())
}

fn classify(error: sqlx::Error) -> CreateError {
    match error.as_database_error() {
        Some(database)
            if database.is_unique_violation() && database.constraint() == Some(TARGET_KEY) =>
        {
            CreateError::Exists
        }
        _ => CreateError::Database(error),
    }
}
