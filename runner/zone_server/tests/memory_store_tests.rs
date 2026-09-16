//! Every statement the memory store issues, executed.
//!
//! The store is written in `db/knowledge.rs`'s runtime `query_as` style rather
//! than with sqlx's checked macros, so the compiler verifies nothing about the
//! SQL: not a column name, not a bind number, not a type. These tests are the
//! only thing standing between a typo and a lost memory, so each of the five
//! statements is executed here, and each of the store's outcomes is reached
//! through the behaviour that produces it rather than asserted about in the
//! abstract.
//!
//! The scoping tests matter most. `knowledge_entries` is a workspace-wide table
//! and these rows belong to one person inside it, so a statement that forgets
//! `created_by` is a privacy failure that no type would catch.

mod common;

use std::time::Duration;

use sqlx::PgPool;
use uuid::Uuid;
use zone_server::db::knowledge;
use zone_server::db::memory::{
    self, MAX_ENTRY_CHARS, MAX_FACTS, MAX_NAME_CHARS, MemoryCategory, MemoryOutcome, MemoryWrite,
    PREFERENCES_TITLE, PROFILE_TITLE,
};

const DEPLOY_WINDOW: &str = "Deploy window";

/// Two people in one workspace, and the same person in a second workspace:
/// every boundary a memory read must not cross.
struct Fixture {
    pool: PgPool,
    organization: Uuid,
    workspace: Uuid,
    elsewhere: Uuid,
    user: Uuid,
    other: Uuid,
}

async fn fixture() -> Fixture {
    let pool = PgPool::connect(&common::context_database_url())
        .await
        .expect("the memory store tests need a migrated disposable database");

    let organization: Uuid =
        sqlx::query_scalar("INSERT INTO organizations (name, slug) VALUES ($1, $2) RETURNING id")
            .bind("Memory")
            .bind(Uuid::new_v4().to_string())
            .fetch_one(&pool)
            .await
            .expect("an organization");

    Fixture {
        workspace: workspace(&pool, organization).await,
        elsewhere: workspace(&pool, organization).await,
        user: user(&pool).await,
        other: user(&pool).await,
        organization,
        pool,
    }
}

async fn workspace(pool: &PgPool, organization: Uuid) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO workspaces (organization_id, name, slug) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(organization)
    .bind("Memory")
    .bind(Uuid::new_v4().to_string())
    .fetch_one(pool)
    .await
    .expect("a workspace")
}

async fn user(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO users (email, password_hash) VALUES ($1, $2) RETURNING id")
        .bind(format!("{}@example.test", Uuid::new_v4()))
        .bind("x")
        .fetch_one(pool)
        .await
        .expect("a user")
}

fn creation<'a>(
    fixture: &Fixture,
    category: MemoryCategory,
    title: &'a str,
    content: &'a str,
) -> MemoryWrite<'a> {
    MemoryWrite {
        workspace_id: fixture.workspace,
        user_id: fixture.user,
        category,
        title,
        description: None,
        content,
        version: None,
    }
}

fn replacement<'a>(
    fixture: &Fixture,
    category: MemoryCategory,
    title: &'a str,
    content: &'a str,
    version: i64,
) -> MemoryWrite<'a> {
    MemoryWrite {
        version: Some(version),
        ..creation(fixture, category, title, content)
    }
}

/// What the row says, read outside the store so a scoping mistake in the store
/// cannot hide a write the store then cannot see.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
struct Stored {
    content: String,
    description: Option<String>,
    version: i64,
    token_count: i32,
    is_active: Option<bool>,
}

