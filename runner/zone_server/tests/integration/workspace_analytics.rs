//! The console's workspace analytics overview: how the agents' runs went over
//! a day, a week, a month or a quarter, which sources and models they ran for,
//! what the tasks behind them shipped, and how their pull requests fared.

use crate::common::TestClient;
use crate::common::test_email;
use crate::common::test_password;

use axum::http::StatusCode;
use chrono::Duration;
use chrono::NaiveDateTime;
use chrono::Utc;
use serde_json::Value;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use zone_server::db::workspace_members;
use zone_server::db::workspace_members::WorkspaceRole;

const RUN_CAP: i32 = 10_000;
const RUN_LENGTH_MINUTES: i64 = 10;
const RUN_LENGTH_SECONDS: f64 = 600.0;
const PERIOD_ERROR: &str = "period must be one of day, week, month, quarter";
const WEEK: &str = "week";

const FAST_MODEL: &str = "fast-model";
const DEEP_MODEL: &str = "deep-model";

const COMPLETED: &str = "completed";
const FAILED: &str = "failed";
const CANCELLED: &str = "cancelled";
const RUNNING: &str = "running";

const CREATED: &str = "created";
const QUEUED: &str = "queued";
const IN_PROGRESS: &str = "in_progress";
const BLOCKED: &str = "blocked";
const PAUSED: &str = "paused";
const REVIEW: &str = "review";
const COMPLETE: &str = "complete";
const TASK_STATUSES: [&str; 8] = [
    CREATED,
    QUEUED,
    IN_PROGRESS,
    BLOCKED,
    PAUSED,
    REVIEW,
    COMPLETE,
    CANCELLED,
];

const MEMBER: &str = "member";
const REMINDER: &str = "reminder";
const AUTOMATION: &str = "automation";
const TASK_ORIGINS: [&str; 3] = [MEMBER, REMINDER, AUTOMATION];

const POOR: &str = "poor";
const SOLID: &str = "solid";
const QUALITY_BANDS: [&str; 4] = [POOR, "mixed", SOLID, "exemplary"];

const MERGED: &str = "merged";
const CLOSED: &str = "closed";
const OPEN: &str = "open";
const MERGED_PULL_REQUEST: &str = "https://github.com/acme/widgets/pull/1";
const CLOSED_PULL_REQUEST: &str = "https://github.com/acme/widgets/pull/2";
const OPEN_PULL_REQUEST: &str = "https://github.com/acme/widgets/pull/3";
const STALE_PULL_REQUEST: &str = "https://github.com/acme/widgets/pull/4";

const EVALUATING: &str = "evaluating";
const EVALUATION: &str = "evaluation";
const IMPROVED: &str = "improved";
const REGRESSED: &str = "regressed";

const TIMEOUT: &str = "timeout";
const TEXT_SOURCE: &str = "text";

fn analytics(workspace: Uuid) -> String {
    format!("/api/workspaces/{workspace}/analytics")
}

async fn registered(client: &TestClient) -> (String, Uuid) {
    let registered = client
        .post_json(
            "/api/auth/register",
            &json!({"email": test_email(), "password": test_password()}),
        )
        .await
        .json_value();
    let token = registered["access_token"]
        .as_str()
        .expect("registration returns an access token")
        .to_string();
    let user = registered["user"]["id"]
        .as_str()
        .expect("registration returns the user")
        .parse()
        .expect("a user id");
    (token, user)
}

/// A new user who owns a new organization's workspace: their token and the
/// workspace.
async fn owned_workspace(client: &TestClient) -> (String, Uuid) {
    let (token, _) = registered(client).await;
    let organization = client
        .post_json_auth(
            "/api/organizations",
            &json!({"name": "Analytics", "slug": format!("analytics-{}", Uuid::new_v4())}),
            &token,
        )
        .await
        .json_value()["organization"]["id"]
        .as_str()
        .expect("an organization")
        .to_string();
    let workspace = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({"name": "Analytics", "slug": format!("analytics-{}", Uuid::new_v4())}),
            &token,
        )
        .await
        .json_value()["workspace"]["id"]
        .as_str()
        .expect("a workspace")
        .parse()
        .expect("a workspace id");
    (token, workspace)
}

