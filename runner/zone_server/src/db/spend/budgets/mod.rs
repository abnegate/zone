mod announcement;
mod budget_row;
mod columns;
mod create_error;
mod crossing;
mod recipient;

pub use announcement::Announcement;
pub use budget_row::BudgetRow;
pub use create_error::CreateError;
pub use crossing::Crossing;
pub use recipient::Recipient;

use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;
use zone_enterprise::spend::{Budget, Month, Scope, State, Target};

use self::columns::Columns;
use crate::auth::{self, Grant, Permission};
use crate::db::DbResult;
use crate::db::organization_members::OrgRole;
use crate::db::workspace_members::WorkspaceRole;

pub const TARGET_KEY: &str = "budgets_target_key";

const WORKSPACE_NOTIFIED: [WorkspaceRole; 2] = [WorkspaceRole::Owner, WorkspaceRole::Admin];

/// The month's spend of each organization `$organizations` picks, once as a whole, once per
/// workspace and once per member. A row whose workspace or member is null and grouped is spend
/// that has none (a deleted workspace, automation), which only the organization's total counts.
/// A sum past `i64::MAX` is held there, as `Microdollars` saturates.
macro_rules! spending {
    ($organizations:literal) => {
        concat!(
            "spending AS (SELECT organization_id, workspace_id, user_id, \
             LEAST(SUM(spend_microdollars), 9223372036854775807)::bigint AS spent \
             FROM spend_records WHERE occurred_at >= $1 AND occurred_at < $2 AND organization_id ",
            $organizations,
            " GROUP BY GROUPING SETS ((organization_id), (organization_id, workspace_id), \
             (organization_id, user_id)) \
             HAVING (GROUPING(workspace_id) = 1 OR workspace_id IS NOT NULL) \
             AND (GROUPING(user_id) = 1 OR user_id IS NOT NULL))"
        )
    };
}

macro_rules! charged {
    () => {
        " LEFT JOIN spending ON spending.organization_id = budget.organization_id \
         AND spending.workspace_id IS NOT DISTINCT FROM budget.workspace_id \
         AND spending.user_id IS NOT DISTINCT FROM budget.user_id"
    };
}

macro_rules! described {
    () => {
        "budget.id, budget.organization_id, budget.workspace_id, budget.user_id, \
         budget.limit_microdollars, budget.threshold_percent, budget.hard, budget.updated_at, \
         workspace.name AS workspace_name, account.display_name AS user_name, \
         account.email AS user_email"
    };
}

macro_rules! named {
    () => {
        " LEFT JOIN workspaces workspace ON workspace.id = budget.workspace_id \
         LEFT JOIN users account ON account.id = budget.user_id"
    };
}

macro_rules! selected {
    ($source:literal, $tail:literal) => {
        concat!(
            " SELECT ",
            described!(),
            ", COALESCE(spending.spent, 0) AS spent_microdollars FROM ",
            $source,
            " budget",
            named!(),
            charged!(),
            " ",
            $tail
        )
    };
}

macro_rules! settled {
    ($filter:literal) => {
        concat!(
            "measured AS (SELECT budget.id, budget.limit_microdollars, budget.threshold_percent, \
             budget.warning_sent_month, budget.exhaustion_sent_month, \
             COALESCE(spending.spent, 0) AS spent FROM budgets budget",
            charged!(),
            $filter,
            "), settled AS (SELECT id, spent, limit_microdollars, threshold_percent, \
             CASE WHEN spent >= limit_microdollars THEN $3::date END AS exhausted_month, \
             CASE WHEN spent::numeric * 100 >= limit_microdollars::numeric * threshold_percent \
             THEN warning_sent_month END AS warning_sent_month, \
             CASE WHEN spent >= limit_microdollars THEN exhaustion_sent_month END \
             AS exhaustion_sent_month, \
             CASE WHEN spent >= limit_microdollars THEN exhaustion_sent_month IS DISTINCT FROM $3 \
             ELSE spent::numeric * 100 >= limit_microdollars::numeric * threshold_percent \
             AND warning_sent_month IS DISTINCT FROM $3 END AS pending FROM measured)"
        )
    };
}