async fn stored(pool: &PgPool, id: Uuid) -> Stored {
    sqlx::query_as(
        "SELECT content, description, version, token_count, is_active
         FROM knowledge_entries WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("the row is readable by id")
}

async fn rows_in(pool: &PgPool, workspace: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_entries WHERE workspace_id = $1")
        .bind(workspace)
        .fetch_one(pool)
        .await
        .expect("the entries are countable")
}

impl Fixture {
    async fn read(&self, category: MemoryCategory, title: &str) -> Option<memory::MemoryRow> {
        memory::read(&self.pool, self.workspace, self.user, category, title)
            .await
            .expect("a read answers")
    }

    async fn index(&self, category: Option<MemoryCategory>) -> Vec<memory::MemoryIndexRow> {
        memory::index(&self.pool, self.workspace, self.user, category)
            .await
            .expect("an index answers")
    }

    async fn write(&self, write: MemoryWrite<'_>) -> MemoryOutcome {
        memory::write(&self.pool, write)
            .await
            .expect("a write answers")
    }

    async fn append(&self, category: MemoryCategory, title: &str, addition: &str) -> MemoryOutcome {
        memory::append(
            &self.pool,
            self.workspace,
            self.user,
            category,
            title,
            addition,
        )
        .await
        .expect("an append answers")
    }

    async fn forget(&self, category: MemoryCategory, title: &str, version: i64) -> MemoryOutcome {
        memory::forget(
            &self.pool,
            self.workspace,
            self.user,
            category,
            title,
            version,
        )
        .await
        .expect("a forget answers")
    }

    /// Every row this fixture opened, closed again. `knowledge_entries` hangs
    /// off the workspace and the workspace off the organization, so one delete
    /// takes the memory with it; `created_by` is `ON DELETE SET NULL`, so the
    /// people have to go separately, and after the entries rather than before.
    async fn clean(self) {
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(self.organization)
            .execute(&self.pool)
            .await
            .expect("the workspaces and everything in them go");
        sqlx::query("DELETE FROM users WHERE id = ANY($1)")
            .bind(vec![self.user, self.other])
            .execute(&self.pool)
            .await
            .expect("both people go");
    }
}

#[tokio::test]
async fn a_first_write_opens_the_entry_at_version_one() {
    let fixture = fixture().await;

    let outcome = fixture
        .write(MemoryWrite {
            description: Some("When deploys go out"),
            ..creation(
                &fixture,
                MemoryCategory::Fact,
                DEPLOY_WINDOW,
                "Thursdays, after standup.",
            )
        })
        .await;

    assert_eq!(outcome, MemoryOutcome::Created { version: 1 });

    let row = fixture
        .read(MemoryCategory::Fact, DEPLOY_WINDOW)
        .await
        .expect("the entry that was just created is readable");
    assert_eq!(row.content, "Thursdays, after standup.");
    assert_eq!(row.description.as_deref(), Some("When deploys go out"));
    assert_eq!(row.version, 1);
    assert_eq!(row.category, MemoryCategory::Fact.as_str());
    assert_eq!(row.workspace_id, fixture.workspace);
    assert_eq!(stored(&fixture.pool, row.id).await.token_count, 7);

    fixture.clean().await;
}

#[tokio::test]
async fn a_second_create_conflicts_with_what_the_first_one_stored() {
    let fixture = fixture().await;

    assert_eq!(
        fixture
            .write(creation(
                &fixture,
                MemoryCategory::Fact,
                DEPLOY_WINDOW,
                "Thursdays, after standup."
            ))
            .await,
        MemoryOutcome::Created { version: 1 }
    );

    let second = fixture
        .write(creation(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            "Tuesdays.",
        ))
        .await;

    assert_eq!(
        second,
        MemoryOutcome::Conflict {
            version: 1,
            content: "Thursdays, after standup.".to_string(),
        },
        "a create that lands on an entry that already exists must hand back what \
         it says so the model can merge, never overwrite it"
    );
    assert_eq!(
        rows_in(&fixture.pool, fixture.workspace).await,
        1,
        "the refused create must not have inserted a second row"
    );

    fixture.clean().await;
}

#[tokio::test]
async fn a_write_quoting_the_version_it_read_replaces_the_entry() {
    let fixture = fixture().await;
    fixture
        .write(creation(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            "Thursdays.",
        ))
        .await;

    let outcome = fixture
        .write(replacement(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            "Tuesdays, now.",
            1,
        ))
        .await;

    assert_eq!(outcome, MemoryOutcome::Updated { version: 2 });
    let row = fixture
        .read(MemoryCategory::Fact, DEPLOY_WINDOW)
        .await
        .expect("the replaced entry is readable");
    assert_eq!(row.content, "Tuesdays, now.");
    assert_eq!(row.version, 2);

    fixture.clean().await;
}

#[tokio::test]
async fn a_write_quoting_a_stale_version_conflicts_and_leaves_the_entry_alone() {
    let fixture = fixture().await;
    fixture
        .write(MemoryWrite {
            description: Some("When deploys go out"),
            ..creation(&fixture, MemoryCategory::Fact, DEPLOY_WINDOW, "Thursdays.")
        })
        .await;
    fixture
        .write(replacement(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            "Tuesdays, now.",
            1,
        ))
        .await;
    let id = fixture
        .read(MemoryCategory::Fact, DEPLOY_WINDOW)
        .await
        .expect("the entry exists")
        .id;
    let before = stored(&fixture.pool, id).await;

    let outcome = fixture
        .write(replacement(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            "Every day, actually.",
            1,
        ))
        .await;

    assert_eq!(
        outcome,
        MemoryOutcome::Conflict {
            version: 2,
            content: "Tuesdays, now.".to_string(),
        }
    );
    assert_eq!(
        stored(&fixture.pool, id).await,
        before,
        "a conflicted write must change nothing at all -- not the content, not \
         the version, not the description, not the token count"
    );

    fixture.clean().await;
}

#[tokio::test]
async fn a_write_forces_the_title_of_a_category_that_holds_one_entry() {
    let fixture = fixture().await;

    assert_eq!(
        fixture
            .write(creation(
                &fixture,
                MemoryCategory::Profile,
                "whatever the model called it",
                "Writes Rust, reviews carefully."
            ))
            .await,
        MemoryOutcome::Created { version: 1 }
    );

    let row = fixture
        .read(MemoryCategory::Profile, "a different name again")
        .await
        .expect("the profile is reachable under its own title whatever was supplied");
    assert_eq!(row.title, PROFILE_TITLE);
    assert_eq!(
        fixture
            .write(creation(
                &fixture,
                MemoryCategory::Profile,
                "a third name",
                "Something else."
            ))
            .await,
        MemoryOutcome::Conflict {
            version: 1,
            content: "Writes Rust, reviews carefully.".to_string(),
        },
        "a second profile under another name would be a second profile"
    );

    fixture.clean().await;
}

#[tokio::test]
async fn an_append_adds_its_line_and_bumps_the_version() {
    let fixture = fixture().await;
    fixture
        .write(creation(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            "Thursdays.",
        ))
        .await;

    let outcome = fixture
        .append(MemoryCategory::Fact, DEPLOY_WINDOW, "And Fridays.")
        .await;

    assert_eq!(outcome, MemoryOutcome::Updated { version: 2 });
    let row = fixture
        .read(MemoryCategory::Fact, DEPLOY_WINDOW)
        .await
        .expect("the appended entry is readable");
    assert_eq!(row.content, "Thursdays.\nAnd Fridays.");
    assert_eq!(row.version, 2);
    assert_eq!(
        stored(&fixture.pool, row.id).await.token_count,
        6,
        "the token count is recomputed over the composition, not left behind"
    );

    fixture.clean().await;
}

#[tokio::test]
async fn an_append_to_an_entry_that_is_not_there_says_so() {
    let fixture = fixture().await;

    assert_eq!(
        fixture
            .append(MemoryCategory::Fact, DEPLOY_WINDOW, "And Fridays.")
            .await,
        MemoryOutcome::Missing,
        "an append must not create the entry it was asked to add to"
    );
    assert_eq!(rows_in(&fixture.pool, fixture.workspace).await, 0);

    fixture.clean().await;
}

#[tokio::test]
async fn an_append_that_would_overrun_the_entry_limit_writes_nothing() {
    let fixture = fixture().await;
    let full = "a".repeat(MAX_ENTRY_CHARS);
    fixture
        .write(creation(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            &full,
        ))
        .await;
    let id = fixture
        .read(MemoryCategory::Fact, DEPLOY_WINDOW)
        .await
        .expect("the entry exists")
        .id;
    let before = stored(&fixture.pool, id).await;

    let outcome = fixture
        .append(MemoryCategory::Fact, DEPLOY_WINDOW, "b")
        .await;

    assert_eq!(
        outcome,
        MemoryOutcome::TooLong {
            limit: MAX_ENTRY_CHARS
        }
    );
    assert_eq!(
        stored(&fixture.pool, id).await,
        before,
        "a refused append must not have written a truncated composition"
    );

    fixture.clean().await;
}

/// Rather than racing an append against a write and hoping to land in the
/// window, hold the row the append has to update and watch it wait: its read
/// has already happened, so the version it is about to quote is the one this
/// test is about to invalidate.
#[tokio::test]
async fn an_append_whose_entry_moves_under_it_conflicts_and_writes_nothing() {
    let fixture = fixture().await;
    fixture
        .write(creation(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            "Thursdays.",
        ))
        .await;

    let mut holder = fixture
        .pool
        .begin()
        .await
        .expect("a transaction to hold the row");
    sqlx::query(
        "SELECT id FROM knowledge_entries
         WHERE workspace_id = $1 AND created_by = $2 AND category = $3 AND title = $4
         FOR UPDATE",
    )
    .bind(fixture.workspace)
    .bind(fixture.user)
    .bind(MemoryCategory::Fact.as_str())
    .bind(DEPLOY_WINDOW)
    .fetch_one(&mut *holder)
    .await
    .expect("the entry is locked");

    let racing = tokio::spawn({
        let pool = fixture.pool.clone();
        let workspace = fixture.workspace;
        let user = fixture.user;
        async move {
            memory::append(
                &pool,
                workspace,
                user,
                MemoryCategory::Fact,
                DEPLOY_WINDOW,
                "And Fridays.",
            )
            .await
        }
    });

    tokio::time::sleep(Duration::from_millis(750)).await;
    assert!(
        !racing.is_finished(),
        "the append did not wait for the row it has to update, so this test is \
         not in the window it means to be in"
    );

    sqlx::query(
        "UPDATE knowledge_entries SET content = $5, version = version + 1, updated_at = NOW()
         WHERE workspace_id = $1 AND created_by = $2 AND category = $3 AND title = $4",
    )
    .bind(fixture.workspace)
    .bind(fixture.user)
    .bind(MemoryCategory::Fact.as_str())
    .bind(DEPLOY_WINDOW)
    .bind("Tuesdays, now.")
    .execute(&mut *holder)
    .await
    .expect("another writer replaces the entry");
    holder.commit().await.expect("the other write lands");

    let outcome = tokio::time::timeout(Duration::from_secs(10), racing)
        .await
        .expect("the append stops waiting")
        .expect("the append task ran")
        .expect("the append is answered");

    assert_eq!(
        outcome,
        MemoryOutcome::Conflict {
            version: 2,
            content: "Tuesdays, now.".to_string(),
        },
        "the append quoted the version it read, which had moved, so it must be \
         told what the entry says now"
    );
    let row = fixture
        .read(MemoryCategory::Fact, DEPLOY_WINDOW)
        .await
        .expect("the entry is still there");
    assert_eq!(
        row.content, "Tuesdays, now.",
        "the conflicted append must not have appended to the row it did not read"
    );
    assert_eq!(row.version, 2);

    fixture.clean().await;
}

#[tokio::test]
async fn forgetting_with_the_version_it_read_deactivates_the_entry() {
    let fixture = fixture().await;
    fixture
        .write(creation(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            "Thursdays.",
        ))
        .await;
    let id = fixture
        .read(MemoryCategory::Fact, DEPLOY_WINDOW)
        .await
        .expect("the entry exists")
        .id;

    assert_eq!(
        fixture.forget(MemoryCategory::Fact, DEPLOY_WINDOW, 1).await,
        MemoryOutcome::Updated { version: 1 }
    );

    assert!(
        fixture
            .read(MemoryCategory::Fact, DEPLOY_WINDOW)
            .await
            .is_none(),
        "a forgotten entry is returned by no read"
    );
    assert!(fixture.index(None).await.is_empty());
    assert_eq!(
        stored(&fixture.pool, id).await.is_active,
        Some(false),
        "forgetting is soft: the row is still there, deactivated"
    );
    assert_eq!(
        fixture.forget(MemoryCategory::Fact, DEPLOY_WINDOW, 1).await,
        MemoryOutcome::Missing,
        "forgetting an entry that is already forgotten finds nothing"
    );

    fixture.clean().await;
}

#[tokio::test]
async fn forgetting_with_a_stale_version_conflicts_and_keeps_the_entry() {
    let fixture = fixture().await;
    fixture
        .write(creation(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            "Thursdays.",
        ))
        .await;
    fixture
        .write(replacement(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            "Tuesdays, now.",
            1,
        ))
        .await;
    let id = fixture
        .read(MemoryCategory::Fact, DEPLOY_WINDOW)
        .await
        .expect("the entry exists")
        .id;

    let outcome = fixture.forget(MemoryCategory::Fact, DEPLOY_WINDOW, 1).await;

    assert_eq!(
        outcome,
        MemoryOutcome::Conflict {
            version: 2,
            content: "Tuesdays, now.".to_string(),
        },
        "a delete proves the row was read, so a stale token must not delete"
    );
    assert_eq!(stored(&fixture.pool, id).await.is_active, Some(true));

    fixture.clean().await;
}

#[tokio::test]
async fn a_write_longer_than_one_entry_holds_stores_nothing() {
    let fixture = fixture().await;
    let over = "a".repeat(MAX_ENTRY_CHARS + 1);

    let outcome = fixture
        .write(creation(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            &over,
        ))
        .await;

    assert_eq!(
        outcome,
        MemoryOutcome::TooLong {
            limit: MAX_ENTRY_CHARS
        }
    );
    assert!(
        fixture
            .read(MemoryCategory::Fact, DEPLOY_WINDOW)
            .await
            .is_none()
    );
    assert_eq!(
        rows_in(&fixture.pool, fixture.workspace).await,
        0,
        "over-length content is refused, never truncated and stored"
    );

    let exactly = "a".repeat(MAX_ENTRY_CHARS);
    assert_eq!(
        fixture
            .write(creation(
                &fixture,
                MemoryCategory::Fact,
                DEPLOY_WINDOW,
                &exactly
            ))
            .await,
        MemoryOutcome::Created { version: 1 },
        "the limit itself is allowed, so the refusal is off by nothing"
    );

    fixture.clean().await;
}

#[tokio::test]
async fn no_read_and_no_index_crosses_from_one_person_to_another() {
    let fixture = fixture().await;
    fixture
        .write(creation(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            "Thursdays.",
        ))
        .await;

    assert!(
        memory::read(
            &fixture.pool,
            fixture.workspace,
            fixture.other,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
        )
        .await
        .expect("a read answers")
        .is_none(),
        "one workspace member must not read another member's memory"
    );
    assert!(
        memory::index(&fixture.pool, fixture.workspace, fixture.other, None)
            .await
            .expect("an index answers")
            .is_empty(),
        "one workspace member must not see another member's memory listed"
    );

    fixture.clean().await;
}

#[tokio::test]
async fn no_read_and_no_index_crosses_from_one_workspace_to_another() {
    let fixture = fixture().await;
    fixture
        .write(creation(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            "Thursdays.",
        ))
        .await;

    assert!(
        memory::read(
            &fixture.pool,
            fixture.elsewhere,
            fixture.user,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
        )
        .await
        .expect("a read answers")
        .is_none(),
        "memory written in one workspace must not appear in another"
    );
    assert!(
        memory::index(&fixture.pool, fixture.elsewhere, fixture.user, None)
            .await
            .expect("an index answers")
            .is_empty()
    );

    fixture.clean().await;
}

/// `created_by` is nullable and `ON DELETE SET NULL`, so a deleted account
/// leaves its memory behind with no owner. `NULL = $2` is never true, which is
/// what makes such a row nobody's rather than everybody's.
#[tokio::test]
async fn an_entry_whose_owner_is_gone_is_read_by_nobody_and_written_by_nobody() {
    let fixture = fixture().await;
    let orphan: Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_entries
           (workspace_id, created_by, category, title, content, token_count, version)
         VALUES ($1, NULL, $2, $3, $4, 3, 1) RETURNING id",
    )
    .bind(fixture.workspace)
    .bind(MemoryCategory::Fact.as_str())
    .bind(DEPLOY_WINDOW)
    .bind("Thursdays.")
    .fetch_one(&fixture.pool)
    .await
    .expect("an entry whose owner has been deleted");
    let before = stored(&fixture.pool, orphan).await;

    assert!(
        fixture
            .read(MemoryCategory::Fact, DEPLOY_WINDOW)
            .await
            .is_none()
    );
    assert!(fixture.index(None).await.is_empty());
    assert_eq!(
        fixture
            .write(replacement(
                &fixture,
                MemoryCategory::Fact,
                DEPLOY_WINDOW,
                "Tuesdays, now.",
                1
            ))
            .await,
        MemoryOutcome::Missing
    );
    assert_eq!(
        fixture
            .append(MemoryCategory::Fact, DEPLOY_WINDOW, "And Fridays.")
            .await,
        MemoryOutcome::Missing
    );
    assert_eq!(
        fixture.forget(MemoryCategory::Fact, DEPLOY_WINDOW, 1).await,
        MemoryOutcome::Missing
    );
    assert_eq!(
        stored(&fixture.pool, orphan).await,
        before,
        "no write may touch an entry that is nobody's"
    );

    assert_eq!(
        fixture
            .write(creation(
                &fixture,
                MemoryCategory::Fact,
                DEPLOY_WINDOW,
                "Mine, not theirs."
            ))
            .await,
        MemoryOutcome::Created { version: 1 },
        "an unowned row of the same name must not block this person from having one"
    );
    assert_eq!(
        stored(&fixture.pool, orphan).await,
        before,
        "and creating alongside it must still not have touched it"
    );

    fixture.clean().await;
}

