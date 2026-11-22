mod day;
mod filter;
mod line_row;
mod named_usage;
mod usage;
mod user_usage;
mod workspace_usage;

pub use day::Day;
pub use filter::Filter;
pub use named_usage::NamedUsage;
pub use usage::Usage;
pub use user_usage::UserUsage;
pub use workspace_usage::WorkspaceUsage;

use futures::{Stream, StreamExt, TryStreamExt, stream};
use sqlx::postgres::PgArguments;
use sqlx::query::QueryAs;
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use zone_enterprise::spend::{Line, Range};

use self::line_row::LineRow;
use crate::db::DbResult;

const SNAPSHOT: &str = "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY";

macro_rules! measured {
    () => {
        "COALESCE(SUM(record.spend_microdollars), 0)::bigint AS spend_microdollars, \
         COUNT(*)::bigint AS requests, \
         COALESCE(SUM(record.prompt_tokens), 0)::bigint AS prompt_tokens, \
         COALESCE(SUM(record.completion_tokens), 0)::bigint AS completion_tokens"
    };
}

macro_rules! filtered {
    () => {
        " FROM spend_records record \
         LEFT JOIN workspaces workspace ON workspace.id = record.workspace_id \
         LEFT JOIN users account ON account.id = record.user_id \
         WHERE record.organization_id = $1 AND record.occurred_at >= $2 \
         AND record.occurred_at < $3 \
         AND ($4::uuid IS NULL OR record.workspace_id = $4) \
         AND ($5::uuid IS NULL OR record.user_id = $5) "
    };
}

macro_rules! day {
    () => {
        "(record.occurred_at AT TIME ZONE 'UTC')::date"
    };
}

const TOTALS: &str = concat!("SELECT ", measured!(), filtered!());

const DAILY: &str = concat!(
    "WITH measured AS (SELECT ",
    day!(),
    " AS date, ",
    measured!(),
    filtered!(),
    "GROUP BY 1) \
     SELECT day::date AS date, \
     COALESCE(measured.spend_microdollars, 0) AS spend_microdollars, \
     COALESCE(measured.requests, 0) AS requests, \
     COALESCE(measured.prompt_tokens, 0) AS prompt_tokens, \
     COALESCE(measured.completion_tokens, 0) AS completion_tokens \
     FROM generate_series($6::date::timestamp, $7::date::timestamp, interval '1 day') AS day \
     LEFT JOIN measured ON measured.date = day::date \
     ORDER BY day"
);

const WORKSPACES: &str = concat!(
    "SELECT record.workspace_id AS id, workspace.name AS name, ",
    measured!(),
    filtered!(),
    "GROUP BY record.workspace_id, workspace.name \
     ORDER BY spend_microdollars DESC, name NULLS LAST, id NULLS LAST"
);

const USERS: &str = concat!(
    "SELECT record.user_id AS id, account.display_name AS name, account.email AS email, \
     record.automated AS automated, ",
    measured!(),
    filtered!(),
    "GROUP BY record.user_id, record.automated, account.display_name, account.email \
     ORDER BY spend_microdollars DESC, name NULLS LAST, email NULLS LAST, id NULLS LAST, \
     automated DESC"
);

const MODELS: &str = concat!(
    "SELECT record.model AS name, ",
    measured!(),
    filtered!(),
    "GROUP BY record.model ORDER BY spend_microdollars DESC, name"
);

const PROVIDERS: &str = concat!(
    "SELECT record.provider AS name, ",
    measured!(),
    filtered!(),
    "GROUP BY record.provider ORDER BY spend_microdollars DESC, name"
);

const LINES: &str = concat!(
    "SELECT ",
    day!(),
    " AS date, record.workspace_id, workspace.name AS workspace_name, record.user_id, \
     account.email AS user_email, record.automated, record.model, record.provider, ",
    measured!(),
    filtered!(),
    "GROUP BY 1, record.workspace_id, workspace.name, record.user_id, account.email, \
     record.automated, record.model, record.provider \
     ORDER BY date, workspace_name NULLS LAST, record.workspace_id NULLS LAST, \
     user_email NULLS LAST, record.user_id NULLS LAST, record.automated DESC, record.model, \
     record.provider"
);

/// A read-only transaction every part of one report reads, so the parts agree with each other
/// however much spend lands while they run.
pub async fn snapshot(pool: &PgPool) -> DbResult<Transaction<'static, Postgres>> {
    pool.begin_with(SNAPSHOT).await
}

pub async fn totals(connection: &mut PgConnection, filter: &Filter) -> DbResult<Usage> {
    bound(sqlx::query_as(TOTALS), filter)
        .fetch_one(connection)
        .await
}

/// One entry for every day of the range, zero on a day nothing was spent.
pub async fn daily(connection: &mut PgConnection, filter: &Filter) -> DbResult<Vec<Day>> {
    bound(sqlx::query_as(DAILY), filter)
        .bind(filter.range.from)
        .bind(filter.range.to)
        .fetch_all(connection)
        .await
}

pub async fn by_workspace(
    connection: &mut PgConnection,
    filter: &Filter,
) -> DbResult<Vec<WorkspaceUsage>> {
    bound(sqlx::query_as(WORKSPACES), filter)
        .fetch_all(connection)
        .await
}

pub async fn by_user(connection: &mut PgConnection, filter: &Filter) -> DbResult<Vec<UserUsage>> {
    bound(sqlx::query_as(USERS), filter)
        .fetch_all(connection)
        .await
}

pub async fn by_model(connection: &mut PgConnection, filter: &Filter) -> DbResult<Vec<NamedUsage>> {
    bound(sqlx::query_as(MODELS), filter)
        .fetch_all(connection)
        .await
}

pub async fn by_provider(
    connection: &mut PgConnection,
    filter: &Filter,
) -> DbResult<Vec<NamedUsage>> {
    bound(sqlx::query_as(PROVIDERS), filter)
        .fetch_all(connection)
        .await
}

/// The rows of a spend export, one per day, workspace, member, model and provider. Each day is
/// read whole on a connection of its own, which goes back to the pool before the day's rows are
/// sent, so a client that reads slowly never holds one.
pub fn lines(pool: PgPool, filter: Filter) -> impl Stream<Item = DbResult<Line>> + Send + 'static {
    stream::iter(filter.range.days())
        .then(move |day| {
            let pool = pool.clone();
            let page = Filter {
                range: Range { from: day, to: day },
                ..filter
            };
            async move {
                bound(sqlx::query_as::<_, LineRow>(LINES), &page)
                    .fetch_all(&pool)
                    .await
            }
        })
        .map_ok(|rows| stream::iter(rows.into_iter().map(|row| Ok(Line::from(row)))))
        .try_flatten()
}

fn bound<'q, O>(
    query: QueryAs<'q, Postgres, O, PgArguments>,
    filter: &Filter,
) -> QueryAs<'q, Postgres, O, PgArguments> {
    query
        .bind(filter.organization)
        .bind(filter.range.start())
        .bind(filter.range.end())
        .bind(filter.workspace)
        .bind(filter.user)
}
