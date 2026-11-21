use chrono::{DateTime, NaiveDate, TimeDelta, Utc};
use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::spend::{Budget, Microdollars, Month, Scope, State, Target};
use zone_server::db::migrations;
use zone_server::db::spend::budgets::{self, Announcement, BudgetRow, CreateError, Crossing};

use crate::common::Database;

const PREFIX: &str = "spend_store";
const TOKEN: &str = "spend-store-token";

struct Fixture {
    database: Database,
}

impl Fixture {
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
        let updated = budgets::update(&mut transaction, id, &budget, actor, month())
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
    let fixture = Fixture::new().await;
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
    let fixture = Fixture::new().await;
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
        budgets::get(fixture.pool(), organization, created.id, month())
            .await
            .unwrap(),
        Some(created.clone())
    );
    let stranger = fixture.organization().await;
    assert_eq!(
        budgets::get(fixture.pool(), stranger, created.id, month())
            .await
            .unwrap(),
        None,
        "another organization's budget is not found"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn the_database_refuses_budgets_the_domain_refuses() {
    let fixture = Fixture::new().await;
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
    let fixture = Fixture::new().await;
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
    let fixture = Fixture::new().await;
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
    assert_eq!(
        budgets::get(fixture.pool(), organization, scoped.id, month())
            .await
            .unwrap(),
        None
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn update_writes_the_budget_and_keeps_the_stamp_when_nothing_changes() {
    let fixture = Fixture::new().await;
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
    let fixture = Fixture::new().await;
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
    fixture
        .spend(
            organization,
            None,
            None,
            7,
            Utc::now() - TimeDelta::seconds(1),
        )
        .await;

    let mut transaction = fixture.pool().begin().await.unwrap();
    let locked = budgets::lock(&mut transaction, organization, created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(locked.id, created.id);
    assert_eq!(
        locked.spent,
        Microdollars(7),
        "lock reads this month's spend"
    );
    assert_eq!(
        budgets::lock(&mut transaction, stranger, created.id)
            .await
            .unwrap(),
        None
    );
    assert!(budgets::delete(&mut transaction, created.id).await.unwrap());
    assert!(!budgets::delete(&mut transaction, created.id).await.unwrap());
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
    let fixture = Fixture::new().await;
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
    let fixture = Fixture::new().await;
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
    let fixture = Fixture::new().await;
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
    let fixture = Fixture::new().await;
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