#[tokio::test]
async fn an_index_lists_at_most_the_bound_it_carries() {
    let fixture = fixture().await;
    let beyond = usize::try_from(MAX_FACTS).expect("the fact bound fits a count") + 5;
    for number in 0..beyond {
        let title = format!("Fact {number:03}");
        assert_eq!(
            fixture
                .write(creation(
                    &fixture,
                    MemoryCategory::Fact,
                    &title,
                    "Something worth keeping."
                ))
                .await,
            MemoryOutcome::Created { version: 1 }
        );
    }
    assert_eq!(
        fixture
            .write(creation(
                &fixture,
                MemoryCategory::Profile,
                PROFILE_TITLE,
                "Ada, an electrical engineer in Wellington."
            ))
            .await,
        MemoryOutcome::Created { version: 1 }
    );
    assert_eq!(
        fixture
            .write(creation(
                &fixture,
                MemoryCategory::Preference,
                PREFERENCES_TITLE,
                "Answer with the command first."
            ))
            .await,
        MemoryOutcome::Created { version: 1 }
    );

    let listed = fixture.index(None).await;
    assert_eq!(
        i64::try_from(listed.len()).expect("a listed count fits"),
        MAX_FACTS + 1,
        "an index reaches one past what it shows, so a reader can tell something was left out"
    );
    let titles: Vec<&str> = listed.iter().map(|row| row.title.as_str()).collect();
    assert!(
        titles.contains(&PROFILE_TITLE),
        "a person with more facts than the bound still has one profile, and it must not be the \
         first thing the bound drops: {titles:?}"
    );
    assert!(
        titles.contains(&PREFERENCES_TITLE),
        "the one entry saying how to work must not be the first thing the bound drops: {titles:?}"
    );
    assert_eq!(
        i64::try_from(fixture.index(Some(MemoryCategory::Fact)).await.len())
            .expect("a listed count fits"),
        MAX_FACTS + 1
    );

    fixture.clean().await;
}