/// Skips a budget whose limit or threshold changed since it was measured; under read committed a
/// concurrent edit is seen here, after waiting for its lock, though not by the rest of the
/// statement.
macro_rules! written {
    ($touched:literal) => {
        concat!(
            " UPDATE budgets budget SET exhausted_month = settled.exhausted_month, \
             warning_sent_month = settled.warning_sent_month, \
             exhaustion_sent_month = settled.exhaustion_sent_month \
             FROM settled WHERE budget.id = settled.id \
             AND budget.limit_microdollars = settled.limit_microdollars \
             AND budget.threshold_percent = settled.threshold_percent \
             AND ((budget.exhausted_month, budget.warning_sent_month, budget.exhaustion_sent_month) \
             IS DISTINCT FROM \
             (settled.exhausted_month, settled.warning_sent_month, settled.exhaustion_sent_month)",
            $touched,
            ")"
        )
    };
}

pub async fn list(
    executor: impl PgExecutor<'_>,
    organization: Uuid,
    month: Month,
) -> DbResult<Vec<BudgetRow>> {
    let rows: Vec<Columns> = sqlx::query_as(concat!(
        "WITH ",
        spending!("= $3"),
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
    rows.into_iter().map(BudgetRow::try_from).collect()
}

pub async fn lock(
    connection: &mut PgConnection,
    organization: Uuid,
    id: Uuid,
    month: Month,
) -> DbResult<Option<BudgetRow>> {
    let row: Option<Columns> = sqlx::query_as(concat!(
        "WITH ",
        spending!("= $3"),
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
    row.map(BudgetRow::try_from).transpose()
}

pub async fn create(
    connection: &mut PgConnection,
    organization: Uuid,
    target: &Target,
    budget: &Budget,
    actor: Uuid,
    month: Month,
) -> Result<BudgetRow, CreateError> {
    let row: Columns = sqlx::query_as(concat!(
        "WITH budget AS (INSERT INTO budgets (organization_id, workspace_id, user_id, \
         limit_microdollars, threshold_percent, hard, created_by, updated_by) \
         VALUES ($3, $4, $5, $6, $7, $8, $9, $9) RETURNING *), ",
        spending!("= $3"),
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
    Ok(BudgetRow::try_from(row)?)
}

pub async fn update(
    connection: &mut PgConnection,
    organization: Uuid,
    id: Uuid,
    budget: &Budget,
    actor: Uuid,
    month: Month,
) -> DbResult<BudgetRow> {
    let row: Columns = sqlx::query_as(concat!(
        "WITH budget AS (UPDATE budgets SET limit_microdollars = $5, threshold_percent = $6, \
         hard = $7, \
         updated_by = CASE WHEN (limit_microdollars, threshold_percent, hard) \
         IS DISTINCT FROM ($5, $6, $7) THEN $8 ELSE updated_by END, \
         updated_at = CASE WHEN (limit_microdollars, threshold_percent, hard) \
         IS DISTINCT FROM ($5, $6, $7) THEN now() ELSE updated_at END \
         WHERE organization_id = $3 AND id = $4 RETURNING *), ",
        spending!("= $3"),
        selected!("budget", "")
    ))
    .bind(month.start())
    .bind(month.end())
    .bind(organization)
    .bind(id)
    .bind(budget.limit.0)
    .bind(i32::from(budget.threshold_percent))
    .bind(budget.hard)
    .bind(actor)
    .fetch_one(connection)
    .await?;
    BudgetRow::try_from(row)
}

pub async fn delete(connection: &mut PgConnection, organization: Uuid, id: Uuid) -> DbResult<bool> {
    let deleted = sqlx::query("DELETE FROM budgets WHERE organization_id = $1 AND id = $2")
        .bind(organization)
        .bind(id)
        .execute(connection)
        .await?
        .rows_affected();
    Ok(deleted > 0)
}

pub async fn refresh(connection: &mut PgConnection, id: Uuid, month: Month) -> DbResult<()> {
    sqlx::query(concat!(
        "WITH ",
        spending!("= (SELECT organization_id FROM budgets WHERE id = $4)"),
        ", ",
        settled!(" WHERE budget.id = $4"),
        written!("")
    ))
    .bind(month.start())
    .bind(month.end())
    .bind(month.date())
    .bind(id)
    .execute(connection)
    .await?;
    Ok(())
}

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

/// A crossing is answered from the row its write returned, so a budget whose limit or threshold
/// changed while the statement waited on it answers nothing: the next sweep measures it again. A
/// pending crossing is written even when nothing in it changed, to take that lock.
pub async fn settle(executor: impl PgExecutor<'_>, month: Month) -> DbResult<Vec<Crossing>> {
    let rows: Vec<Columns> = sqlx::query_as(concat!(
        "WITH ",
        spending!("IN (SELECT organization_id FROM budgets)"),
        ", ",
        settled!(""),
        ", written AS (",
        written!(" OR settled.pending"),
        " RETURNING budget.id, budget.organization_id, budget.workspace_id, budget.user_id, \
         budget.limit_microdollars, budget.threshold_percent, budget.hard, budget.updated_at, \
         settled.spent, settled.pending) SELECT ",
        described!(),
        ", budget.spent AS spent_microdollars FROM written budget",
        named!(),
        " WHERE budget.pending ORDER BY budget.organization_id, budget.id"
    ))
    .bind(month.start())
    .bind(month.end())
    .bind(month.date())
    .fetch_all(executor)
    .await?;
    let mut crossings = Vec::with_capacity(rows.len());
    for row in rows {
        let row = BudgetRow::try_from(row)?;
        let announcement = match row.state() {
            State::Under => continue,
            State::Warning => Announcement::Warning,
            State::Exhausted => Announcement::Exhaustion,
        };
        crossings.push(Crossing { row, announcement });
    }
    Ok(crossings)
}

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

pub async fn recipients(
    executor: impl PgExecutor<'_>,
    organization: Uuid,
    workspace: Option<Uuid>,
) -> DbResult<Vec<Recipient>> {
    sqlx::query_as(
        "SELECT email, name FROM (SELECT DISTINCT ON (lower(account.email)) account.email, \
         COALESCE(account.display_name, account.email) AS name, people.tier, people.joined \
         FROM (SELECT member.user_id, 0 AS tier, member.created_at AS joined \
         FROM organization_members member \
         WHERE member.organization_id = $1 AND member.is_active \
         AND CASE WHEN member.custom_role_id IS NULL THEN member.role = ANY($3) \
         ELSE EXISTS (SELECT 1 FROM custom_roles custom_role \
         JOIN custom_role_permissions granted ON granted.role_id = custom_role.id \
         WHERE custom_role.id = member.custom_role_id AND custom_role.organization_id = $1 \
         AND custom_role.scope = $5 AND granted.permission = $6) END \
         UNION ALL SELECT user_id, 1, created_at FROM workspace_members \
         WHERE workspace_id = $2 AND is_active AND role = ANY($4)) people \
         JOIN users account ON account.id = people.user_id \
         WHERE account.is_active IS NOT FALSE \
         ORDER BY lower(account.email), people.tier, people.joined) recipient \
         ORDER BY tier, joined, email",
    )
    .bind(organization)
    .bind(workspace)
    .bind(readers())
    .bind(WORKSPACE_NOTIFIED.map(|role| role.as_str()).to_vec())
    .bind(auth::Scope::Organization.name())
    .bind(Permission::SpendRead.name())
    .fetch_all(executor)
    .await
}

/// The built-in organization roles that hold `spend:read`. A custom role holds it when its stored
/// permissions name it.
fn readers() -> Vec<&'static str> {
    OrgRole::ALL
        .into_iter()
        .filter(|role| Grant::organization(*role).allows(Permission::SpendRead))
        .map(|role| role.as_str())
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organization_owners_and_admins_read_spend() {
        assert_eq!(
            readers(),
            [OrgRole::Admin.as_str(), OrgRole::Owner.as_str()]
        );
    }

    #[test]
    fn an_organization_role_naming_spend_read_holds_it_and_a_workspace_role_does_not() {
        assert_eq!(Permission::SpendRead.requires(), None);
        let stored = [Permission::SpendRead.name()];
        assert!(Grant::stored(auth::Scope::Organization, stored).allows(Permission::SpendRead));
        assert!(!Grant::stored(auth::Scope::Workspace, stored).allows(Permission::SpendRead));
    }
}
