//! `role_demotion_tests` proves the guard refuses one demotion too many. This
//! proves it still refuses when two arrive at once: counting the owners and
//! demoting one of them are separate statements, so without a lock held across
//! both, two demotions each read a safe count and between them leave none.

mod common;

use std::time::Duration;

use common::create_test_pool;
use sqlx::{PgPool, Row};
use uuid::Uuid;
use zone_server::db::organization_members::{
    OrgRole, Removal, RoleChange, change_role, remove_guarded,
};

async fn user(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO users (email, password_hash) VALUES ($1, $2) RETURNING id")
        .bind(format!("{}@example.test", Uuid::new_v4()))
        .bind("x")
        .fetch_one(pool)
        .await
        .expect("a user")
}

async fn organization_owned_by(pool: &PgPool, owners: &[Uuid]) -> Uuid {
    let organization: Uuid =
        sqlx::query_scalar("INSERT INTO organizations (name, slug) VALUES ($1, $2) RETURNING id")
            .bind("Race")
            .bind(Uuid::new_v4().to_string())
            .fetch_one(pool)
            .await
            .expect("an organization");

    for owner in owners {
        sqlx::query(
            "INSERT INTO organization_members (organization_id, user_id, role) VALUES ($1, $2, 'owner')",
        )
        .bind(organization)
        .bind(owner)
        .execute(pool)
        .await
        .expect("an owner is seated");
    }

    organization
}

async fn owners(pool: &PgPool, organization: Uuid) -> i64 {
    sqlx::query(
        "SELECT COUNT(*) AS count FROM organization_members \
         WHERE organization_id = $1 AND role = 'owner' AND is_active = TRUE",
    )
    .bind(organization)
    .fetch_one(pool)
    .await
    .expect("the owners are countable")
    .get::<i64, _>("count")
}

/// Rather than racing the two demotions and hoping to land in the window, hold
/// the lock the guard depends on and watch the second demotion wait for it. A
/// guard that reads the count outside a transaction does not wait, and answers
/// from a snapshot taken before the first demotion.
#[tokio::test]
async fn a_demotion_waits_for_one_already_in_flight_and_then_sees_it() {
    let pool = create_test_pool().await;
    let first = user(&pool).await;
    let second = user(&pool).await;
    let organization = organization_owned_by(&pool, &[first, second]).await;

    let mut holder = pool.begin().await.expect("a transaction to hold the lock");
    sqlx::query(
        "SELECT user_id FROM organization_members \
         WHERE organization_id = $1 AND role = 'owner' AND is_active = TRUE FOR UPDATE",
    )
    .bind(organization)
    .fetch_all(&mut *holder)
    .await
    .expect("the owner rows are locked");

    let racing = tokio::spawn({
        let pool = pool.clone();
        async move { change_role(&pool, organization, second, OrgRole::Member, OrgRole::Owner).await }
    });

    tokio::time::sleep(Duration::from_millis(750)).await;
    assert!(
        !racing.is_finished(),
        "the second demotion did not wait for the owner rows, so it is counting \
         a snapshot taken before the first demotion lands"
    );

    sqlx::query("UPDATE organization_members SET role = 'member' WHERE organization_id = $1 AND user_id = $2")
        .bind(organization)
        .bind(first)
        .execute(&mut *holder)
        .await
        .expect("the first owner steps down");
    holder.commit().await.expect("the first demotion lands");

    let outcome = tokio::time::timeout(Duration::from_secs(10), racing)
        .await
        .expect("the second demotion stops waiting")
        .expect("the demotion task ran")
        .expect("the demotion is answered");

    assert!(
        matches!(outcome, RoleChange::LastOwner),
        "the second demotion counted the owners the first one left, so it must \
         refuse: {outcome:?}"
    );
    assert_eq!(
        owners(&pool, organization).await,
        1,
        "an organization must not be left without an owner"
    );
}