#[tokio::test]
async fn a_name_longer_than_a_name_holds_stores_nothing() {
    let fixture = fixture().await;
    let name = "n".repeat(MAX_NAME_CHARS + 1);

    let outcome = fixture
        .write(creation(
            &fixture,
            MemoryCategory::Fact,
            &name,
            "Something worth keeping.",
        ))
        .await;

    let MemoryOutcome::NameTooLong { limit } = outcome else {
        panic!("a name is bounded on its own, not as though it were content: {outcome:?}")
    };
    assert_eq!(limit, MAX_NAME_CHARS);
    assert_eq!(
        memory::name_too_long(limit),
        "That name is longer than a name holds (256 characters). Give it a shorter name; the \
         entry itself may be longer.",
        "the outcome is only ever read as what it renders, so the test reads it the same way"
    );
    assert_eq!(
        rows_in(&fixture.pool, fixture.workspace).await,
        0,
        "a refused write stores nothing"
    );

    let exactly = "n".repeat(MAX_NAME_CHARS);
    assert_eq!(
        fixture
            .write(creation(
                &fixture,
                MemoryCategory::Fact,
                &exactly,
                "Something worth keeping."
            ))
            .await,
        MemoryOutcome::Created { version: 1 },
        "the limit itself is allowed, so the refusal is off by nothing"
    );

    fixture.clean().await;
}

