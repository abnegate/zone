//! The spend data layer: budgets, derived keys, ingesting LiteLLM's spend logs, and keeping
//! LiteLLM's keys in step.

use std::time::{Duration, Instant};

use abnegate_notify::Fanout;
use abnegate_secret::MasterKey;
use axum::http::StatusCode;
use chrono::{DateTime, NaiveDate, SubsecRound, TimeDelta, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, ResponseTemplate};
use zone_enterprise::spend::{Budget, Microdollars, Month, Scope, State, Target};
use zone_server::db::audit::lock::Lock;
use zone_server::db::migrations;
use zone_server::db::organization_members::OrgRole;
use zone_server::db::spend::budgets::{self, Announcement, BudgetRow, CreateError, Crossing};
use zone_server::db::spend::{cursor, keys};
use zone_server::db::workspace_members::WorkspaceRole;
use zone_server::services::spend::attribution::attribute;
use zone_server::services::spend::client::{DATE_FORMAT, PAGE_SIZE};
use zone_server::services::spend::ingest::{self, Ingested, OVERLAP, SPEND_LOCK};
use zone_server::services::spend::{Error, Key, Standing, notice, registry, upkeep};

use crate::common::Database;
use crate::common::spend::{DELETE_PATH, Fixture, GENERATE_PATH, LOGS_PATH, MASTER, Spend, log};

const PREFIX: &str = "spend_store";
const TOKEN: &str = "spend-store-token";

struct Store {
    database: Database,
}

impl Store {
    async fn new() -> Self {
        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool).await.unwrap();
        Self { database }
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    async fn organization(&self) -> Uuid {
        let organization = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Spend', $1::text)")
            .bind(organization)
            .execute(self.pool())
            .await
            .unwrap();
        organization
    }

    async fn workspace(&self, organization: Uuid, name: &str) -> Uuid {
        let workspace = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO workspaces (id, organization_id, name, slug) VALUES ($1, $2, $3, $1::text)",
        )
        .bind(workspace)
        .bind(organization)
        .bind(name)
        .execute(self.pool())
        .await
        .unwrap();
        workspace
    }

    async fn user(&self, name: &str) -> Uuid {
        let user = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, email, password_hash, display_name) \
             VALUES ($1, $1::text || '@example.com', 'spend', $2)",
        )
        .bind(user)
        .bind(name)
        .execute(self.pool())
        .await
        .unwrap();
        user
    }

    async fn spend(
        &self,
        organization: Uuid,
        workspace: Option<Uuid>,
        user: Option<Uuid>,
        microdollars: i64,
        occurred_at: DateTime<Utc>,
    ) {
        sqlx::query(
            "INSERT INTO spend_records (request_id, organization_id, workspace_id, user_id, \
             token, model, provider, spend_microdollars, occurred_at) \
             VALUES ($1, $2, $3, $4, $5, 'gpt-4o', 'openai', $6, $7)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(organization)
        .bind(workspace)
        .bind(user)
        .bind(TOKEN)
        .bind(microdollars)
        .bind(occurred_at)
        .execute(self.pool())
        .await
        .unwrap();
    }

    async fn create(
        &self,
        organization: Uuid,
        target: Target,
        budget: Budget,
        actor: Uuid,
    ) -> Result<BudgetRow, CreateError> {
        let mut transaction = self.pool().begin().await.unwrap();
        let created = budgets::create(
            &mut transaction,
            organization,
            &target,
            &budget,
            actor,
            month(),
        )
        .await?;
        budgets::refresh(&mut transaction, created.id, month())
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        Ok(created)
    }

    async fn update(&self, id: Uuid, budget: Budget, actor: Uuid) -> BudgetRow {
        let mut transaction = self.pool().begin().await.unwrap();
        let organization: Uuid =
            sqlx::query_scalar("SELECT organization_id FROM budgets WHERE id = $1")
                .bind(id)
                .fetch_one(&mut *transaction)
                .await
                .unwrap();
        let updated = budgets::update(&mut transaction, organization, id, &budget, actor, month())
            .await
            .unwrap();
        budgets::refresh(&mut transaction, id, month())
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        updated
    }

    async fn refresh(&self, id: Uuid) {
        let mut connection = self.pool().acquire().await.unwrap();
        budgets::refresh(&mut connection, id, month())
            .await
            .unwrap();
    }

    async fn fetched(&self, organization: Uuid, id: Uuid) -> Option<BudgetRow> {
        budgets::list(self.pool(), organization, month())
            .await
            .unwrap()
            .into_iter()
            .find(|row| row.id == id)
    }

    async fn markers(&self, id: Uuid) -> Markers {
        sqlx::query_as(
            "SELECT exhausted_month, warning_sent_month, exhaustion_sent_month \
             FROM budgets WHERE id = $1",
        )
        .bind(id)
        .fetch_one(self.pool())
        .await
        .unwrap()
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

type Markers = (Option<NaiveDate>, Option<NaiveDate>, Option<NaiveDate>);

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text).unwrap().to_utc()
}

fn month() -> Month {
    Month::of(at("2026-11-15T12:00:00Z"))
}

fn inside() -> DateTime<Utc> {
    at("2026-11-10T09:00:00Z")
}

fn budget(limit: i64, hard: bool) -> Budget {
    Budget {
        limit: Microdollars(limit),
        threshold_percent: 80,
        hard,
    }
}

fn organization_wide() -> Target {
    Target::of(None, None)
}

fn crossing(crossings: &[Crossing], id: Uuid) -> Option<Announcement> {
    crossings
        .iter()
        .find(|crossing| crossing.row.id == id)
        .map(|crossing| crossing.announcement)
}