struct Board {
    client: TestClient,
    token: String,
    workspace: Uuid,
}

impl Board {
    async fn new() -> Self {
        let client = TestClient::with_db().await;
        let (token, workspace) = owned_workspace(&client).await;
        Self {
            client,
            token,
            workspace,
        }
    }

    fn pool(&self) -> &PgPool {
        self.client.state().db()
    }

    fn seeder(&self) -> Seeder<'_> {
        Seeder {
            pool: self.pool(),
            workspace: self.workspace,
            now: Utc::now().naive_utc(),
        }
    }

    async fn read_as(&self, query: &str, token: &str) -> (StatusCode, Value) {
        let response = self
            .client
            .get_auth(&format!("{}{query}", analytics(self.workspace)), token)
            .await;
        (response.status, response.json_value())
    }

    async fn overview(&self, query: &str) -> Value {
        let (status, body) = self.read_as(query, &self.token).await;
        assert_eq!(status, StatusCode::OK, "{query}: {body}");
        body
    }

    async fn viewer(&self) -> String {
        let (token, user) = registered(&self.client).await;
        workspace_members::add_member(
            self.pool(),
            self.workspace,
            user,
            WorkspaceRole::Viewer,
            None,
        )
        .await
        .expect("a viewer joins the workspace");
        token
    }
}

struct Source {
    id: Uuid,
    name: String,
}

struct Run<'a> {
    status: &'a str,
    model: Option<&'a str>,
    finished: Option<NaiveDateTime>,
    artifacts: Value,
}

struct Seeder<'a> {
    pool: &'a PgPool,
    workspace: Uuid,
    now: NaiveDateTime,
}

impl Seeder<'_> {
    fn ago(&self, elapsed: Duration) -> NaiveDateTime {
        self.now - elapsed
    }

    async fn source(&self, label: &str) -> Source {
        let name = format!("{label} {}", Uuid::new_v4());
        let id = sqlx::query_scalar(
            "INSERT INTO sources (workspace_id, name, source_type) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(self.workspace)
        .bind(&name)
        .bind(TEXT_SOURCE)
        .fetch_one(self.pool)
        .await
        .expect("source is created");
        Source { id, name }
    }

    async fn task(&self, source: Option<Uuid>, status: &str, origin: &str) -> Uuid {
        sqlx::query_scalar(
            "INSERT INTO tasks (workspace_id, title, description, source_id, status, origin)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id",
        )
        .bind(self.workspace)
        .bind(format!("{status} task"))
        .bind("seeded for the analytics overview")
        .bind(source)
        .bind(status)
        .bind(origin)
        .fetch_one(self.pool)
        .await
        .expect("task is created")
    }

    async fn run(&self, task: Uuid, run: Run<'_>) -> Uuid {
        let started = run.finished.unwrap_or(self.now) - Duration::minutes(RUN_LENGTH_MINUTES);
        sqlx::query_scalar(
            "INSERT INTO task_runs (task_id, status, model, started_at, completed_at, artifacts)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id",
        )
        .bind(task)
        .bind(run.status)
        .bind(run.model)
        .bind(started)
        .bind(run.finished)
        .bind(run.artifacts)
        .fetch_one(self.pool)
        .await
        .expect("run is created")
    }

    async fn pull_request(&self, task: Uuid, url: &str, status: &str, opened: NaiveDateTime) {
        sqlx::query(
            "UPDATE tasks SET pr_url = $2, pr_status = $3, pr_created_at = $4 WHERE id = $1",
        )
        .bind(task)
        .bind(url)
        .bind(status)
        .bind(opened)
        .execute(self.pool)
        .await
        .expect("pull request is recorded");
    }

    async fn shipped(&self, task: Uuid, shipped: NaiveDateTime, reopened: Option<NaiveDateTime>) {
        sqlx::query("UPDATE tasks SET shipped_at = $2, reopened_at = $3 WHERE id = $1")
            .bind(task)
            .bind(shipped)
            .bind(reopened)
            .execute(self.pool)
            .await
            .expect("shipping is recorded");
    }

    async fn evaluation(&self, run: Uuid, metadata: Value, at: NaiveDateTime) {
        sqlx::query(
            "INSERT INTO task_run_logs (task_run_id, phase, agent_type, log_level, message, metadata, created_at)
             VALUES ($1, $2, $3, 'info', 'evaluated', $4, $5)",
        )
        .bind(run)
        .bind(EVALUATING)
        .bind(EVALUATION)
        .bind(metadata)
        .bind(at)
        .execute(self.pool)
        .await
        .expect("evaluation is logged");
    }

    async fn runs_in_bulk(&self, task: Uuid, count: i32) {
        sqlx::query(
            "INSERT INTO task_runs (task_id, status, model, started_at, completed_at)
             SELECT $1, $2, $3,
                    $4 - make_interval(secs => n + 60),
                    $4 - make_interval(secs => n)
             FROM generate_series(1, $5) AS n",
        )
        .bind(task)
        .bind(COMPLETED)
        .bind(FAST_MODEL)
        .bind(self.now)
        .bind(count)
        .execute(self.pool)
        .await
        .expect("runs are created in bulk");
    }
}