#[tokio::test]
async fn a_description_longer_than_one_entry_holds_stores_nothing() {
    let fixture = fixture().await;
    let description = "d".repeat(MAX_ENTRY_CHARS + 1);

    let outcome = fixture
        .write(MemoryWrite {
            description: Some(&description),
            ..creation(
                &fixture,
                MemoryCategory::Fact,
                DEPLOY_WINDOW,
                "Thursdays, after standup.",
            )
        })
        .await;

    let MemoryOutcome::TooLong { limit } = outcome else {
        panic!("a description is bounded as the content it is read beside: {outcome:?}")
    };
    assert_eq!(limit, MAX_ENTRY_CHARS);
    assert_eq!(
        memory::too_long(limit),
        "That is longer than one entry holds (2000 characters). Say it more briefly, or store it \
         as a document instead.",
        "the outcome is only ever read as what it renders, so the test reads it the same way"
    );
    assert_eq!(
        rows_in(&fixture.pool, fixture.workspace).await,
        0,
        "a refused write stores nothing"
    );

    fixture.clean().await;
}

/// A single-entry kind carries a title of the store's own, so a name the model
/// supplied for one is never what is measured.
#[tokio::test]
async fn a_forced_title_is_never_the_name_that_is_too_long() {
    let fixture = fixture().await;
    let name = "n".repeat(MAX_NAME_CHARS + 1);

    assert_eq!(
        fixture
            .write(creation(
                &fixture,
                MemoryCategory::Profile,
                &name,
                "Ada, an electrical engineer in Wellington."
            ))
            .await,
        MemoryOutcome::Created { version: 1 }
    );

    fixture.clean().await;
}