/// Removal counts the owners the same way the demotion does, so it needs the
/// same lock. Two removals arriving together would otherwise each read a count
/// that says one owner may go.
#[tokio::test]
async fn a_removal_waits_for_a_demotion_already_in_flight_and_then_sees_it() {
    let pool = create_test_pool().await;
    let first = user(&pool).await;
    let second = user(&pool).await;
    let organization = organization_owned_by(&pool, &[first, second]).await;

    let mut holder = pool.begin().await.expect("a transaction to hold the lock");
    sqlx::query(
        "SELECT user_id FROM organization_members \
         WHERE organization_id = $1 AND role = 'owner' AND is_active = TRUE FOR UPDATE",
    )
    .bind(organization)
    .fetch_all(&mut *holder)
    .await
    .expect("the owner rows are locked");

    let racing = tokio::spawn({
        let pool = pool.clone();
        async move { remove_guarded(&pool, organization, second, OrgRole::Owner).await }
    });

    tokio::time::sleep(Duration::from_millis(750)).await;
    assert!(
        !racing.is_finished(),
        "the removal did not wait for the owner rows, so it is counting a \
         snapshot taken before the demotion lands"
    );

    sqlx::query("UPDATE organization_members SET role = 'member' WHERE organization_id = $1 AND user_id = $2")
        .bind(organization)
        .bind(first)
        .execute(&mut *holder)
        .await
        .expect("the first owner steps down");
    holder.commit().await.expect("the demotion lands");

    let outcome = tokio::time::timeout(Duration::from_secs(10), racing)
        .await
        .expect("the removal stops waiting")
        .expect("the removal task ran")
        .expect("the removal is answered");

    assert!(
        matches!(outcome, Removal::LastOwner),
        "the removal counted the owners the demotion left, so it must refuse: {outcome:?}"
    );
    assert_eq!(
        owners(&pool, organization).await,
        1,
        "an organization must not be left without an owner"
    );
}

/// The count is not the only thing read too early. The routes used to fetch
/// the target's role, decide the caller outranked it, and only then call in
/// here -- so an owner promoting that target in the gap left an admin
/// demoting an admin, which the route had just refused in principle.
///
/// Rather than race it, hold the rows the guard must take and promote the
/// target while the demotion waits for them.
#[tokio::test]
async fn a_demotion_sees_the_rank_the_target_holds_when_it_lands() {
    let pool = create_test_pool().await;
    let owner = user(&pool).await;
    let target = user(&pool).await;
    let organization = organization_owned_by(&pool, &[owner]).await;

    sqlx::query(
        "INSERT INTO organization_members (organization_id, user_id, role) VALUES ($1, $2, 'member')",
    )
    .bind(organization)
    .bind(target)
    .execute(&pool)
    .await
    .expect("the target is seated as a member");

    let mut holder = pool.begin().await.expect("a transaction to hold the lock");
    sqlx::query(
        "SELECT user_id FROM organization_members \
         WHERE organization_id = $1 AND role = 'owner' AND is_active = TRUE FOR UPDATE",
    )
    .bind(organization)
    .fetch_all(&mut *holder)
    .await
    .expect("the owner rows are locked");

    // An admin re-seating a member: allowed, on the rank the target holds now.
    let racing = tokio::spawn({
        let pool = pool.clone();
        async move { change_role(&pool, organization, target, OrgRole::Member, OrgRole::Admin).await }
    });

    tokio::time::sleep(Duration::from_millis(750)).await;
    assert!(
        !racing.is_finished(),
        "the demotion did not wait for the rows the guard takes"
    );

    sqlx::query("UPDATE organization_members SET role = 'admin' WHERE organization_id = $1 AND user_id = $2")
        .bind(organization)
        .bind(target)
        .execute(&mut *holder)
        .await
        .expect("the target is promoted");
    holder.commit().await.expect("the promotion lands");

    let outcome = tokio::time::timeout(Duration::from_secs(10), racing)
        .await
        .expect("the demotion stops waiting")
        .expect("the demotion task ran")
        .expect("the demotion is answered");

    assert!(
        matches!(outcome, RoleChange::Forbidden),
        "an admin demoted an admin, because the rank was read before the \
         promotion landed: {outcome:?}"
    );

    let role: String = sqlx::query_scalar(
        "SELECT role FROM organization_members WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization)
    .bind(target)
    .fetch_one(&pool)
    .await
    .expect("the target is readable");
    assert_eq!(role, "admin", "the target was demoted anyway");
}