/// What the overview should report about the seeded week, kept so each test
/// can find its own bucket.
struct Seeded {
    alpha: Source,
    beta: Source,
}

/// Two sources and tasks with none; runs on two models and none; successes,
/// a timeout twice, a cancellation, a run still going and a run from ten days
/// back; three pull requests opened this week and one opened long ago; and
/// evaluation verdicts that only the newest, windowed one of should count.
async fn seed(board: &Board) -> Seeded {
    let seeder = board.seeder();
    let alpha = seeder.source("Alpha").await;
    let beta = seeder.source("Beta").await;

    let merged = seeder.task(Some(alpha.id), COMPLETE, MEMBER).await;
    let closed = seeder.task(Some(alpha.id), BLOCKED, MEMBER).await;
    let created = seeder.task(Some(alpha.id), CREATED, MEMBER).await;
    let open = seeder.task(Some(beta.id), REVIEW, MEMBER).await;
    let paused = seeder.task(Some(beta.id), PAUSED, AUTOMATION).await;
    let reopened = seeder.task(None, IN_PROGRESS, REMINDER).await;
    let cancelled = seeder.task(None, CANCELLED, MEMBER).await;
    let queued = seeder.task(None, QUEUED, MEMBER).await;
    seeder.task(None, PAUSED, AUTOMATION).await;

    seeder
        .run(
            merged,
            Run {
                status: COMPLETED,
                model: Some(FAST_MODEL),
                finished: Some(seeder.ago(Duration::days(2))),
                artifacts: json!({
                    "pr": {
                        "pr_url": MERGED_PULL_REQUEST,
                        "minutes_to_merge": 999,
                        "review_cycles": 9,
                        "approvals": 9
                    },
                    "quality": {"band": POOR}
                }),
            },
        )
        .await;
    let newest_merged = seeder
        .run(
            merged,
            Run {
                status: COMPLETED,
                model: Some(FAST_MODEL),
                finished: Some(seeder.ago(Duration::hours(3))),
                artifacts: json!({
                    "pr": {
                        "pr_url": MERGED_PULL_REQUEST,
                        "minutes_to_merge": 30,
                        "review_cycles": 2,
                        "approvals": 1
                    },
                    "quality": {"band": SOLID}
                }),
            },
        )
        .await;
    seeder
        .run(
            closed,
            Run {
                status: FAILED,
                model: Some(DEEP_MODEL),
                finished: Some(seeder.ago(Duration::hours(5))),
                artifacts: json!({"failure": {"category": TIMEOUT}}),
            },
        )
        .await;
    let legacy = seeder
        .run(
            open,
            Run {
                status: COMPLETED,
                model: Some(DEEP_MODEL),
                finished: Some(seeder.ago(Duration::hours(6))),
                artifacts: json!({"pr": OPEN_PULL_REQUEST}),
            },
        )
        .await;
    seeder
        .run(
            paused,
            Run {
                status: CANCELLED,
                model: None,
                finished: Some(seeder.ago(Duration::hours(7))),
                artifacts: json!({}),
            },
        )
        .await;
    seeder
        .run(
            reopened,
            Run {
                status: FAILED,
                model: Some(FAST_MODEL),
                finished: Some(seeder.ago(Duration::hours(8))),
                artifacts: json!({"failure": {"category": TIMEOUT}}),
            },
        )
        .await;
    seeder
        .run(
            cancelled,
            Run {
                status: COMPLETED,
                model: None,
                finished: Some(seeder.ago(Duration::hours(9))),
                artifacts: json!({}),
            },
        )
        .await;
    seeder
        .run(
            queued,
            Run {
                status: RUNNING,
                model: Some(FAST_MODEL),
                finished: None,
                artifacts: json!({}),
            },
        )
        .await;
    let old = seeder
        .run(
            created,
            Run {
                status: COMPLETED,
                model: Some(FAST_MODEL),
                finished: Some(seeder.ago(Duration::days(10))),
                artifacts: json!({}),
            },
        )
        .await;

    seeder
        .pull_request(
            merged,
            MERGED_PULL_REQUEST,
            MERGED,
            seeder.ago(Duration::days(2)),
        )
        .await;
    seeder
        .pull_request(
            closed,
            CLOSED_PULL_REQUEST,
            CLOSED,
            seeder.ago(Duration::days(1)),
        )
        .await;
    seeder
        .pull_request(open, OPEN_PULL_REQUEST, OPEN, seeder.ago(Duration::days(1)))
        .await;
    seeder
        .pull_request(
            cancelled,
            STALE_PULL_REQUEST,
            MERGED,
            seeder.ago(Duration::days(20)),
        )
        .await;

    seeder
        .shipped(merged, seeder.ago(Duration::days(10)), None)
        .await;
    seeder
        .shipped(
            reopened,
            seeder.ago(Duration::days(2)),
            Some(seeder.ago(Duration::days(1))),
        )
        .await;

    seeder
        .evaluation(
            legacy,
            json!({"verdict": IMPROVED}),
            seeder.ago(Duration::hours(7)),
        )
        .await;
    seeder
        .evaluation(
            legacy,
            json!({"verdict": REGRESSED}),
            seeder.ago(Duration::hours(6)),
        )
        .await;
    seeder
        .evaluation(
            legacy,
            json!({"score": 0.4}),
            seeder.ago(Duration::hours(5)),
        )
        .await;
    seeder
        .evaluation(
            newest_merged,
            json!({"score": 0.9}),
            seeder.ago(Duration::hours(2)),
        )
        .await;
    seeder
        .evaluation(
            old,
            json!({"verdict": IMPROVED}),
            seeder.ago(Duration::hours(1)),
        )
        .await;

    Seeded { alpha, beta }
}