#[tokio::test]
async fn an_append_under_a_name_longer_than_a_name_holds_is_refused_before_it_reads() {
    let fixture = fixture().await;
    let name = "n".repeat(MAX_NAME_CHARS + 1);

    let outcome = fixture
        .append(MemoryCategory::Fact, &name, "One more line.")
        .await;

    let MemoryOutcome::NameTooLong { limit } = outcome else {
        panic!(
            "a name too long to store is refused for what it is, not reported as an entry that \
             was never there and not as content to shorten: {outcome:?}"
        )
    };
    assert_eq!(limit, MAX_NAME_CHARS);
    assert_eq!(
        memory::name_too_long(limit),
        "That name is longer than a name holds (256 characters). Give it a shorter name; the \
         entry itself may be longer.",
        "the outcome is only ever read as what it renders, so the test reads it the same way"
    );

    fixture.clean().await;
}

/// The create's `WHERE NOT EXISTS` is a check and not a constraint, so what
/// stops two writers that overlap exactly is the index, and only the index.
#[tokio::test]
async fn a_second_active_row_under_one_name_cannot_be_inserted_beside_the_first() {
    let fixture = fixture().await;
    assert_eq!(
        fixture
            .write(creation(
                &fixture,
                MemoryCategory::Fact,
                DEPLOY_WINDOW,
                "Thursdays, after standup."
            ))
            .await,
        MemoryOutcome::Created { version: 1 }
    );

    let duplicate = sqlx::query(
        "INSERT INTO knowledge_entries
           (workspace_id, title, content, category, token_count, created_by, version)
         VALUES ($1, $2, $3, $4, 1, $5, 1)",
    )
    .bind(fixture.workspace)
    .bind(DEPLOY_WINDOW)
    .bind("Tuesdays.")
    .bind(MemoryCategory::Fact.as_str())
    .bind(fixture.user)
    .execute(&fixture.pool)
    .await;

    let refused = duplicate.expect_err("a second active row under one name must not be insertable");
    assert!(
        matches!(&refused, sqlx::Error::Database(database) if database.is_unique_violation()),
        "the refusal has to come from the unique index rather than from anything else: {refused}"
    );
    assert_eq!(
        rows_in(&fixture.pool, fixture.workspace).await,
        1,
        "the refused insert leaves one row behind, not two"
    );

    fixture.clean().await;
}

