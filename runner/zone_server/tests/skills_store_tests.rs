//! The one statement the skills index issues, executed.
//!
//! `knowledge::skills` is written in `db/knowledge.rs`'s runtime `query_as`
//! style, so nothing about its SQL is checked at compile time. What it has to
//! get right is scope — one workspace's skills and nobody else's, active ones
//! only, filed under the skill category and no other — and the bound, which
//! reaches one row past the limit so the index can say it stopped short.
mod common;

use sqlx::PgPool;
use uuid::Uuid;
use zone_server::agent::skills::{self, MAX_SKILLS};
use zone_server::db::knowledge::{self, SKILL_CATEGORY, SKILL_HEAD_CHARS};

struct Fixture {
    pool: PgPool,
    workspace: Uuid,
    elsewhere: Uuid,
    user: Uuid,
}

async fn fixture() -> Fixture {
    let pool = PgPool::connect(&common::context_database_url())
        .await
        .expect("the skills store tests need a migrated disposable database");
    let organization: Uuid =
        sqlx::query_scalar("INSERT INTO organizations (name, slug) VALUES ($1, $2) RETURNING id")
            .bind("Skills")
            .bind(Uuid::new_v4().to_string())
            .fetch_one(&pool)
            .await
            .expect("an organization");
    let mine = workspace(&pool, organization).await;
    let theirs = workspace(&pool, organization).await;
    let user: Uuid =
        sqlx::query_scalar("INSERT INTO users (email, password_hash) VALUES ($1, $2) RETURNING id")
            .bind(format!("{}@example.test", Uuid::new_v4()))
            .bind("x")
            .fetch_one(&pool)
            .await
            .expect("a user");
    Fixture {
        pool,
        workspace: mine,
        elsewhere: theirs,
        user,
    }
}

async fn workspace(pool: &PgPool, organization: Uuid) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO workspaces (organization_id, name, slug) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(organization)
    .bind("Skills")
    .bind(Uuid::new_v4().to_string())
    .fetch_one(pool)
    .await
    .expect("a workspace")
}

async fn file(
    fixture: &Fixture,
    workspace: Uuid,
    category: Option<&str>,
    title: &str,
    content: &str,
) -> Uuid {
    knowledge::create_knowledge(
        &fixture.pool,
        workspace,
        title,
        content,
        category,
        &[],
        1,
        fixture.user,
    )
    .await
    .expect("a document")
}

fn skill_md(trigger: &str) -> String {
    format!("---\nname: skill\ndescription: {trigger}\n---\n\n# Procedure\n\nStep one.\n")
}

#[tokio::test]
async fn the_index_holds_one_workspace_s_active_skills_and_nothing_else() {
    let fixture = fixture().await;
    let deploy = file(
        &fixture,
        fixture.workspace,
        Some(SKILL_CATEGORY),
        "Deploy checklist",
        &skill_md("Use when shipping to production."),
    )
    .await;
    file(
        &fixture,
        fixture.workspace,
        Some("standing-instruction"),
        "Not a skill",
        "Never edit a shipped migration.",
    )
    .await;
    file(&fixture, fixture.workspace, None, "Uncategorised", "Prose.").await;
    file(
        &fixture,
        fixture.elsewhere,
        Some(SKILL_CATEGORY),
        "Another workspace's skill",
        &skill_md("Theirs."),
    )
    .await;
    let forgotten = file(
        &fixture,
        fixture.workspace,
        Some(SKILL_CATEGORY),
        "Retired",
        &skill_md("Was."),
    )
    .await;
    sqlx::query("UPDATE knowledge_entries SET is_active = FALSE WHERE id = $1")
        .bind(forgotten)
        .execute(&fixture.pool)
        .await
        .unwrap();

    let rows = knowledge::skills(&fixture.pool, fixture.workspace, 10)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].id, deploy);
    assert_eq!(rows[0].title, "Deploy checklist");
    assert!(
        rows[0].head.starts_with("---\nname: skill"),
        "{}",
        rows[0].head
    );

    let rendered = skills::prompt(&fixture.pool, fixture.workspace)
        .await
        .unwrap();
    assert!(
        rendered.contains(&format!(
            "- Deploy checklist [{deploy}]: Use when shipping to production."
        )),
        "{rendered}"
    );
    assert!(!rendered.contains("Not a skill"), "{rendered}");
    assert!(!rendered.contains("Retired"), "{rendered}");
    assert!(!rendered.contains("Another workspace"), "{rendered}");
    assert_eq!(
        skills::prompt(&fixture.pool, fixture.elsewhere)
            .await
            .unwrap()
            .matches("\n- ")
            .count(),
        1
    );
}

#[tokio::test]
async fn the_store_reaches_one_past_the_bound_and_reads_only_the_head() {
    let fixture = fixture().await;
    let long = format!("{}{}", skill_md("First."), "x".repeat(5_000));
    for index in 0..MAX_SKILLS + 2 {
        file(
            &fixture,
            fixture.workspace,
            Some(SKILL_CATEGORY),
            &format!("Skill {index:02}"),
            &long,
        )
        .await;
    }
    let limit = i64::try_from(MAX_SKILLS).unwrap();
    let rows = knowledge::skills(&fixture.pool, fixture.workspace, limit)
        .await
        .unwrap();
    assert_eq!(rows.len(), MAX_SKILLS + 1, "one past the bound, no further");
    assert!(
        rows.windows(2).all(|pair| pair[0].title <= pair[1].title),
        "title order"
    );
    assert!(
        rows.iter()
            .all(|row| row.head.chars().count() <= SKILL_HEAD_CHARS as usize),
        "the head is bounded so a listing never fetches a procedure"
    );
    let rendered = skills::prompt(&fixture.pool, fixture.workspace)
        .await
        .unwrap();
    assert_eq!(
        rendered.matches("\n- Skill ").count(),
        MAX_SKILLS,
        "{rendered}"
    );
    assert!(
        rendered.contains("2 more skills are filed under skill"),
        "{rendered}"
    );
}