fn moment(value: &Value) -> NaiveDateTime {
    let text = value
        .as_str()
        .unwrap_or_else(|| panic!("a moment is a string: {value}"));
    assert!(
        !text.ends_with('Z') && !text.contains('+'),
        "a moment is naive UTC with no offset: {text}"
    );
    text.parse()
        .unwrap_or_else(|error| panic!("{text} is not a naive date time: {error}"))
}

fn span(window: &Value) -> Duration {
    moment(&window["end"]) - moment(&window["start"])
}

fn count(value: &Value) -> i64 {
    value
        .as_i64()
        .unwrap_or_else(|| panic!("a count is an integer: {value}"))
}

fn rate(value: &Value) -> f64 {
    value
        .as_f64()
        .unwrap_or_else(|| panic!("a rate is a number: {value}"))
}

fn named_counts(list: &Value) -> Vec<(String, i64)> {
    list.as_array()
        .unwrap_or_else(|| panic!("a list of named counts: {list}"))
        .iter()
        .map(|entry| {
            (
                entry["name"]
                    .as_str()
                    .unwrap_or_else(|| panic!("a named count: {entry}"))
                    .to_string(),
                count(&entry["count"]),
            )
        })
        .collect()
}

fn expected_counts(names: &[&str], counts: &[i64]) -> Vec<(String, i64)> {
    names
        .iter()
        .zip(counts)
        .map(|(name, count)| (name.to_string(), *count))
        .collect()
}