/// An ordinary document is outside the index's predicate, and a forgotten entry
/// is outside it too: neither may be caught by a bound that is memory's.
#[tokio::test]
async fn the_name_of_a_forgotten_entry_is_free_again_and_a_document_was_never_bound() {
    let fixture = fixture().await;
    let created = fixture
        .write(creation(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            "Thursdays, after standup.",
        ))
        .await;
    assert_eq!(created, MemoryOutcome::Created { version: 1 });
    assert_eq!(
        fixture.forget(MemoryCategory::Fact, DEPLOY_WINDOW, 1).await,
        MemoryOutcome::Updated { version: 1 }
    );

    assert_eq!(
        fixture
            .write(creation(
                &fixture,
                MemoryCategory::Fact,
                DEPLOY_WINDOW,
                "Tuesdays."
            ))
            .await,
        MemoryOutcome::Created { version: 1 },
        "an entry the user asked to forget must leave its name free"
    );

    for _ in 0..2 {
        sqlx::query(
            "INSERT INTO knowledge_entries (workspace_id, title, content, token_count, created_by)
             VALUES ($1, $2, $3, 1, $4)",
        )
        .bind(fixture.workspace)
        .bind(DEPLOY_WINDOW)
        .bind("An ordinary document under the same title.")
        .bind(fixture.user)
        .execute(&fixture.pool)
        .await
        .expect("an ordinary document has no category, so no bound of memory's reaches it");
    }

    fixture.clean().await;
}