#[tokio::test]
async fn a_second_budget_for_the_same_target_already_exists() {
    let fixture = Store::new().await;
    let organization = fixture.organization().await;
    let other = fixture.organization().await;
    let workspace = fixture.workspace(organization, "Platform").await;
    let sibling = fixture.workspace(organization, "Research").await;
    let member = fixture.user("Ada").await;

    for target in [
        organization_wide(),
        Target::of(Some(workspace), None),
        Target::of(None, Some(member)),
    ] {
        fixture
            .create(organization, target, budget(10, false), member)
            .await
            .unwrap();
        let again = fixture
            .create(organization, target, budget(20, true), member)
            .await;
        assert!(
            matches!(again, Err(CreateError::Exists)),
            "{target:?}: {again:?}"
        );
    }
    fixture
        .create(
            organization,
            Target::of(Some(sibling), None),
            budget(10, false),
            member,
        )
        .await
        .unwrap();
    fixture
        .create(other, organization_wide(), budget(10, false), member)
        .await
        .unwrap();

    let stored = budgets::list(fixture.pool(), organization, month())
        .await
        .unwrap();
    assert_eq!(stored.len(), 4);

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_created_budget_answers_its_target_names_and_author() {
    let fixture = Store::new().await;
    let organization = fixture.organization().await;
    let workspace = fixture.workspace(organization, "Platform").await;
    let member = fixture.user("Ada").await;
    let author = fixture.user("Grace").await;

    let created = fixture
        .create(
            organization,
            Target::of(Some(workspace), None),
            budget(50, true),
            author,
        )
        .await
        .unwrap();
    assert_eq!(created.organization_id, organization);
    assert_eq!(created.target.scope, Scope::Workspace);
    assert_eq!(created.workspace_name.as_deref(), Some("Platform"));
    assert_eq!(created.user_name, None);
    assert_eq!(created.budget, budget(50, true));
    assert_eq!(created.spent, Microdollars::ZERO);

    let user = fixture
        .create(
            organization,
            Target::of(None, Some(member)),
            budget(50, false),
            author,
        )
        .await
        .unwrap();
    assert_eq!(user.target.scope, Scope::User);
    assert_eq!(user.user_name.as_deref(), Some("Ada"));
    assert_eq!(user.user_email, Some(format!("{member}@example.com")));

    let authors: (Option<Uuid>, Option<Uuid>) =
        sqlx::query_as("SELECT created_by, updated_by FROM budgets WHERE id = $1")
            .bind(created.id)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert_eq!(authors, (Some(author), Some(author)));

    assert_eq!(
        fixture.fetched(organization, created.id).await,
        Some(created.clone())
    );
    let stranger = fixture.organization().await;
    assert_eq!(
        fixture.fetched(stranger, created.id).await,
        None,
        "another organization's budget is not found"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn the_database_refuses_budgets_the_domain_refuses() {
    let fixture = Store::new().await;
    let organization = fixture.organization().await;
    let workspace = fixture.workspace(organization, "Platform").await;
    let member = fixture.user("Ada").await;

    for (workspace, user, limit, threshold, constraint) in [
        (None, None, 0, 80, "budgets_limit_check"),
        (None, None, 1, 0, "budgets_threshold_check"),
        (None, None, 1, 101, "budgets_threshold_check"),
        (Some(workspace), Some(member), 1, 80, "budgets_scope_check"),
    ] {
        let error = sqlx::query(
            "INSERT INTO budgets (organization_id, workspace_id, user_id, limit_microdollars, \
             threshold_percent) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(organization)
        .bind(workspace)
        .bind(user)
        .bind(limit)
        .bind(threshold)
        .execute(fixture.pool())
        .await
        .unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|database| database.constraint()),
            Some(constraint),
            "{error}"
        );
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn list_sums_only_the_month_and_the_target() {
    let fixture = Store::new().await;
    let organization = fixture.organization().await;
    let other = fixture.organization().await;
    let platform = fixture.workspace(organization, "Platform").await;
    let research = fixture.workspace(organization, "Research").await;
    let elsewhere = fixture.workspace(other, "Elsewhere").await;
    let ada = fixture.user("Ada").await;
    let grace = fixture.user("Grace").await;

    fixture
        .spend(organization, Some(platform), Some(ada), 1, inside())
        .await;
    fixture
        .spend(organization, Some(platform), None, 10, inside())
        .await;
    fixture
        .spend(organization, Some(research), Some(ada), 100, inside())
        .await;
    fixture
        .spend(organization, Some(research), Some(grace), 1_000, inside())
        .await;
    fixture
        .spend(organization, None, Some(ada), 10_000, inside())
        .await;
    fixture
        .spend(
            organization,
            Some(platform),
            Some(ada),
            100_000,
            month().start() - TimeDelta::microseconds(1),
        )
        .await;
    fixture
        .spend(
            organization,
            Some(platform),
            Some(ada),
            1_000_000,
            month().end(),
        )
        .await;
    fixture
        .spend(other, Some(elsewhere), Some(ada), 10_000_000, inside())
        .await;
    fixture
        .spend(
            organization,
            Some(platform),
            Some(ada),
            100_000_000,
            month().start(),
        )
        .await;

    let whole = fixture
        .create(organization, organization_wide(), budget(1, false), ada)
        .await
        .unwrap();
    let workspace = fixture
        .create(
            organization,
            Target::of(Some(platform), None),
            budget(1, false),
            ada,
        )
        .await
        .unwrap();
    let user = fixture
        .create(
            organization,
            Target::of(None, Some(ada)),
            budget(1, false),
            ada,
        )
        .await
        .unwrap();
    let quiet = fixture
        .create(
            organization,
            Target::of(Some(research), None),
            budget(1, false),
            ada,
        )
        .await
        .unwrap();
    fixture
        .create(other, organization_wide(), budget(1, false), ada)
        .await
        .unwrap();

    let listed = budgets::list(fixture.pool(), organization, month())
        .await
        .unwrap();
    let spent: Vec<(Uuid, i64)> = listed.iter().map(|row| (row.id, row.spent.0)).collect();
    assert_eq!(
        spent,
        [
            (whole.id, 100_011_111),
            (workspace.id, 100_000_011),
            (quiet.id, 1_100),
            (user.id, 100_010_101),
        ],
        "organization first, then workspaces by name, then members"
    );
    assert!(listed.iter().all(|row| row.organization_id == organization));

    let empty = budgets::list(
        fixture.pool(),
        organization,
        Month::of(at("2026-10-15T00:00:00Z")),
    )
    .await
    .unwrap();
    let october: Vec<i64> = empty.iter().map(|row| row.spent.0).collect();
    assert_eq!(october, [100_000, 100_000, 0, 100_000]);

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_deleted_workspace_keeps_counting_for_its_organization() {
    let fixture = Store::new().await;
    let organization = fixture.organization().await;
    let workspace = fixture.workspace(organization, "Platform").await;
    let ada = fixture.user("Ada").await;
    fixture
        .spend(organization, Some(workspace), Some(ada), 25, inside())
        .await;
    let whole = fixture
        .create(organization, organization_wide(), budget(100, false), ada)
        .await
        .unwrap();
    let scoped = fixture
        .create(
            organization,
            Target::of(Some(workspace), None),
            budget(100, false),
            ada,
        )
        .await
        .unwrap();

    sqlx::query("DELETE FROM workspaces WHERE id = $1")
        .bind(workspace)
        .execute(fixture.pool())
        .await
        .unwrap();

    let listed = budgets::list(fixture.pool(), organization, month())
        .await
        .unwrap();
    assert_eq!(listed.len(), 1, "the workspace's budget goes with it");
    assert_eq!(listed[0].id, whole.id);
    assert_eq!(listed[0].spent, Microdollars(25));
    assert_eq!(fixture.fetched(organization, scoped.id).await, None);

    fixture.cleanup().await;
}

#[tokio::test]
async fn update_writes_the_budget_and_keeps_the_stamp_when_nothing_changes() {
    let fixture = Store::new().await;
    let organization = fixture.organization().await;
    let author = fixture.user("Ada").await;
    let editor = fixture.user("Grace").await;
    let created = fixture
        .create(
            organization,
            organization_wide(),
            budget(100, false),
            author,
        )
        .await
        .unwrap();

    let unchanged = fixture.update(created.id, budget(100, false), editor).await;
    assert_eq!(unchanged, created, "a no-op write keeps updated_at");
    let updater: Option<Uuid> = sqlx::query_scalar("SELECT updated_by FROM budgets WHERE id = $1")
        .bind(created.id)
        .fetch_one(fixture.pool())
        .await
        .unwrap();
    assert_eq!(updater, Some(author));

    let changed = Budget {
        limit: Microdollars(250),
        threshold_percent: 50,
        hard: true,
    };
    let updated = fixture.update(created.id, changed, editor).await;
    assert_eq!(updated.budget, changed);
    assert!(updated.updated_at > created.updated_at);
    let updater: Option<Uuid> = sqlx::query_scalar("SELECT updated_by FROM budgets WHERE id = $1")
        .bind(created.id)
        .fetch_one(fixture.pool())
        .await
        .unwrap();
    assert_eq!(updater, Some(editor));

    fixture.cleanup().await;
}

#[tokio::test]
async fn lock_answers_the_organizations_budget_and_delete_removes_it() {
    let fixture = Store::new().await;
    let organization = fixture.organization().await;
    let stranger = fixture.organization().await;
    let author = fixture.user("Ada").await;
    let created = fixture
        .create(
            organization,
            organization_wide(),
            budget(100, false),
            author,
        )
        .await
        .unwrap();
    fixture.spend(organization, None, None, 7, inside()).await;
    fixture
        .spend(organization, None, None, 70, month().end())
        .await;

    let mut transaction = fixture.pool().begin().await.unwrap();
    let locked = budgets::lock(&mut transaction, organization, created.id, month())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(locked.id, created.id);
    assert_eq!(
        locked.spent,
        Microdollars(7),
        "lock reads the month's spend"
    );
    assert_eq!(
        budgets::lock(&mut transaction, stranger, created.id, month())
            .await
            .unwrap(),
        None
    );
    assert!(
        !budgets::delete(&mut transaction, stranger, created.id)
            .await
            .unwrap(),
        "another organization can't delete the budget"
    );
    assert!(
        budgets::delete(&mut transaction, organization, created.id)
            .await
            .unwrap()
    );
    assert!(
        !budgets::delete(&mut transaction, organization, created.id)
            .await
            .unwrap()
    );
    transaction.commit().await.unwrap();

    assert!(
        budgets::list(fixture.pool(), organization, month())
            .await
            .unwrap()
            .is_empty()
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn refresh_sets_and_clears_the_exhausted_month_and_rearms_notices() {
    let fixture = Store::new().await;
    let organization = fixture.organization().await;
    let author = fixture.user("Ada").await;
    fixture.spend(organization, None, None, 100, inside()).await;

    let created = fixture
        .create(organization, organization_wide(), budget(100, true), author)
        .await
        .unwrap();
    assert_eq!(created.state(), State::Exhausted);
    assert_eq!(
        fixture.markers(created.id).await,
        (Some(month().date()), None, None)
    );

    budgets::announce(fixture.pool(), created.id, Announcement::Warning, month())
        .await
        .unwrap();
    budgets::announce(
        fixture.pool(),
        created.id,
        Announcement::Exhaustion,
        month(),
    )
    .await
    .unwrap();
    fixture.refresh(created.id).await;
    assert_eq!(
        fixture.markers(created.id).await,
        (
            Some(month().date()),
            Some(month().date()),
            Some(month().date())
        ),
        "markers stay while their conditions hold"
    );

    let warning = fixture.update(created.id, budget(120, true), author).await;
    assert_eq!(warning.state(), State::Warning);
    assert_eq!(
        fixture.markers(created.id).await,
        (None, Some(month().date()), None),
        "raising the limit past spend unblocks and re-arms the exhaustion notice"
    );

    let under = fixture
        .update(created.id, budget(1_000, true), author)
        .await;
    assert_eq!(under.state(), State::Under);
    assert_eq!(fixture.markers(created.id).await, (None, None, None));

    fixture.update(created.id, budget(50, true), author).await;
    assert_eq!(
        fixture.markers(created.id).await,
        (Some(month().date()), None, None),
        "lowering the limit under spend exhausts it again"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn exhausted_answers_the_broadest_hard_budget_and_ignores_soft_ones() {
    let fixture = Store::new().await;
    let organization = fixture.organization().await;
    let workspace = fixture.workspace(organization, "Platform").await;
    let sibling = fixture.workspace(organization, "Research").await;
    let ada = fixture.user("Ada").await;
    let grace = fixture.user("Grace").await;
    fixture
        .spend(organization, Some(workspace), Some(ada), 100, inside())
        .await;

    let exhausted = |workspace: Uuid, user: Option<Uuid>, month: Month| {
        let pool = fixture.pool().clone();
        async move {
            budgets::exhausted(&pool, organization, workspace, user, month)
                .await
                .unwrap()
        }
    };

    let soft = fixture
        .create(organization, organization_wide(), budget(10, false), ada)
        .await
        .unwrap();
    assert_eq!(soft.state(), State::Exhausted);
    assert_eq!(
        exhausted(workspace, Some(ada), month()).await,
        None,
        "soft budgets never block"
    );

    let user = fixture
        .create(
            organization,
            Target::of(None, Some(ada)),
            budget(10, true),
            ada,
        )
        .await
        .unwrap();
    assert_eq!(
        exhausted(workspace, Some(ada), month()).await,
        Some(Scope::User)
    );
    assert_eq!(
        exhausted(sibling, Some(ada), month()).await,
        Some(Scope::User)
    );
    assert_eq!(exhausted(workspace, Some(grace), month()).await, None);
    assert_eq!(
        exhausted(workspace, None, month()).await,
        None,
        "user budgets spare automation"
    );

    let scoped = fixture
        .create(
            organization,
            Target::of(Some(workspace), None),
            budget(10, true),
            ada,
        )
        .await
        .unwrap();
    assert_eq!(
        exhausted(workspace, Some(ada), month()).await,
        Some(Scope::Workspace)
    );
    assert_eq!(
        exhausted(workspace, None, month()).await,
        Some(Scope::Workspace)
    );
    assert_eq!(exhausted(sibling, Some(grace), month()).await, None);

    fixture.update(soft.id, budget(10, true), ada).await;
    assert_eq!(
        exhausted(workspace, Some(ada), month()).await,
        Some(Scope::Organization)
    );
    assert_eq!(
        exhausted(sibling, None, month()).await,
        Some(Scope::Organization)
    );

    let next = Month::of(month().end());
    assert_eq!(
        exhausted(workspace, Some(ada), next).await,
        None,
        "a new month unblocks"
    );
    let stranger = fixture.organization().await;
    assert_eq!(
        budgets::exhausted(fixture.pool(), stranger, workspace, Some(ada), month())
            .await
            .unwrap(),
        None
    );

    fixture.update(user.id, budget(1_000, true), ada).await;
    fixture.update(scoped.id, budget(1_000, true), ada).await;
    fixture.update(soft.id, budget(1_000, true), ada).await;
    assert_eq!(exhausted(workspace, Some(ada), month()).await, None);

    fixture.cleanup().await;
}

#[tokio::test]
async fn settle_answers_each_crossing_until_it_is_announced() {
    let fixture = Store::new().await;
    let organization = fixture.organization().await;
    let platform = fixture.workspace(organization, "Platform").await;
    let research = fixture.workspace(organization, "Research").await;
    let ada = fixture.user("Ada").await;

    let warning = fixture
        .create(
            organization,
            Target::of(Some(platform), None),
            budget(100, false),
            ada,
        )
        .await
        .unwrap();
    let exhausted = fixture
        .create(
            organization,
            Target::of(Some(research), None),
            budget(100, true),
            ada,
        )
        .await
        .unwrap();
    let under = fixture
        .create(organization, organization_wide(), budget(1_000, true), ada)
        .await
        .unwrap();
    assert!(
        budgets::settle(fixture.pool(), month())
            .await
            .unwrap()
            .is_empty()
    );

    fixture
        .spend(organization, Some(platform), None, 80, inside())
        .await;
    fixture
        .spend(organization, Some(research), None, 150, inside())
        .await;

    for _ in 0..2 {
        let crossings = budgets::settle(fixture.pool(), month()).await.unwrap();
        assert_eq!(crossings.len(), 2, "{crossings:?}");
        assert_eq!(
            crossing(&crossings, warning.id),
            Some(Announcement::Warning)
        );
        assert_eq!(
            crossing(&crossings, exhausted.id),
            Some(Announcement::Exhaustion)
        );
        assert_eq!(crossing(&crossings, under.id), None);
        let row = &crossings
            .iter()
            .find(|crossing| crossing.row.id == exhausted.id)
            .unwrap()
            .row;
        assert_eq!(row.spent, Microdollars(150));
        assert_eq!(row.workspace_name.as_deref(), Some("Research"));
    }
    assert_eq!(
        fixture.markers(exhausted.id).await,
        (Some(month().date()), None, None),
        "settle exhausts the budget for the month"
    );
    assert_eq!(fixture.markers(warning.id).await, (None, None, None));

    budgets::announce(fixture.pool(), warning.id, Announcement::Warning, month())
        .await
        .unwrap();
    let crossings = budgets::settle(fixture.pool(), month()).await.unwrap();
    assert_eq!(crossings.len(), 1);
    assert_eq!(
        crossing(&crossings, exhausted.id),
        Some(Announcement::Exhaustion)
    );

    budgets::announce(
        fixture.pool(),
        exhausted.id,
        Announcement::Exhaustion,
        month(),
    )
    .await
    .unwrap();
    assert!(
        budgets::settle(fixture.pool(), month())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        budgets::settle(fixture.pool(), month())
            .await
            .unwrap()
            .is_empty()
    );

    fixture
        .spend(organization, Some(platform), None, 600, inside())
        .await;
    let crossings = budgets::settle(fixture.pool(), month()).await.unwrap();
    assert_eq!(
        crossing(&crossings, warning.id),
        Some(Announcement::Exhaustion)
    );
    assert_eq!(crossing(&crossings, under.id), Some(Announcement::Warning));
    assert_eq!(crossings.len(), 2);

    fixture.cleanup().await;
}

#[tokio::test]
async fn settle_rearms_a_notice_once_its_condition_lapses() {
    let fixture = Store::new().await;
    let organization = fixture.organization().await;
    let ada = fixture.user("Ada").await;
    fixture.spend(organization, None, None, 100, inside()).await;
    let created = fixture
        .create(organization, organization_wide(), budget(100, true), ada)
        .await
        .unwrap();
    budgets::announce(
        fixture.pool(),
        created.id,
        Announcement::Exhaustion,
        month(),
    )
    .await
    .unwrap();
    assert!(
        budgets::settle(fixture.pool(), month())
            .await
            .unwrap()
            .is_empty()
    );

    let next = Month::of(month().end());
    assert!(
        budgets::settle(fixture.pool(), next)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture.markers(created.id).await,
        (None, None, None),
        "a month with no spend clears every marker"
    );
    fixture
        .spend(organization, None, None, 100, next.start())
        .await;
    let crossings = budgets::settle(fixture.pool(), next).await.unwrap();
    assert_eq!(
        crossing(&crossings, created.id),
        Some(Announcement::Exhaustion)
    );
    assert_eq!(
        fixture.markers(created.id).await,
        (Some(next.date()), None, None)
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn every_target_counts_its_own_spend_and_unbudgeted_organizations_count_for_none() {
    let fixture = Store::new().await;
    let organization = fixture.organization().await;
    let unbudgeted = fixture.organization().await;
    let platform = fixture.workspace(organization, "Platform").await;
    let research = fixture.workspace(organization, "Research").await;
    let gone = fixture.workspace(organization, "Gone").await;
    let elsewhere = fixture.workspace(unbudgeted, "Elsewhere").await;
    let ada = fixture.user("Ada").await;
    let grace = fixture.user("Grace").await;
    let linus = fixture.user("Linus").await;
    for (organization, workspace, user, microdollars) in [
        (organization, Some(platform), Some(ada), 1),
        (organization, Some(platform), None, 10),
        (organization, Some(gone), Some(ada), 100),
        (organization, Some(gone), None, 1_000),
        (organization, Some(research), Some(linus), 10_000),
        (organization, Some(research), Some(grace), 100_000),
        (unbudgeted, Some(elsewhere), Some(ada), 1_000_000),
        (unbudgeted, None, None, 10_000_000),
    ] {
        fixture
            .spend(organization, workspace, user, microdollars, inside())
            .await;
    }
    let mut expected = Vec::new();
    for (target, spent) in [
        (organization_wide(), 111_111),
        (Target::of(Some(platform), None), 11),
        (Target::of(Some(research), None), 110_000),
        (Target::of(None, Some(ada)), 101),
        (Target::of(None, Some(grace)), 100_000),
    ] {
        let created = fixture
            .create(organization, target, budget(1, false), ada)
            .await
            .unwrap();
        expected.push((created.id, spent));
    }
    sqlx::query("DELETE FROM workspaces WHERE id = $1")
        .bind(gone)
        .execute(fixture.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(linus)
        .execute(fixture.pool())
        .await
        .unwrap();
    let (research_budget, research_spent) = expected[2];
    expected.sort();

    let mut listed: Vec<(Uuid, i64)> = budgets::list(fixture.pool(), organization, month())
        .await
        .unwrap()
        .iter()
        .map(|row| (row.id, row.spent.0))
        .collect();
    listed.sort();
    assert_eq!(
        listed, expected,
        "spend without a workspace or member counts only for the organization"
    );
    let mut settled: Vec<(Uuid, i64)> = budgets::settle(fixture.pool(), month())
        .await
        .unwrap()
        .iter()
        .map(|crossing| (crossing.row.id, crossing.row.spent.0))
        .collect();
    settled.sort();
    assert_eq!(settled, expected, "settle sums each target as list does");
    let mut transaction = fixture.pool().begin().await.unwrap();
    let locked = budgets::lock(&mut transaction, organization, research_budget, month())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(locked.spent.0, research_spent);
    transaction.rollback().await.unwrap();
    assert!(
        budgets::list(fixture.pool(), unbudgeted, month())
            .await
            .unwrap()
            .is_empty()
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_month_past_the_largest_sum_is_held_at_the_most_microdollars() {
    let fixture = Store::new().await;
    let organization = fixture.organization().await;
    let ada = fixture.user("Ada").await;
    for _ in 0..2 {
        fixture
            .spend(organization, None, None, i64::MAX, inside())
            .await;
    }

    let created = fixture
        .create(organization, organization_wide(), budget(1, true), ada)
        .await
        .unwrap();
    assert_eq!(created.spent, Microdollars(i64::MAX));
    let listed = budgets::list(fixture.pool(), organization, month())
        .await
        .unwrap();
    assert_eq!(listed[0].spent, Microdollars(i64::MAX));
    let crossings = budgets::settle(fixture.pool(), month()).await.unwrap();
    assert_eq!(
        crossing(&crossings, created.id),
        Some(Announcement::Exhaustion)
    );
    assert_eq!(crossings[0].row.spent, Microdollars(i64::MAX));

    fixture.cleanup().await;
}

#[tokio::test]
async fn settle_answers_nothing_for_a_budget_changed_while_it_waited() {
    let fixture = Store::new().await;
    let organization = fixture.organization().await;
    let ada = fixture.user("Ada").await;
    let created = fixture
        .create(organization, organization_wide(), budget(100, true), ada)
        .await
        .unwrap();
    fixture.spend(organization, None, None, 150, inside()).await;

    let mut edit = fixture.pool().begin().await.unwrap();
    let editor: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *edit)
        .await
        .unwrap();
    budgets::update(
        &mut edit,
        organization,
        created.id,
        &budget(1_000, true),
        ada,
        month(),
    )
    .await
    .unwrap();
    budgets::refresh(&mut edit, created.id, month())
        .await
        .unwrap();
    let pool = fixture.pool().clone();
    let settling = tokio::spawn(async move { budgets::settle(&pool, month()).await });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let blocked: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid)))",
        )
        .bind(editor)
        .fetch_one(fixture.pool())
        .await
        .unwrap();
        if blocked {
            break;
        }
        assert!(Instant::now() < deadline, "settle never waited on the edit");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    edit.commit().await.unwrap();

    let crossings = settling.await.unwrap().unwrap();
    assert!(
        crossings.is_empty(),
        "no notice for the limit the edit replaced: {crossings:?}"
    );
    assert_eq!(fixture.markers(created.id).await, (None, None, None));
    assert!(
        budgets::settle(fixture.pool(), month())
            .await
            .unwrap()
            .is_empty()
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn update_leaves_another_organizations_budget_alone() {
    let fixture = Store::new().await;
    let organization = fixture.organization().await;
    let stranger = fixture.organization().await;
    let ada = fixture.user("Ada").await;
    let created = fixture
        .create(organization, organization_wide(), budget(100, false), ada)
        .await
        .unwrap();

    let mut transaction = fixture.pool().begin().await.unwrap();
    let error = budgets::update(
        &mut transaction,
        stranger,
        created.id,
        &budget(500, true),
        ada,
        month(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, sqlx::Error::RowNotFound), "{error}");
    transaction.commit().await.unwrap();

    assert_eq!(
        fixture.fetched(organization, created.id).await,
        Some(created)
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_budget_notice_goes_to_active_accounts_only() {
    let fixture = Fixture::new("spend_notice_active").await;
    let admin = fixture.policy.member(OrgRole::Admin, None).await;
    let departed = fixture.policy.member(OrgRole::Admin, None).await;
    let steward = fixture
        .policy
        .member(OrgRole::Member, Some(WorkspaceRole::Admin))
        .await;
    let departed_steward = fixture
        .policy
        .member(OrgRole::Member, Some(WorkspaceRole::Owner))
        .await;
    fixture
        .policy
        .member(OrgRole::Member, Some(WorkspaceRole::Member))
        .await;
    sqlx::query("UPDATE users SET is_active = false WHERE id = ANY($1)")
        .bind(vec![departed.id, departed_steward.id])
        .execute(fixture.pool())
        .await
        .unwrap();
    fixture
        .record(Spend {
            occurred_at: inside(),
            ..fixture.spend()
        })
        .await;
    let mut connection = fixture.pool().acquire().await.unwrap();
    let created = budgets::create(
        &mut connection,
        fixture.policy.organization,
        &Target::of(Some(fixture.policy.workspace), None),
        &budget(1, false),
        fixture.policy.owner.id,
        month(),
    )
    .await
    .unwrap();
    drop(connection);

    let crossings = budgets::settle(fixture.pool(), month()).await.unwrap();
    let crossing = crossings
        .iter()
        .find(|crossing| crossing.row.id == created.id)
        .expect("the budget is used up");
    assert!(
        notice::announce(fixture.state(), &Fanout::new(), crossing, month())
            .await
            .unwrap()
    );

    let mut mailed = fixture.mailed();
    mailed.sort();
    let mut expected = vec![
        fixture.policy.owner.email.clone(),
        admin.email,
        steward.email,
    ];
    expected.sort();
    assert_eq!(mailed, expected, "deactivated accounts get no notice");

    fixture.cleanup().await;
}

fn overlap() -> TimeDelta {
    TimeDelta::from_std(OVERLAP).unwrap()
}

fn query(request: &wiremock::Request, name: &str) -> String {
    request
        .url
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default()
}

fn seconds(moment: DateTime<Utc>) -> String {
    moment.format(DATE_FORMAT).to_string()
}

/// Registers the owner's key through attribution, as a member's first turn would.
async fn attributed(fixture: &Fixture) -> Key {
    fixture.mount_register().await;
    let endpoint = attribute(
        fixture.state(),
        fixture.instance(),
        &fixture.subject(),
        &Standing::read(fixture.state()).await,
    )
    .await;
    let key = fixture.key(&fixture.subject());
    assert_eq!(endpoint.key().expose(), key.secret.expose());
    key
}

async fn ingested(fixture: &Fixture) -> Ingested {
    ingest::run(fixture.state(), &fixture.client())
        .await
        .unwrap()
}

#[tokio::test]
async fn keys_are_recorded_once_and_revoked_keys_stay() {
    let fixture = Fixture::new("spend_keys").await;
    let pool = fixture.pool();
    let subject = fixture.subject();
    assert_eq!(keys::earliest(pool).await.unwrap(), None);

    for _ in 0..2 {
        keys::insert(
            pool,
            "member",
            subject.organization,
            subject.workspace,
            subject.user,
        )
        .await
        .unwrap();
    }
    keys::insert(
        pool,
        "automation",
        subject.organization,
        subject.workspace,
        None,
    )
    .await
    .unwrap();
    let active = keys::active(pool).await.unwrap();
    assert_eq!(active.len(), 2);
    let earliest = keys::earliest(pool).await.unwrap().unwrap();
    assert_eq!(
        earliest,
        active.iter().map(|row| row.created_at).min().unwrap()
    );

    keys::revoked(pool, "member").await.unwrap();
    let active = keys::active(pool).await.unwrap();
    assert_eq!(
        active
            .iter()
            .map(|row| row.token.as_str())
            .collect::<Vec<_>>(),
        ["automation"]
    );
    assert_eq!(active[0].user_id, None);
    assert_eq!(keys::earliest(pool).await.unwrap(), Some(earliest));

    keys::insert(
        pool,
        "member",
        subject.organization,
        subject.workspace,
        subject.user,
    )
    .await
    .unwrap();
    assert_eq!(
        keys::active(pool).await.unwrap().len(),
        2,
        "a key derived again after its revocation is active once more"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn the_cursor_is_read_back_where_it_was_advanced() {
    let fixture = Fixture::new("spend_cursor").await;
    let pool = fixture.pool();
    assert_eq!(cursor::read(pool).await.unwrap(), None);

    let first = Utc::now().trunc_subsecs(6);
    cursor::advance(pool, first).await.unwrap();
    assert_eq!(cursor::read(pool).await.unwrap(), Some(first));
    let second = first + TimeDelta::minutes(1);
    cursor::advance(pool, second).await.unwrap();
    assert_eq!(cursor::read(pool).await.unwrap(), Some(second));
    cursor::advance(pool, first).await.unwrap();
    assert_eq!(
        cursor::read(pool).await.unwrap(),
        Some(second),
        "the cursor never moves back"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn attributed_spend_lands_with_its_organization_workspace_and_user() {
    let fixture = Fixture::new("spend_attributed").await;
    fixture.licensed().await;
    let member = attributed(&fixture).await;
    let automation = fixture.key(&fixture.automation());
    keys::insert(
        fixture.pool(),
        &automation.token,
        fixture.policy.organization,
        fixture.policy.workspace,
        None,
    )
    .await
    .unwrap();
    let start = Utc::now().trunc_subsecs(3) - TimeDelta::minutes(1);
    fixture
        .mount_logs(vec![
            log("member-request", &member.token, start, 0.012346),
            log("automation-request", &automation.token, start, 0.5),
        ])
        .await;

    assert_eq!(
        ingested(&fixture).await,
        Ingested::Done { rows: 2, stored: 2 }
    );

    let stored = fixture.stored().await;
    assert_eq!(stored.len(), 2);
    let automated = &stored[0];
    assert_eq!(automated.request_id, "automation-request");
    assert_eq!(automated.user_id, None);
    assert_eq!(automated.spend_microdollars, 500_000);
    let spent = &stored[1];
    assert_eq!(spent.request_id, "member-request");
    assert_eq!(spent.organization_id, fixture.policy.organization);
    assert_eq!(spent.workspace_id, Some(fixture.policy.workspace));
    assert_eq!(spent.user_id, Some(fixture.policy.owner.id));
    assert_eq!(spent.token, member.token);
    assert_eq!(spent.spend_microdollars, 12_346);
    assert_eq!((spent.prompt_tokens, spent.completion_tokens), (100, 20));
    assert_eq!(
        (spent.model.as_str(), spent.provider.as_str()),
        ("gpt-4o", "openai")
    );
    assert!(!spent.failed);
    assert_eq!(spent.occurred_at, start);

    fixture.cleanup().await;
}

#[tokio::test]
async fn spend_under_keys_zone_did_not_derive_is_skipped() {
    let fixture = Fixture::new("spend_foreign").await;
    fixture.licensed().await;
    let member = attributed(&fixture).await;
    let now = Utc::now();
    fixture
        .mount_logs(vec![
            log("foreign", &Key::hash("sk-someone-elses-key-0000"), now, 1.0),
            log("master", &Key::hash(MASTER), now, 1.0),
            log("ours", &member.token, now, 1.0),
        ])
        .await;

    assert_eq!(
        ingested(&fixture).await,
        Ingested::Done { rows: 3, stored: 1 }
    );
    let stored = fixture.stored().await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].request_id, "ours");

    fixture.cleanup().await;
}

#[tokio::test]
async fn ingesting_again_keeps_one_record_with_the_newest_spend() {
    let fixture = Fixture::new("spend_idempotent").await;
    fixture.licensed().await;
    let member = attributed(&fixture).await;
    let start = Utc::now() - TimeDelta::minutes(2);
    fixture
        .mount_logs(vec![log("request", &member.token, start, 0.1)])
        .await;
    ingested(&fixture).await;
    ingested(&fixture).await;
    assert_eq!(fixture.stored().await.len(), 1);

    fixture.litellm.reset().await;
    let mut settled = log("request", &member.token, start, 0.2);
    settled["status"] = "failure".into();
    settled["completion_tokens"] = 40.into();
    fixture.mount_logs(vec![settled]).await;
    ingested(&fixture).await;

    let stored = fixture.stored().await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].spend_microdollars, 200_000);
    assert_eq!(stored[0].completion_tokens, 40);
    assert!(stored[0].failed);

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_run_advances_the_cursor_to_the_end_it_read_through() {
    let fixture = Fixture::new("spend_cursor_end").await;
    fixture.licensed().await;
    let member = attributed(&fixture).await;
    let earliest = keys::earliest(fixture.pool()).await.unwrap().unwrap();
    fixture
        .mount_logs(vec![log("request", &member.token, Utc::now(), 0.1)])
        .await;

    let before = Utc::now().trunc_subsecs(6);
    ingested(&fixture).await;
    let after = Utc::now();

    let through = cursor::read(fixture.pool()).await.unwrap().unwrap();
    assert!(
        before <= through && through <= after,
        "{before} {through} {after}"
    );
    let requests = fixture.received(LOGS_PATH).await;
    assert_eq!(requests.len(), 1);
    assert_eq!(query(&requests[0], "end_date"), seconds(through));
    assert_eq!(
        query(&requests[0], "start_date"),
        seconds(earliest - overlap()),
        "the first run reads from the overlap before the first key"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_request_logged_late_inside_the_overlap_is_still_ingested() {
    let fixture = Fixture::new("spend_overlap").await;
    fixture.licensed().await;
    let member = attributed(&fixture).await;
    let through = Utc::now().trunc_subsecs(3);
    cursor::advance(fixture.pool(), through).await.unwrap();
    let late = through - TimeDelta::minutes(30);
    fixture
        .mount_logs(vec![log("late", &member.token, late, 0.3)])
        .await;

    assert_eq!(
        ingested(&fixture).await,
        Ingested::Done { rows: 1, stored: 1 }
    );

    assert_eq!(fixture.stored().await[0].occurred_at, late);
    let requests = fixture.received(LOGS_PATH).await;
    assert_eq!(
        query(&requests[0], "start_date"),
        seconds(through - overlap())
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_failed_page_keeps_the_cursor_at_the_last_page_stored() {
    let fixture = Fixture::new("spend_partial").await;
    fixture.licensed().await;
    let member = attributed(&fixture).await;
    sqlx::query("UPDATE spend_keys SET created_at = now() - interval '1 hour'")
        .execute(fixture.pool())
        .await
        .unwrap();
    let first = Utc::now().trunc_subsecs(3) - TimeDelta::seconds(10);
    let page = |number: i64| -> Value {
        let rows: Vec<Value> = (0..PAGE_SIZE as i64)
            .map(|row| {
                let index = number * PAGE_SIZE as i64 + row;
                let start = first + TimeDelta::milliseconds(index);
                log(&format!("request-{index}"), &member.token, start, 0.001)
            })
            .collect();
        json!({ "data": rows })
    };
    for number in 0..2 {
        Mock::given(method("GET"))
            .and(path(LOGS_PATH))
            .and(query_param("page", (number + 1).to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(page(number)))
            .expect(1)
            .mount(&fixture.litellm)
            .await;
    }
    Mock::given(method("GET"))
        .and(path(LOGS_PATH))
        .and(query_param("page", "3"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&fixture.litellm)
        .await;

    let failed = ingest::run(fixture.state(), &fixture.client()).await;
    assert!(
        matches!(
            failed,
            Err(Error::Status(StatusCode::INTERNAL_SERVER_ERROR))
        ),
        "{failed:?}"
    );
    let through = first + TimeDelta::milliseconds(2 * PAGE_SIZE as i64 - 1);
    assert_eq!(
        cursor::read(fixture.pool()).await.unwrap(),
        Some(through),
        "the cursor keeps the last start of the last page stored"
    );
    assert_eq!(fixture.stored().await.len(), 2 * PAGE_SIZE);
    fixture.litellm.verify().await;

    fixture.litellm.reset().await;
    fixture.mount_logs(Vec::new()).await;
    assert_eq!(
        ingested(&fixture).await,
        Ingested::Done { rows: 0, stored: 0 }
    );
    let requests = fixture.received(LOGS_PATH).await;
    assert_eq!(
        query(&requests[0], "start_date"),
        seconds(through - overlap()),
        "the next run picks up the overlap before the pages already stored"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_run_while_another_instance_ingests_is_skipped() {
    let fixture = Fixture::new("spend_locked").await;
    fixture.licensed().await;
    attributed(&fixture).await;
    let held = Lock::attempt(fixture.pool(), SPEND_LOCK)
        .await
        .unwrap()
        .expect("nothing else holds the lock");

    assert_eq!(ingested(&fixture).await, Ingested::Skipped);
    assert!(fixture.received(LOGS_PATH).await.is_empty());
    assert_eq!(cursor::read(fixture.pool()).await.unwrap(), None);

    held.release().await.unwrap();
    fixture.mount_logs(Vec::new()).await;
    assert_eq!(
        ingested(&fixture).await,
        Ingested::Done { rows: 0, stored: 0 }
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn with_no_key_there_is_nothing_to_ingest() {
    let fixture = Fixture::new("spend_idle").await;
    fixture.licensed().await;

    assert_eq!(ingested(&fixture).await, Ingested::Idle);
    assert!(fixture.received(LOGS_PATH).await.is_empty());

    fixture.cleanup().await;
}

#[tokio::test]
async fn community_sends_the_master_key_and_registers_nothing() {
    let fixture = Fixture::new("spend_community").await;
    Fixture::registration()
        .expect(0)
        .mount(&fixture.litellm)
        .await;

    let endpoint = attribute(
        fixture.state(),
        fixture.instance(),
        &fixture.subject(),
        &Standing::read(fixture.state()).await,
    )
    .await;

    assert_eq!(endpoint.key().expose(), MASTER);
    assert!(keys::active(fixture.pool()).await.unwrap().is_empty());
    assert_eq!(ingested(&fixture).await, Ingested::Idle);
    fixture.litellm.verify().await;

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_still_attributes() {
    let fixture = Fixture::new("spend_lapsed").await;
    fixture.lapsed().await;
    fixture.mount_register().await;

    let endpoint = attribute(
        fixture.state(),
        fixture.instance(),
        &fixture.subject(),
        &Standing::read(fixture.state()).await,
    )
    .await;

    assert!(endpoint.key().expose().starts_with("sk-zone-"));

    fixture.cleanup().await;
}

#[tokio::test]
async fn attribution_registers_each_key_once_per_process() {
    let fixture = Fixture::new("spend_register_once").await;
    fixture.licensed().await;
    Fixture::registration()
        .expect(1)
        .mount(&fixture.litellm)
        .await;
    let key = fixture.key(&fixture.subject());

    for _ in 0..3 {
        let endpoint = attribute(
            fixture.state(),
            fixture.instance(),
            &fixture.subject(),
            &Standing::read(fixture.state()).await,
        )
        .await;
        assert!(endpoint.key().expose().starts_with("sk-zone-"));
        assert_eq!(endpoint.key().expose(), key.secret.expose());
        assert_eq!(endpoint.credential().expose(), MASTER);
    }

    fixture.litellm.verify().await;
    let active = keys::active(fixture.pool()).await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].token, key.token);
    assert_eq!(active[0].user_id, Some(fixture.policy.owner.id));
    let registered = fixture.received(GENERATE_PATH).await;
    let body: serde_json::Value = serde_json::from_slice(&registered[0].body).unwrap();
    assert_eq!(
        body["metadata"]["zone_workspace"],
        fixture.policy.workspace.to_string()
    );
    assert!(body.get("key_alias").is_none() && body.get("user_id").is_none());

    fixture.cleanup().await;
}

#[tokio::test]
async fn an_endpoint_that_is_not_attributable_is_sent_as_it_is() {
    let fixture = Fixture::new("spend_unattributable").await;
    fixture.licensed().await;
    Fixture::registration()
        .expect(0)
        .mount(&fixture.litellm)
        .await;
    let saved = fixture
        .instance()
        .with_key(abnegate_secret::SecretValue::new(
            "sk-workspace-saved-key-000",
        ));

    let endpoint = attribute(
        fixture.state(),
        saved,
        &fixture.subject(),
        &Standing::read(fixture.state()).await,
    )
    .await;

    assert_eq!(endpoint.key().expose(), "sk-workspace-saved-key-000");
    fixture.litellm.verify().await;

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_failed_registration_sends_the_master_key_and_cools_before_retrying() {
    let fixture = Fixture::new("spend_register_failed").await;
    fixture.licensed().await;
    Mock::given(method("POST"))
        .and(path(GENERATE_PATH))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&fixture.litellm)
        .await;
    let key = fixture.key(&fixture.subject());

    for _ in 0..2 {
        let endpoint = attribute(
            fixture.state(),
            fixture.instance(),
            &fixture.subject(),
            &Standing::read(fixture.state()).await,
        )
        .await;
        assert_eq!(endpoint.key().expose(), MASTER);
    }

    let host = fixture.client().host().to_string();
    assert!(registry::cooling(&host, &key.token));
    assert!(!registry::registered(&host, &key.token));
    assert!(keys::active(fixture.pool()).await.unwrap().is_empty());
    fixture.litellm.verify().await;

    fixture.cleanup().await;
}

#[tokio::test]
async fn verify_registers_again_a_key_litellm_lost() {
    let fixture = Fixture::new("spend_verify").await;
    fixture.licensed().await;
    let key = attributed(&fixture).await;
    fixture.litellm.reset().await;
    fixture.mount_missing_key().await;
    Fixture::registration()
        .expect(1)
        .mount(&fixture.litellm)
        .await;

    assert_eq!(
        upkeep::verify(fixture.state(), &fixture.client())
            .await
            .unwrap(),
        1
    );

    let endpoint = attribute(
        fixture.state(),
        fixture.instance(),
        &fixture.subject(),
        &Standing::read(fixture.state()).await,
    )
    .await;
    assert_eq!(endpoint.key().expose(), key.secret.expose());
    fixture.litellm.verify().await;
    assert_eq!(keys::active(fixture.pool()).await.unwrap().len(), 1);

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_fixture_never_sees_keys_an_earlier_fixture_registered() {
    let earlier = Fixture::new("spend_isolated_earlier").await;
    earlier.licensed().await;
    attributed(&earlier).await;
    let host = earlier.client().host().to_string();
    assert_eq!(registry::tokens(&host).len(), 1);
    earlier.cleanup().await;

    let fixture = Fixture::new("spend_isolated").await;
    let host = fixture.client().host().to_string();
    assert!(
        registry::tokens(&host).is_empty(),
        "{host} still holds another fixture's keys"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn verify_leaves_keys_litellm_still_has() {
    let fixture = Fixture::new("spend_verify_present").await;
    fixture.licensed().await;
    attributed(&fixture).await;
    fixture.litellm.reset().await;
    Mock::given(method("GET"))
        .and(path("/key/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "info": {} })))
        .expect(1)
        .mount(&fixture.litellm)
        .await;
    Fixture::registration()
        .expect(0)
        .mount(&fixture.litellm)
        .await;

    assert_eq!(
        upkeep::verify(fixture.state(), &fixture.client())
            .await
            .unwrap(),
        0
    );
    fixture.litellm.verify().await;

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_key_an_earlier_encryption_key_derived_is_revoked_and_its_spend_still_attributes() {
    let fixture = Fixture::new("spend_revoke").await;
    fixture.licensed().await;
    let current = attributed(&fixture).await;
    let subject = fixture.subject();
    let former = Key::derive(&MasterKey::new([0x07; 32]), subject.workspace, subject.user);
    keys::insert(
        fixture.pool(),
        &former.token,
        subject.organization,
        subject.workspace,
        subject.user,
    )
    .await
    .unwrap();
    Mock::given(method("POST"))
        .and(path(DELETE_PATH))
        .and(body_json(serde_json::json!({ "keys": [former.token] })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "deleted_keys": [former.token] })),
        )
        .expect(1)
        .mount(&fixture.litellm)
        .await;

    assert_eq!(
        upkeep::revoke(fixture.state(), &fixture.client())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        upkeep::revoke(fixture.state(), &fixture.client())
            .await
            .unwrap(),
        0,
        "a revoked key is not revoked again"
    );
    fixture.litellm.verify().await;

    let active = keys::active(fixture.pool()).await.unwrap();
    assert_eq!(
        active
            .iter()
            .map(|row| row.token.as_str())
            .collect::<Vec<_>>(),
        [current.token.as_str()]
    );
    let revoked_at: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT revoked_at FROM spend_keys WHERE token = $1")
            .bind(&former.token)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert!(revoked_at.is_some());

    fixture
        .mount_logs(vec![log("late", &former.token, Utc::now(), 0.4)])
        .await;
    assert_eq!(
        ingested(&fixture).await,
        Ingested::Done { rows: 1, stored: 1 }
    );
    let stored = fixture.stored().await;
    assert_eq!(stored[0].user_id, Some(fixture.policy.owner.id));
    assert_eq!(stored[0].workspace_id, Some(fixture.policy.workspace));

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_deleted_workspace_keeps_its_spend_in_the_organization() {
    let fixture = Fixture::new("spend_workspace_deleted").await;
    fixture.licensed().await;
    let member = attributed(&fixture).await;
    fixture
        .mount_logs(vec![log("request", &member.token, Utc::now(), 0.1)])
        .await;
    ingested(&fixture).await;

    sqlx::query("DELETE FROM workspaces WHERE id = $1")
        .bind(fixture.policy.workspace)
        .execute(fixture.pool())
        .await
        .unwrap();

    let stored = fixture.stored().await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].workspace_id, None);
    assert_eq!(stored[0].organization_id, fixture.policy.organization);
    assert!(keys::active(fixture.pool()).await.unwrap().is_empty());

    fixture.cleanup().await;
}