fn bucket<'a>(list: &'a Value, key: &str, wanted: Option<&str>) -> &'a Value {
    list.as_array()
        .unwrap_or_else(|| panic!("a list of buckets: {list}"))
        .iter()
        .find(|entry| entry[key].as_str() == wanted)
        .unwrap_or_else(|| panic!("no bucket with {key} {wanted:?} in {list}"))
}

fn totals(list: &Value) -> i64 {
    list.as_array()
        .unwrap_or_else(|| panic!("a list of buckets: {list}"))
        .iter()
        .map(|entry| count(&entry["outcomes"]["total"]))
        .sum()
}

#[tokio::test]
async fn the_overview_defaults_to_a_week_ending_now() {
    let board = Board::new().await;

    let before = Utc::now().naive_utc();
    let body = board.overview("").await;
    let after = Utc::now().naive_utc();

    assert_eq!(body["period"], json!(WEEK), "{body}");
    assert_eq!(span(&body["window"]), Duration::days(7), "{body}");
    let end = moment(&body["window"]["end"]);
    assert!(
        end > before - Duration::hours(1) && end <= after,
        "the window ends now, in UTC: {end} is not between {before} and {after}"
    );
    assert_eq!(body["runs"]["period"], body["period"], "{body}");
    assert_eq!(body["runs"]["window"], body["window"], "{body}");
    assert_eq!(body["truncated"], json!(false), "{body}");
    assert_eq!(count(&body["runs"]["outcomes"]["total"]), 0, "{body}");
}

#[tokio::test]
async fn each_period_spans_its_own_number_of_days() {
    let board = Board::new().await;

    for (period, days) in [("day", 1), (WEEK, 7), ("month", 30), ("quarter", 90)] {
        let body = board.overview(&format!("?period={period}")).await;
        assert_eq!(body["period"], json!(period), "{body}");
        assert_eq!(
            span(&body["window"]),
            Duration::days(days),
            "a {period} covers {days} days: {body}"
        );
        assert_eq!(body["runs"]["window"], body["window"], "{body}");
    }
}

#[tokio::test]
async fn an_unknown_period_is_refused_naming_the_periods_that_work() {
    let board = Board::new().await;

    for query in ["?period=bogus", "?period=year"] {
        let (status, body) = board.read_as(query, &board.token).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_eq!(
            body,
            json!({"success": false, "error": PERIOD_ERROR}),
            "{query}"
        );
    }
}

#[tokio::test]
async fn only_runs_that_finished_inside_the_window_are_counted() {
    let board = Board::new().await;
    seed(&board).await;

    let body = board.overview("").await;
    let outcomes = &body["runs"]["outcomes"];
    assert_eq!(
        count(&outcomes["total"]),
        7,
        "the run from ten days back and the one still going are left out: {body}"
    );
    assert_eq!(count(&outcomes["succeeded"]), 4, "{body}");
    assert_eq!(count(&outcomes["failed"]), 2, "{body}");
    assert_eq!(count(&outcomes["cancelled"]), 1, "{body}");
    assert!(
        (rate(&outcomes["success_rate"]) - 4.0 / 7.0).abs() < 1e-9,
        "{body}"
    );

    let timeout = bucket(&outcomes["categories"], "category", Some(TIMEOUT));
    assert_eq!(count(&timeout["occurrences"]), 2, "{body}");
    assert_eq!(count(&timeout["distinct_tasks"]), 2, "{body}");

    let completion = &body["runs"]["completion"];
    assert_eq!(count(&completion["measured"]), 4, "{body}");
    assert_eq!(rate(&completion["median_seconds"]), RUN_LENGTH_SECONDS);
    assert_eq!(body["truncated"], json!(false), "{body}");

    let month = board.overview("?period=month").await;
    assert_eq!(
        count(&month["runs"]["outcomes"]["total"]),
        8,
        "a month reaches the run from ten days back: {month}"
    );
}