#[tokio::test]
async fn an_index_filtered_to_one_category_returns_only_that_category() {
    let fixture = fixture().await;
    fixture
        .write(creation(
            &fixture,
            MemoryCategory::Profile,
            PROFILE_TITLE,
            "Writes Rust.",
        ))
        .await;
    fixture
        .write(creation(
            &fixture,
            MemoryCategory::Preference,
            "ignored",
            "Terse answers.",
        ))
        .await;
    fixture
        .write(creation(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            "Thursdays.",
        ))
        .await;
    fixture
        .write(creation(
            &fixture,
            MemoryCategory::Fact,
            "Review rota",
            "Alternating weeks.",
        ))
        .await;

    let everything = fixture.index(None).await;
    assert_eq!(everything.len(), 4);
    let categories: Vec<&str> = everything.iter().map(|row| row.category.as_str()).collect();
    assert_eq!(
        categories,
        [
            MemoryCategory::Preference.as_str(),
            MemoryCategory::Profile.as_str(),
            MemoryCategory::Fact.as_str(),
            MemoryCategory::Fact.as_str(),
        ],
        "the kind a person has many of sorts last, so the two they have one of each survive \
         the bound; within that it is category then title"
    );
    let facts: Vec<&str> = everything[2..]
        .iter()
        .map(|row| row.title.as_str())
        .collect();
    assert_eq!(facts, [DEPLOY_WINDOW, "Review rota"]);

    for category in MemoryCategory::ALL {
        let listed = fixture.index(Some(category)).await;
        assert!(
            listed.iter().all(|row| row.category == category.as_str()),
            "an index filtered to {category} returned something else"
        );
        assert_eq!(
            listed.len(),
            if category == MemoryCategory::Fact {
                2
            } else {
                1
            },
            "an index filtered to {category} listed the wrong number of entries"
        );
    }
    assert_eq!(
        fixture.index(Some(MemoryCategory::Fact)).await[0].title,
        DEPLOY_WINDOW
    );

    fixture.clean().await;
}

#[tokio::test]
async fn a_written_memory_entry_is_never_embedded() {
    let fixture = fixture().await;
    fixture
        .write(creation(
            &fixture,
            MemoryCategory::Fact,
            DEPLOY_WINDOW,
            "Thursdays.",
        ))
        .await;
    let id = fixture
        .read(MemoryCategory::Fact, DEPLOY_WINDOW)
        .await
        .expect("the entry exists")
        .id;

    let embeddings: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM knowledge_embeddings WHERE knowledge_entry_id = $1",
    )
    .bind(id)
    .fetch_one(&fixture.pool)
    .await
    .expect("the embeddings are countable");

    assert_eq!(
        embeddings, 0,
        "a memory entry is one person's, and semantic search over the workspace \
         index is not: the store must never hand one to the indexer"
    );
    let anywhere: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_embeddings WHERE workspace_id = $1")
            .bind(fixture.workspace)
            .fetch_one(&fixture.pool)
            .await
            .expect("the embeddings are countable");
    assert_eq!(anywhere, 0);

    fixture.clean().await;
}

/// The column is `NOT NULL DEFAULT 0`, so every entry that existed before
/// memory did reads as never written by a memory tool.
#[tokio::test]
async fn a_version_starts_at_zero_on_an_entry_that_predates_memory() {
    let fixture = fixture().await;

    let id = knowledge::create_knowledge(
        &fixture.pool,
        fixture.workspace,
        "Runbook",
        "Deploy on Thursdays.",
        None,
        &[],
        5,
        fixture.user,
    )
    .await
    .expect("an ordinary knowledge entry");

    let row = stored(&fixture.pool, id).await;
    assert_eq!(row.version, 0);
    assert_eq!(row.description, None);
    assert!(
        fixture.index(None).await.is_empty(),
        "an entry outside the memory categories is not one person's memory"
    );

    fixture.clean().await;
}