#[tokio::test]
async fn sources_partition_the_runs_and_carry_their_merged_pull_requests() {
    let board = Board::new().await;
    let seeded = seed(&board).await;

    let body = board.overview("").await;
    let sources = &body["sources"];
    assert_eq!(
        totals(sources),
        count(&body["runs"]["outcomes"]["total"]),
        "every run in the window lands in exactly one source: {body}"
    );

    let alpha_id = seeded.alpha.id.to_string();
    let alpha = bucket(sources, "source_id", Some(alpha_id.as_str()));
    assert_eq!(alpha["name"], json!(seeded.alpha.name), "{body}");
    assert_eq!(alpha["source_type"], json!(TEXT_SOURCE), "{body}");
    assert_eq!(count(&alpha["outcomes"]["total"]), 3, "{body}");
    assert_eq!(count(&alpha["outcomes"]["succeeded"]), 2, "{body}");
    assert_eq!(count(&alpha["outcomes"]["failed"]), 1, "{body}");
    assert_eq!(count(&alpha["merged_pull_requests"]), 1, "{body}");

    let beta_id = seeded.beta.id.to_string();
    let beta = bucket(sources, "source_id", Some(beta_id.as_str()));
    assert_eq!(beta["name"], json!(seeded.beta.name), "{body}");
    assert_eq!(count(&beta["outcomes"]["total"]), 2, "{body}");
    assert_eq!(count(&beta["outcomes"]["cancelled"]), 1, "{body}");
    assert_eq!(count(&beta["merged_pull_requests"]), 0, "{body}");

    let unsourced = bucket(sources, "source_id", None);
    assert_eq!(unsourced["source_id"], Value::Null, "{body}");
    assert_eq!(unsourced["name"], Value::Null, "{body}");
    assert_eq!(unsourced["source_type"], Value::Null, "{body}");
    assert_eq!(count(&unsourced["outcomes"]["total"]), 2, "{body}");
    assert_eq!(
        count(&unsourced["merged_pull_requests"]),
        0,
        "a pull request merged before the window is not this week's: {body}"
    );
}

#[tokio::test]
async fn models_partition_the_runs_with_a_bucket_for_runs_without_one() {
    let board = Board::new().await;
    seed(&board).await;

    let body = board.overview("").await;
    let models = &body["models"];
    assert_eq!(
        totals(models),
        count(&body["runs"]["outcomes"]["total"]),
        "every run in the window lands in exactly one model: {body}"
    );

    let fast = bucket(models, "model", Some(FAST_MODEL));
    assert_eq!(count(&fast["outcomes"]["total"]), 3, "{body}");
    assert_eq!(count(&fast["outcomes"]["succeeded"]), 2, "{body}");
    assert_eq!(count(&fast["completion"]["measured"]), 2, "{body}");

    let deep = bucket(models, "model", Some(DEEP_MODEL));
    assert_eq!(count(&deep["outcomes"]["total"]), 2, "{body}");
    assert_eq!(count(&deep["outcomes"]["failed"]), 1, "{body}");

    let unnamed = bucket(models, "model", None);
    assert_eq!(unnamed["model"], Value::Null, "{body}");
    assert_eq!(count(&unnamed["outcomes"]["total"]), 2, "{body}");
}

#[tokio::test]
async fn pull_requests_opened_in_the_window_are_timed_from_their_newest_run() {
    let board = Board::new().await;
    seed(&board).await;

    let body = board.overview("").await;
    let pull_requests = &body["pull_requests"];
    assert_eq!(
        count(&pull_requests["opened"]),
        3,
        "the pull request opened twenty days ago is not this week's: {body}"
    );
    assert_eq!(count(&pull_requests["merged"]), 1, "{body}");
    assert_eq!(count(&pull_requests["closed"]), 1, "{body}");
    assert_eq!(
        count(&pull_requests["open"]),
        1,
        "a pull request recorded as a bare legacy URL still counts as open: {body}"
    );
    assert_eq!(rate(&pull_requests["merge_rate"]), 0.5, "{body}");
    assert_eq!(
        rate(&pull_requests["median_minutes_to_merge"]),
        30.0,
        "timing comes from the newest run that opened the pull request, not an older one: {body}"
    );
    assert_eq!(rate(&pull_requests["mean_review_cycles"]), 2.0, "{body}");
    assert_eq!(rate(&pull_requests["mean_approvals"]), 1.0, "{body}");
    assert_eq!(
        named_counts(&pull_requests["quality"]),
        expected_counts(&QUALITY_BANDS, &[0, 0, 1, 0]),
        "only the newest run's quality band counts: {body}"
    );
}

#[tokio::test]
async fn the_newest_verdict_of_each_run_finished_in_the_window_is_counted() {
    let board = Board::new().await;
    seed(&board).await;

    let body = board.overview("").await;
    assert_eq!(
        body["evaluations"],
        json!({"improved": 0, "unchanged": 0, "regressed": 1, "not_measured": 0}),
        "an earlier verdict is superseded, a log without a verdict is ignored, \
         and a run that finished before the window does not count however recently it was judged"
    );
}

#[tokio::test]
async fn tasks_are_counted_by_every_status_and_origin() {
    let board = Board::new().await;
    seed(&board).await;

    let body = board.overview("").await;
    let tasks = &body["tasks"];
    assert_eq!(
        named_counts(&tasks["statuses"]),
        expected_counts(&TASK_STATUSES, &[1, 1, 1, 1, 2, 1, 1, 1]),
        "{body}"
    );
    assert_eq!(
        named_counts(&tasks["origins"]),
        expected_counts(&TASK_ORIGINS, &[6, 1, 2]),
        "{body}"
    );
    assert_eq!(count(&tasks["created"]), 9, "{body}");
    assert_eq!(
        count(&tasks["shipped"]),
        1,
        "a task shipped ten days ago is not this week's: {body}"
    );
    assert_eq!(count(&tasks["reopened"]), 1, "{body}");
}

#[tokio::test]
async fn an_empty_workspace_still_lists_every_status_origin_and_quality_band() {
    let board = Board::new().await;

    let body = board.overview("").await;
    assert_eq!(
        named_counts(&body["tasks"]["statuses"]),
        expected_counts(&TASK_STATUSES, &[0; 8]),
        "{body}"
    );
    assert_eq!(
        named_counts(&body["tasks"]["origins"]),
        expected_counts(&TASK_ORIGINS, &[0; 3]),
        "{body}"
    );
    assert_eq!(
        named_counts(&body["pull_requests"]["quality"]),
        expected_counts(&QUALITY_BANDS, &[0; 4]),
        "{body}"
    );
    assert_eq!(
        body["evaluations"],
        json!({"improved": 0, "unchanged": 0, "regressed": 0, "not_measured": 0})
    );
}

#[tokio::test]
async fn a_workspace_past_the_run_cap_is_marked_truncated() {
    let board = Board::new().await;
    let seeder = board.seeder();
    let task = seeder.task(None, IN_PROGRESS, MEMBER).await;
    seeder.runs_in_bulk(task, RUN_CAP + 1).await;

    let body = board.overview("").await;
    assert_eq!(
        body["truncated"],
        json!(true),
        "the numbers describe only part of the window and must say so"
    );
    assert_eq!(
        count(&body["runs"]["outcomes"]["total"]),
        i64::from(RUN_CAP),
        "{}",
        body["runs"]["outcomes"]
    );
}

#[tokio::test]
async fn a_viewer_reads_the_overview() {
    let board = Board::new().await;
    seed(&board).await;
    let viewer = board.viewer().await;

    let (status, body) = board.read_as("", &viewer).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(count(&body["runs"]["outcomes"]["total"]), 7, "{body}");
}

#[tokio::test]
async fn a_member_of_another_organization_is_refused() {
    let board = Board::new().await;
    seed(&board).await;
    let (outsider, _) = owned_workspace(&board.client).await;

    let (status, body) = board.read_as("", &outsider).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(
        body.get("runs").is_none(),
        "a refusal carries none of the numbers: {body}"
    );
}

#[tokio::test]
async fn an_unauthenticated_request_is_refused() {
    let board = Board::new().await;

    let response = board.client.get(&analytics(board.workspace)).await;
    response.assert_status(StatusCode::UNAUTHORIZED);
}
