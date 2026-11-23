use sqlx::PgPool;
use uuid::Uuid;
use zone_server::auth::{Grant, Permission, Scope};
use zone_server::db::custom_roles::{self, Creation, Definition, Deletion, Summary};
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::db::scim::{
    Admission, Applied, Criterion, GroupMember, Link, Mapping, NewError, NewToken, Trigger, errors,
    group_members, groups, roles, tokens, users,
};
use zone_server::services::scim::secret;

use crate::common::scim::Fixture;

const PREFIX: &str = "scim_store";
const UNIQUE_VIOLATION: &str = "23505";
const CHECK_VIOLATION: &str = "23514";

async fn account(pool: &PgPool) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, display_name) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(format!("{}@acme.test", Uuid::new_v4()))
    .bind(format!("unusable-{}", Uuid::new_v4()))
    .bind("Ada Lovelace")
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn join(pool: &PgPool, organization: Uuid, user: Uuid, role: OrgRole) {
    organization_members::add_member(pool, organization, user, role, None)
        .await
        .unwrap();
}

async fn member(fixture: &Fixture, role: OrgRole) -> Uuid {
    let user = account(fixture.pool()).await;
    join(fixture.pool(), fixture.organization, user, role).await;
    user
}

async fn deactivate(pool: &PgPool, organization: Uuid, user: Uuid) {
    organization_members::remove_member(pool, organization, user)
        .await
        .unwrap();
}

async fn link(pool: &PgPool, organization: Uuid, user: Uuid, user_name: &str) -> Link {
    users::upsert(pool, &Link::new(organization, user, user_name))
        .await
        .unwrap()
}

async fn linked(fixture: &Fixture, role: OrgRole) -> Uuid {
    let user = member(fixture, role).await;
    link(
        fixture.pool(),
        fixture.organization,
        user,
        &format!("{user}@acme.test"),
    )
    .await;
    user
}

async fn custom_role(
    fixture: &Fixture,
    organization: Uuid,
    name: &str,
    scope: Scope,
    permissions: &[Permission],
) -> Summary {
    let names: Vec<String> = permissions
        .iter()
        .map(|permission| permission.name().to_owned())
        .collect();
    let grant = Grant::compose(scope, &names).unwrap();
    let definition = Definition {
        name,
        description: None,
        scope,
        grant: &grant,
    };
    match custom_roles::create(fixture.pool(), organization, &definition, fixture.owner.id)
        .await
        .unwrap()
    {
        Creation::Created(role) => Summary {
            id: role.id,
            name: role.name,
        },
        Creation::Forbidden => panic!("the creator does not hold roles:update"),
        Creation::NameTaken => panic!("{name} is already taken"),
    }
}

async fn group(fixture: &Fixture, display_name: &str) -> Uuid {
    groups::create(fixture.pool(), fixture.organization, display_name, None)
        .await
        .unwrap()
        .id
}

async fn mapped(fixture: &Fixture, display_name: &str, mapping: Mapping) -> Uuid {
    let id = group(fixture, display_name).await;
    groups::map(
        fixture.pool(),
        fixture.organization,
        id,
        &mapping,
        fixture.owner.id,
    )
    .await
    .unwrap()
    .expect("the group is mapped");
    id
}

async fn add(fixture: &Fixture, group: Uuid, users: &[Uuid]) -> Admission {
    let mut connection = fixture.pool().acquire().await.unwrap();
    group_members::add(&mut connection, fixture.organization, group, users)
        .await
        .unwrap()
}

async fn apply(fixture: &Fixture, user: Uuid, trigger: Trigger) -> Applied {
    let mut connection = fixture.pool().acquire().await.unwrap();
    roles::apply(&mut connection, fixture.organization, user, trigger)
        .await
        .unwrap()
}

async fn assign(fixture: &Fixture, user: Uuid, role: Uuid) {
    sqlx::query(
        "UPDATE organization_members SET role = 'member', custom_role_id = $3 \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(fixture.organization)
    .bind(user)
    .bind(role)
    .execute(fixture.pool())
    .await
    .unwrap();
}

async fn holding(fixture: &Fixture, user: Uuid) -> (String, Option<Uuid>) {
    sqlx::query_as(
        "SELECT role, custom_role_id FROM organization_members \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(fixture.organization)
    .bind(user)
    .fetch_one(fixture.pool())
    .await
    .unwrap()
}

fn constraint(error: &sqlx::Error) -> (Option<String>, Option<String>) {
    let database = error.as_database_error().expect("a database error");
    (
        database.code().map(|code| code.into_owned()),
        database.constraint().map(str::to_owned),
    )
}

fn unique(key: &str) -> (Option<String>, Option<String>) {
    (Some(UNIQUE_VIOLATION.to_owned()), Some(key.to_owned()))
}

async fn issue(fixture: &Fixture, organization: Uuid, name: &str) -> (String, Uuid) {
    let secret = secret::generate();
    let record = tokens::create(
        fixture.pool(),
        &NewToken {
            organization_id: organization,
            name,
            token_hash: &secret::hash(&secret),
            hint: &secret::hint(&secret),
            created_by: Some(fixture.owner.id),
        },
    )
    .await
    .unwrap();
    assert_eq!(record.name, name);
    assert_eq!(record.organization_id, organization);
    assert_eq!(record.hint, secret::hint(&secret));
    assert_eq!(record.created_by, Some(fixture.owner.id));
    assert_eq!(record.rotated_at, None);
    assert_eq!(record.last_used_at, None);
    (secret, record.id)
}

#[tokio::test]
async fn tokens_are_issued_listed_and_matched_by_hash_alone() {
    let fixture = Fixture::new(PREFIX).await;
    let other = fixture.another_organization().await;
    let (first, first_id) = issue(&fixture, fixture.organization, "Okta").await;
    let (_, second_id) = issue(&fixture, fixture.organization, "Entra").await;
    issue(&fixture, other, "Foreign").await;

    let listed = tokens::list(fixture.pool(), fixture.organization)
        .await
        .unwrap();
    assert_eq!(
        listed.iter().map(|token| token.id).collect::<Vec<_>>(),
        vec![first_id, second_id]
    );

    let active = tokens::active(fixture.pool(), fixture.organization)
        .await
        .unwrap();
    assert_eq!(active.len(), 2);
    let matched: Vec<Uuid> = active
        .iter()
        .filter(|credential| secret::matches(&credential.token_hash, &first))
        .map(|credential| credential.id)
        .collect();
    assert_eq!(matched, vec![first_id]);

    let stored: String =
        sqlx::query_scalar("SELECT string_agg(row_to_json(t)::text, ' ') FROM scim_tokens t")
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert!(!stored.contains(&first), "a plaintext secret was stored");

    let blank = tokens::create(
        fixture.pool(),
        &NewToken {
            organization_id: fixture.organization,
            name: "  ",
            token_hash: &secret::hash(&secret::generate()),
            hint: "abcd",
            created_by: None,
        },
    )
    .await
    .expect_err("a blank name is refused");
    assert_eq!(
        constraint(&blank).0.as_deref(),
        Some(CHECK_VIOLATION),
        "{blank:?}"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn rotation_replaces_the_secret_at_once_and_only_in_its_organization() {
    let fixture = Fixture::new(PREFIX).await;
    let other = fixture.another_organization().await;
    let (old, id) = issue(&fixture, fixture.organization, "Okta").await;

    let fresh = secret::generate();
    assert_eq!(
        tokens::rotate(fixture.pool(), other, id, &secret::hash(&fresh), "zzzz")
            .await
            .unwrap(),
        None,
        "another organization cannot rotate the token"
    );

    let rotated = tokens::rotate(
        fixture.pool(),
        fixture.organization,
        id,
        &secret::hash(&fresh),
        &secret::hint(&fresh),
    )
    .await
    .unwrap()
    .expect("the token is rotated");
    assert_eq!(rotated.id, id);
    assert_eq!(rotated.hint, secret::hint(&fresh));
    assert!(rotated.rotated_at.is_some());

    let active = tokens::active(fixture.pool(), fixture.organization)
        .await
        .unwrap();
    assert_eq!(active.len(), 1);
    assert!(secret::matches(&active[0].token_hash, &fresh));
    assert!(!secret::matches(&active[0].token_hash, &old));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_revoked_token_is_neither_listed_nor_active_nor_rotatable() {
    let fixture = Fixture::new(PREFIX).await;
    let other = fixture.another_organization().await;
    let (_, kept) = issue(&fixture, fixture.organization, "Okta").await;
    let (_, revoked) = issue(&fixture, fixture.organization, "Entra").await;

    assert_eq!(
        tokens::revoke(fixture.pool(), other, revoked)
            .await
            .unwrap(),
        None,
        "another organization cannot revoke the token"
    );
    let record = tokens::revoke(fixture.pool(), fixture.organization, revoked)
        .await
        .unwrap()
        .expect("the token is revoked");
    assert_eq!(record.id, revoked);

    let listed = tokens::list(fixture.pool(), fixture.organization)
        .await
        .unwrap();
    assert_eq!(
        listed.iter().map(|token| token.id).collect::<Vec<_>>(),
        vec![kept]
    );
    let active = tokens::active(fixture.pool(), fixture.organization)
        .await
        .unwrap();
    assert_eq!(
        active.iter().map(|token| token.id).collect::<Vec<_>>(),
        vec![kept]
    );
    assert_eq!(
        tokens::revoke(fixture.pool(), fixture.organization, revoked)
            .await
            .unwrap(),
        None,
        "a token is revoked once"
    );
    assert_eq!(
        tokens::rotate(
            fixture.pool(),
            fixture.organization,
            revoked,
            &secret::hash(&secret::generate()),
            "abcd",
        )
        .await
        .unwrap(),
        None,
        "a revoked token cannot be brought back by rotating it"
    );
    assert!(
        !tokens::touch(fixture.pool(), revoked).await.unwrap(),
        "a revoked token is never touched"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn touch_writes_at_most_once_a_minute_and_last_used_spans_every_token() {
    let fixture = Fixture::new(PREFIX).await;
    let (_, first) = issue(&fixture, fixture.organization, "Okta").await;
    let (_, second) = issue(&fixture, fixture.organization, "Entra").await;
    assert_eq!(
        tokens::last_used(fixture.pool(), fixture.organization)
            .await
            .unwrap(),
        None
    );

    assert!(tokens::touch(fixture.pool(), first).await.unwrap());
    assert!(
        !tokens::touch(fixture.pool(), first).await.unwrap(),
        "a second use within a minute writes nothing"
    );
    sqlx::query("UPDATE scim_tokens SET last_used_at = now() - interval '2 minutes' WHERE id = $1")
        .bind(first)
        .execute(fixture.pool())
        .await
        .unwrap();
    assert!(tokens::touch(fixture.pool(), first).await.unwrap());

    sqlx::query("UPDATE scim_tokens SET last_used_at = now() + interval '1 hour' WHERE id = $1")
        .bind(second)
        .execute(fixture.pool())
        .await
        .unwrap();
    tokens::revoke(fixture.pool(), fixture.organization, second)
        .await
        .unwrap();
    let expected: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT last_used_at FROM scim_tokens WHERE id = $1")
            .bind(second)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert_eq!(
        tokens::last_used(fixture.pool(), fixture.organization)
            .await
            .unwrap(),
        expected,
        "the latest use counts even when its token was revoked since"
    );
    let listed = tokens::list(fixture.pool(), fixture.organization)
        .await
        .unwrap();
    assert!(listed[0].last_used_at.is_some());
    fixture.cleanup().await;
}

#[tokio::test]
async fn members_cover_every_membership_with_their_link_and_groups() {
    let fixture = Fixture::new(PREFIX).await;
    let other = fixture.another_organization().await;
    let plain = member(&fixture, OrgRole::Admin).await;
    let provisioned = linked(&fixture, OrgRole::Member).await;
    let suspended = member(&fixture, OrgRole::Member).await;
    deactivate(fixture.pool(), fixture.organization, suspended).await;
    let foreign = account(fixture.pool()).await;
    join(fixture.pool(), other, foreign, OrgRole::Member).await;

    let engineering = group(&fixture, "Engineering").await;
    let design = group(&fixture, "design").await;
    add(&fixture, engineering, &[provisioned]).await;
    add(&fixture, design, &[provisioned]).await;

    let listing = users::list(fixture.pool(), fixture.organization, &[], 0, 100)
        .await
        .unwrap();
    assert_eq!(listing.total, 4);
    assert_eq!(
        listing
            .items
            .iter()
            .map(|member| member.user_id)
            .collect::<Vec<_>>(),
        vec![fixture.owner.id, plain, provisioned, suspended]
    );

    let found = users::get(fixture.pool(), fixture.organization, provisioned)
        .await
        .unwrap()
        .expect("the member is found");
    assert!(found.active);
    assert_eq!(found.role, OrgRole::Member);
    assert_eq!(found.display_name.as_deref(), Some("Ada Lovelace"));
    let link = found.link.expect("the member is linked");
    assert_eq!(link.user_name, format!("{provisioned}@acme.test"));
    assert_eq!(
        found
            .groups
            .iter()
            .map(|group| (group.id, group.display_name.as_str()))
            .collect::<Vec<_>>(),
        vec![(design, "design"), (engineering, "Engineering")]
    );
    assert!(found.updated_at >= found.created_at);

    let unlinked = users::get(fixture.pool(), fixture.organization, plain)
        .await
        .unwrap()
        .expect("an unlinked member is found");
    assert_eq!(unlinked.link, None);
    assert_eq!(unlinked.role, OrgRole::Admin);
    assert!(unlinked.groups.is_empty());

    let inactive = users::get(fixture.pool(), fixture.organization, suspended)
        .await
        .unwrap()
        .expect("an inactive member is found");
    assert!(!inactive.active);

    assert_eq!(
        users::get(fixture.pool(), fixture.organization, foreign)
            .await
            .unwrap(),
        None,
        "another organization's member is not found"
    );
    assert_eq!(
        users::get(fixture.pool(), other, provisioned)
            .await
            .unwrap(),
        None,
        "a member is not found through another organization"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn member_criteria_are_anded_and_never_reach_another_organization() {
    let fixture = Fixture::new(PREFIX).await;
    let other = fixture.another_organization().await;
    let ada = member(&fixture, OrgRole::Member).await;
    let mut ada_link = Link::new(fixture.organization, ada, "Ada@Acme.test");
    ada_link.external_id = Some("00u-ada".to_owned());
    ada_link.display_name = Some("Countess".to_owned());
    users::upsert(fixture.pool(), &ada_link).await.unwrap();
    let grace = member(&fixture, OrgRole::Member).await;
    let grace_email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(grace)
        .fetch_one(fixture.pool())
        .await
        .unwrap();
    let foreign = account(fixture.pool()).await;
    join(fixture.pool(), other, foreign, OrgRole::Member).await;
    let mut foreign_link = Link::new(other, foreign, "ada@acme.test");
    foreign_link.external_id = Some("00u-ada".to_owned());
    users::upsert(fixture.pool(), &foreign_link).await.unwrap();

    let matching = |criteria: Vec<Criterion>| {
        let pool = fixture.pool().clone();
        let organization = fixture.organization;
        async move {
            users::list(&pool, organization, &criteria, 0, 100)
                .await
                .unwrap()
                .items
                .into_iter()
                .map(|member| member.user_id)
                .collect::<Vec<_>>()
        }
    };

    assert_eq!(
        matching(vec![Criterion::UserName("ADA@acme.TEST".to_owned())]).await,
        vec![ada]
    );
    assert_eq!(
        matching(vec![Criterion::UserName(grace_email.to_uppercase())]).await,
        vec![grace],
        "an unlinked member is found by their email"
    );
    assert_eq!(
        matching(vec![Criterion::ExternalId("00u-ada".to_owned())]).await,
        vec![ada]
    );
    assert_eq!(
        matching(vec![Criterion::ExternalId("00U-ADA".to_owned())]).await,
        Vec::<Uuid>::new(),
        "an external id is matched exactly"
    );
    assert_eq!(
        matching(vec![Criterion::DisplayName("countess".to_owned())]).await,
        vec![ada]
    );
    assert_eq!(
        matching(vec![Criterion::DisplayName("ada lovelace".to_owned())]).await,
        vec![grace],
        "an unlinked member is found by their account name"
    );
    assert_eq!(matching(vec![Criterion::Id(grace)]).await, vec![grace]);
    assert_eq!(
        matching(vec![
            Criterion::UserName("ada@acme.test".to_owned()),
            Criterion::ExternalId("00u-ada".to_owned()),
        ])
        .await,
        vec![ada]
    );
    assert_eq!(
        matching(vec![
            Criterion::UserName("ada@acme.test".to_owned()),
            Criterion::Id(grace),
        ])
        .await,
        Vec::<Uuid>::new()
    );
    assert_eq!(
        matching(vec![Criterion::Member(ada)]).await,
        Vec::<Uuid>::new(),
        "a user has no members"
    );
    assert_eq!(
        matching(vec![Criterion::Id(foreign)]).await,
        Vec::<Uuid>::new()
    );
    assert_eq!(
        matching(vec![Criterion::UserName("x' OR 1=1 --".to_owned())]).await,
        Vec::<Uuid>::new()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn member_pages_count_every_match() {
    let fixture = Fixture::new(PREFIX).await;
    let mut expected = vec![fixture.owner.id];
    for _ in 0..4 {
        expected.push(member(&fixture, OrgRole::Member).await);
    }

    let page = users::list(fixture.pool(), fixture.organization, &[], 1, 2)
        .await
        .unwrap();
    assert_eq!(page.total, 5);
    assert_eq!(
        page.items
            .iter()
            .map(|member| member.user_id)
            .collect::<Vec<_>>(),
        expected[1..3].to_vec()
    );

    let empty = users::list(fixture.pool(), fixture.organization, &[], 0, 0)
        .await
        .unwrap();
    assert_eq!(empty.total, 5);
    assert!(empty.items.is_empty());

    let beyond = users::list(fixture.pool(), fixture.organization, &[], 10, 5)
        .await
        .unwrap();
    assert_eq!(beyond.total, 5);
    assert!(beyond.items.is_empty());

    let clamped = users::list(fixture.pool(), fixture.organization, &[], -3, 2)
        .await
        .unwrap();
    assert_eq!(clamped.items.len(), 2);
    fixture.cleanup().await;
}

#[tokio::test]
async fn upsert_updates_in_place_and_keeps_the_provisioned_password() {
    let fixture = Fixture::new(PREFIX).await;
    let other = fixture.another_organization().await;
    let user = member(&fixture, OrgRole::Member).await;

    let mut created = Link::new(fixture.organization, user, "ada@acme.test");
    created.external_id = Some("00u-1".to_owned());
    created.given_name = Some("Ada".to_owned());
    created.family_name = Some("Lovelace".to_owned());
    created.email = Some("ada@acme.test".to_owned());
    created.provisioned_password = Some("unusable".to_owned());
    let stored = users::upsert(fixture.pool(), &created).await.unwrap();
    assert_eq!(stored.user_name, "ada@acme.test");
    assert_eq!(stored.provisioned_password.as_deref(), Some("unusable"));

    let changed = Link {
        user_name: "ada.lovelace@acme.test".to_owned(),
        external_id: None,
        given_name: Some("Augusta".to_owned()),
        provisioned_password: None,
        ..stored.clone()
    };
    let updated = users::upsert(fixture.pool(), &changed).await.unwrap();
    assert_eq!(updated.user_name, "ada.lovelace@acme.test");
    assert_eq!(updated.external_id, None);
    assert_eq!(updated.given_name.as_deref(), Some("Augusta"));
    assert_eq!(updated.family_name.as_deref(), Some("Lovelace"));
    assert_eq!(
        updated.provisioned_password.as_deref(),
        Some("unusable"),
        "an update never forgets that SCIM created the account"
    );
    assert_eq!(updated.created_at, stored.created_at);
    assert!(updated.updated_at >= stored.updated_at);

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scim_users WHERE user_id = $1")
        .bind(user)
        .fetch_one(fixture.pool())
        .await
        .unwrap();
    assert_eq!(rows, 1);

    let foreign = account(fixture.pool()).await;
    join(fixture.pool(), other, foreign, OrgRole::Member).await;
    users::upsert(
        fixture.pool(),
        &Link::new(other, foreign, "ADA.LOVELACE@acme.test"),
    )
    .await
    .expect("another organization may use the same user name");
    fixture.cleanup().await;
}

#[tokio::test]
async fn user_names_are_unique_ignoring_case_and_external_ids_exactly() {
    let fixture = Fixture::new(PREFIX).await;
    let first = member(&fixture, OrgRole::Member).await;
    let second = member(&fixture, OrgRole::Member).await;
    let mut taken = Link::new(fixture.organization, first, "ada@acme.test");
    taken.external_id = Some("00u-1".to_owned());
    users::upsert(fixture.pool(), &taken).await.unwrap();

    let clash = users::upsert(
        fixture.pool(),
        &Link::new(fixture.organization, second, "ADA@acme.test"),
    )
    .await
    .expect_err("the user name is taken");
    assert_eq!(constraint(&clash), unique(users::USER_NAME_KEY));

    let mut same_external = Link::new(fixture.organization, second, "grace@acme.test");
    same_external.external_id = Some("00u-1".to_owned());
    let clash = users::upsert(fixture.pool(), &same_external)
        .await
        .expect_err("the external id is taken");
    assert_eq!(constraint(&clash), unique(users::EXTERNAL_ID_KEY));

    same_external.external_id = Some("00U-1".to_owned());
    users::upsert(fixture.pool(), &same_external)
        .await
        .expect("external ids differing in case are distinct");
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_account_is_pristine_only_with_the_scim_password_no_sign_in_and_no_identity() {
    let fixture = Fixture::new(PREFIX).await;
    let user = member(&fixture, OrgRole::Member).await;
    assert!(
        !users::pristine(fixture.pool(), user).await.unwrap(),
        "an account SCIM never touched is not pristine"
    );

    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(fixture.pool())
        .await
        .unwrap();
    let mut created = Link::new(fixture.organization, user, "ada@acme.test");
    created.provisioned_password = Some(hash.clone());
    users::upsert(fixture.pool(), &created).await.unwrap();
    assert!(users::pristine(fixture.pool(), user).await.unwrap());

    let set_password = |password: String| {
        let pool = fixture.pool().clone();
        async move {
            sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1")
                .bind(user)
                .bind(password)
                .execute(&pool)
                .await
                .unwrap();
        }
    };
    set_password("chosen-by-the-user".to_owned()).await;
    assert!(
        !users::pristine(fixture.pool(), user).await.unwrap(),
        "an account whose password was reset is not pristine"
    );
    set_password(hash).await;
    assert!(users::pristine(fixture.pool(), user).await.unwrap());

    sqlx::query("UPDATE users SET last_login_at = now() WHERE id = $1")
        .bind(user)
        .execute(fixture.pool())
        .await
        .unwrap();
    assert!(
        !users::pristine(fixture.pool(), user).await.unwrap(),
        "an account that has signed in is not pristine"
    );
    sqlx::query("UPDATE users SET last_login_at = NULL WHERE id = $1")
        .bind(user)
        .execute(fixture.pool())
        .await
        .unwrap();
    assert!(users::pristine(fixture.pool(), user).await.unwrap());

    let provider: Uuid = sqlx::query_scalar(
        "INSERT INTO sso_providers (protocol, display_name, issuer, client_id, client_secret) \
         VALUES ('oidc', 'Okta', 'https://acme.okta.test', 'client', 'ENC[v1:sealed]') \
         RETURNING id",
    )
    .fetch_one(fixture.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sso_identities (provider_id, user_id, subject, email) \
         VALUES ($1, $2, 'subject', 'ada@acme.test')",
    )
    .bind(provider)
    .bind(user)
    .execute(fixture.pool())
    .await
    .unwrap();
    assert!(
        !users::pristine(fixture.pool(), user).await.unwrap(),
        "an account with a single sign-on identity is not pristine"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn sole_organization_counts_every_other_membership_active_or_not() {
    let fixture = Fixture::new(PREFIX).await;
    let other = fixture.another_organization().await;
    let user = member(&fixture, OrgRole::Member).await;
    assert!(
        users::sole_organization(fixture.pool(), fixture.organization, user)
            .await
            .unwrap()
    );

    join(fixture.pool(), other, user, OrgRole::Member).await;
    assert!(
        !users::sole_organization(fixture.pool(), fixture.organization, user)
            .await
            .unwrap()
    );
    deactivate(fixture.pool(), other, user).await;
    assert!(
        !users::sole_organization(fixture.pool(), fixture.organization, user)
            .await
            .unwrap(),
        "an inactive membership elsewhere still counts"
    );

    let registered = fixture.member(OrgRole::Member).await;
    assert!(
        !users::sole_organization(fixture.pool(), fixture.organization, registered.id)
            .await
            .unwrap(),
        "a registered account owns an organization of its own"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn groups_are_created_read_renamed_and_kept_to_their_organization() {
    let fixture = Fixture::new(PREFIX).await;
    let other = fixture.another_organization().await;
    let created = groups::create(
        fixture.pool(),
        fixture.organization,
        "Engineering",
        Some("00g-1"),
    )
    .await
    .unwrap();
    assert_eq!(created.organization_id, fixture.organization);
    assert_eq!(created.display_name, "Engineering");
    assert_eq!(created.external_id.as_deref(), Some("00g-1"));
    assert_eq!(created.mapping, Mapping::Unmapped);
    assert_eq!(created.members, 0);

    assert_eq!(
        groups::get(fixture.pool(), fixture.organization, created.id)
            .await
            .unwrap(),
        Some(created.clone())
    );
    assert_eq!(
        groups::get(fixture.pool(), other, created.id)
            .await
            .unwrap(),
        None
    );

    let renamed = groups::update(
        fixture.pool(),
        fixture.organization,
        created.id,
        Some("Platform"),
        None,
    )
    .await
    .unwrap()
    .expect("the group is renamed");
    assert_eq!(renamed.display_name, "Platform");
    assert_eq!(renamed.external_id.as_deref(), Some("00g-1"));
    assert!(renamed.updated_at >= created.updated_at);

    let cleared = groups::update(
        fixture.pool(),
        fixture.organization,
        created.id,
        None,
        Some(None),
    )
    .await
    .unwrap()
    .expect("the external id is cleared");
    assert_eq!(cleared.display_name, "Platform");
    assert_eq!(cleared.external_id, None);

    assert_eq!(
        groups::update(fixture.pool(), other, created.id, Some("Stolen"), None)
            .await
            .unwrap(),
        None,
        "another organization cannot rename the group"
    );
    assert_eq!(
        groups::delete(fixture.pool(), other, created.id)
            .await
            .unwrap(),
        None,
        "another organization cannot delete the group"
    );
    assert!(
        groups::get(fixture.pool(), fixture.organization, created.id)
            .await
            .unwrap()
            .is_some()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn group_names_and_external_ids_are_unique_per_organization() {
    let fixture = Fixture::new(PREFIX).await;
    let other = fixture.another_organization().await;
    groups::create(
        fixture.pool(),
        fixture.organization,
        "Engineering",
        Some("00g-1"),
    )
    .await
    .unwrap();

    let clash = groups::create(fixture.pool(), fixture.organization, " engineering ", None)
        .await
        .expect_err("the name is taken ignoring case and spaces");
    assert_eq!(constraint(&clash), unique(groups::DISPLAY_NAME_KEY));

    let clash = groups::create(
        fixture.pool(),
        fixture.organization,
        "Design",
        Some("00g-1"),
    )
    .await
    .expect_err("the external id is taken");
    assert_eq!(constraint(&clash), unique(groups::EXTERNAL_ID_KEY));

    let blank = groups::create(fixture.pool(), fixture.organization, "   ", None)
        .await
        .expect_err("a blank name is refused");
    assert_eq!(constraint(&blank).0.as_deref(), Some(CHECK_VIOLATION));

    groups::create(fixture.pool(), other, "Engineering", Some("00g-1"))
        .await
        .expect("another organization may reuse both");
    fixture.cleanup().await;
}

#[tokio::test]
async fn group_criteria_and_pages() {
    let fixture = Fixture::new(PREFIX).await;
    let other = fixture.another_organization().await;
    let user = member(&fixture, OrgRole::Member).await;
    let engineering = groups::create(
        fixture.pool(),
        fixture.organization,
        "Engineering",
        Some("00g-1"),
    )
    .await
    .unwrap()
    .id;
    let design = group(&fixture, "Design").await;
    let support = group(&fixture, "Support").await;
    groups::create(fixture.pool(), other, "Engineering", Some("00g-1"))
        .await
        .unwrap();
    add(&fixture, design, &[user]).await;

    let matching = |criteria: Vec<Criterion>, offset: i64, limit: i64| {
        let pool = fixture.pool().clone();
        let organization = fixture.organization;
        async move {
            let listing = groups::list(&pool, organization, &criteria, offset, limit)
                .await
                .unwrap();
            (
                listing.total,
                listing
                    .items
                    .into_iter()
                    .map(|group| group.id)
                    .collect::<Vec<_>>(),
            )
        }
    };

    assert_eq!(
        matching(vec![], 0, 100).await,
        (3, vec![engineering, design, support])
    );
    assert_eq!(matching(vec![], 1, 1).await, (3, vec![design]));
    assert_eq!(matching(vec![], 0, 0).await, (3, vec![]));
    assert_eq!(
        matching(
            vec![Criterion::DisplayName(" ENGINEERING".to_owned())],
            0,
            100
        )
        .await,
        (1, vec![engineering])
    );
    assert_eq!(
        matching(vec![Criterion::ExternalId("00g-1".to_owned())], 0, 100).await,
        (1, vec![engineering])
    );
    assert_eq!(
        matching(vec![Criterion::Member(user)], 0, 100).await,
        (1, vec![design])
    );
    assert_eq!(
        matching(vec![Criterion::Id(design), Criterion::Member(user)], 0, 100).await,
        (1, vec![design])
    );
    assert_eq!(
        matching(
            vec![Criterion::Id(support), Criterion::Member(user)],
            0,
            100
        )
        .await,
        (0, vec![])
    );
    assert_eq!(
        matching(vec![Criterion::UserName("Design".to_owned())], 0, 100).await,
        (0, vec![]),
        "a group has no user name"
    );

    let counted = groups::get(fixture.pool(), fixture.organization, design)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(counted.members, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn deleting_a_group_returns_its_former_members() {
    let fixture = Fixture::new(PREFIX).await;
    let first = member(&fixture, OrgRole::Member).await;
    let second = member(&fixture, OrgRole::Member).await;
    let engineering = group(&fixture, "Engineering").await;
    add(&fixture, engineering, &[first, second]).await;

    let former = groups::delete(fixture.pool(), fixture.organization, engineering)
        .await
        .unwrap()
        .expect("the group is deleted");
    let mut expected = vec![first, second];
    expected.sort();
    assert_eq!(former, expected);
    assert_eq!(
        groups::get(fixture.pool(), fixture.organization, engineering)
            .await
            .unwrap(),
        None
    );
    let left: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM scim_group_members WHERE group_id = $1")
            .bind(engineering)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert_eq!(left, 0);
    assert_eq!(
        groups::delete(fixture.pool(), fixture.organization, engineering)
            .await
            .unwrap(),
        None
    );

    let empty = group(&fixture, "Empty").await;
    assert_eq!(
        groups::delete(fixture.pool(), fixture.organization, empty)
            .await
            .unwrap(),
        Some(vec![])
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_group_maps_to_admin_member_or_an_organization_custom_role_and_never_owner() {
    let fixture = Fixture::new(PREFIX).await;
    let other = fixture.another_organization().await;
    let engineering = group(&fixture, "Engineering").await;
    let map = |mapping: Mapping| {
        let pool = fixture.pool().clone();
        let organization = fixture.organization;
        let owner = fixture.owner.id;
        async move { groups::map(&pool, organization, engineering, &mapping, owner).await }
    };

    let admin = map(Mapping::BuiltIn(OrgRole::Admin))
        .await
        .unwrap()
        .expect("the group is mapped");
    assert_eq!(admin.mapping, Mapping::BuiltIn(OrgRole::Admin));
    let mapped_by: Option<Uuid> =
        sqlx::query_scalar("SELECT mapped_by FROM scim_groups WHERE id = $1")
            .bind(engineering)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert_eq!(mapped_by, Some(fixture.owner.id));

    let auditor = custom_role(
        &fixture,
        fixture.organization,
        "Auditor",
        Scope::Organization,
        &[Permission::AuditLogsRead],
    )
    .await;
    let custom = map(Mapping::Custom(auditor.clone()))
        .await
        .unwrap()
        .expect("the group is mapped to a custom role");
    assert_eq!(custom.mapping, Mapping::Custom(auditor.clone()));

    let workspace_role = custom_role(
        &fixture,
        fixture.organization,
        "Writer",
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;
    assert_eq!(
        map(Mapping::Custom(workspace_role)).await.unwrap(),
        None,
        "a workspace role cannot be mapped"
    );
    let foreign_role = custom_role(
        &fixture,
        other,
        "Foreign",
        Scope::Organization,
        &[Permission::AuditLogsRead],
    )
    .await;
    assert_eq!(
        map(Mapping::Custom(foreign_role)).await.unwrap(),
        None,
        "another organization's role cannot be mapped"
    );
    assert_eq!(
        groups::get(fixture.pool(), fixture.organization, engineering)
            .await
            .unwrap()
            .unwrap()
            .mapping,
        Mapping::Custom(auditor.clone()),
        "a refused mapping changes nothing"
    );

    let owner = map(Mapping::BuiltIn(OrgRole::Owner))
        .await
        .expect_err("a group never maps to owner");
    assert_eq!(constraint(&owner).0.as_deref(), Some(CHECK_VIOLATION));

    assert_eq!(
        groups::map(
            fixture.pool(),
            other,
            engineering,
            &Mapping::BuiltIn(OrgRole::Admin),
            fixture.owner.id,
        )
        .await
        .unwrap(),
        None,
        "another organization cannot map the group"
    );

    let unmapped = map(Mapping::Unmapped)
        .await
        .unwrap()
        .expect("the group is unmapped");
    assert_eq!(unmapped.mapping, Mapping::Unmapped);
    fixture.cleanup().await;
}

#[tokio::test]
async fn deleting_a_mapped_custom_role_unmaps_its_groups() {
    let fixture = Fixture::new(PREFIX).await;
    let auditor = custom_role(
        &fixture,
        fixture.organization,
        "Auditor",
        Scope::Organization,
        &[Permission::AuditLogsRead],
    )
    .await;
    let auditors = mapped(&fixture, "Auditors", Mapping::Custom(auditor.clone())).await;

    let deletion = custom_roles::delete(
        fixture.pool(),
        fixture.organization,
        auditor.id,
        fixture.owner.id,
    )
    .await
    .unwrap();
    let Deletion::Deleted { unmapped, .. } = deletion else {
        panic!("the role is deleted: {deletion:?}");
    };
    assert_eq!(
        unmapped
            .iter()
            .map(|group| (group.id, group.mapping.clone()))
            .collect::<Vec<_>>(),
        [(auditors, Mapping::Unmapped)],
        "the deletion names every group it unmapped"
    );
    assert_eq!(
        groups::get(fixture.pool(), fixture.organization, auditors)
            .await
            .unwrap()
            .unwrap()
            .mapping,
        Mapping::Unmapped
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn group_members_admit_any_membership_and_refuse_strangers_whole() {
    let fixture = Fixture::new(PREFIX).await;
    let other = fixture.another_organization().await;
    let active = member(&fixture, OrgRole::Member).await;
    let inactive = member(&fixture, OrgRole::Member).await;
    deactivate(fixture.pool(), fixture.organization, inactive).await;
    let foreign = account(fixture.pool()).await;
    join(fixture.pool(), other, foreign, OrgRole::Member).await;
    let nobody = account(fixture.pool()).await;
    let engineering = group(&fixture, "Engineering").await;

    let mut both = vec![active, inactive];
    both.sort();
    assert_eq!(
        add(&fixture, engineering, &[active, inactive, active]).await,
        Admission::Admitted(both.clone())
    );
    assert_eq!(
        add(&fixture, engineering, &[active]).await,
        Admission::Admitted(vec![]),
        "a member already in the group is not admitted again"
    );

    let mut strangers = vec![foreign, nobody];
    strangers.sort();
    assert_eq!(
        add(&fixture, engineering, &[foreign, active, nobody]).await,
        Admission::Strangers(strangers.clone()),
        "a member of another organization is a stranger here"
    );
    let mut connection = fixture.pool().acquire().await.unwrap();
    assert_eq!(
        group_members::replace(
            &mut connection,
            fixture.organization,
            engineering,
            &[foreign]
        )
        .await
        .unwrap(),
        Admission::Strangers(vec![foreign])
    );
    drop(connection);

    let listed = group_members::list(fixture.pool(), engineering)
        .await
        .unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|member| member.user_id)
            .collect::<std::collections::BTreeSet<_>>(),
        both.iter().copied().collect(),
        "a refused admission writes nothing"
    );
    assert!(listed.iter().all(|member| member.display == "Ada Lovelace"));

    let foreign_group = groups::create(fixture.pool(), other, "Foreign", None)
        .await
        .unwrap()
        .id;
    let mut connection = fixture.pool().acquire().await.unwrap();
    let error = group_members::add(
        &mut connection,
        fixture.organization,
        foreign_group,
        &[active],
    )
    .await
    .expect_err("another organization's group is not found");
    assert!(matches!(error, sqlx::Error::RowNotFound), "{error:?}");
    drop(connection);
    fixture.cleanup().await;
}

#[tokio::test]
async fn group_members_are_listed_by_their_scim_name_first() {
    let fixture = Fixture::new(PREFIX).await;
    let provisioned = member(&fixture, OrgRole::Member).await;
    let mut named = Link::new(fixture.organization, provisioned, "ada@acme.test");
    named.display_name = Some("Countess".to_owned());
    users::upsert(fixture.pool(), &named).await.unwrap();
    let engineering = group(&fixture, "Engineering").await;
    add(&fixture, engineering, &[provisioned]).await;

    assert_eq!(
        group_members::list(fixture.pool(), engineering)
            .await
            .unwrap(),
        vec![GroupMember {
            user_id: provisioned,
            display: "Countess".to_owned(),
        }]
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn group_members_are_removed_and_replaced_naming_everyone_affected() {
    let fixture = Fixture::new(PREFIX).await;
    let first = member(&fixture, OrgRole::Member).await;
    let second = member(&fixture, OrgRole::Member).await;
    let third = member(&fixture, OrgRole::Member).await;
    let engineering = group(&fixture, "Engineering").await;
    add(&fixture, engineering, &[first, second]).await;

    let mut connection = fixture.pool().acquire().await.unwrap();
    assert_eq!(
        group_members::remove(
            &mut connection,
            fixture.organization,
            engineering,
            &[second, third]
        )
        .await
        .unwrap(),
        vec![second],
        "only a member of the group is removed"
    );

    let mut affected = vec![first, second, third];
    affected.sort();
    assert_eq!(
        group_members::replace(
            &mut connection,
            fixture.organization,
            engineering,
            &[second, third]
        )
        .await
        .unwrap(),
        Admission::Admitted(affected)
    );
    let mut expected = vec![second, third];
    expected.sort();
    let mut listed: Vec<Uuid> = group_members::list(&mut *connection, engineering)
        .await
        .unwrap()
        .into_iter()
        .map(|member| member.user_id)
        .collect();
    listed.sort();
    assert_eq!(listed, expected);

    assert_eq!(
        group_members::replace(&mut connection, fixture.organization, engineering, &[])
            .await
            .unwrap(),
        Admission::Admitted(expected),
        "replacing with nobody empties the group"
    );
    drop(connection);
    fixture.cleanup().await;
}

#[tokio::test]
async fn apply_skips_the_unlinked_the_inactive_owners_and_strangers() {
    let fixture = Fixture::new(PREFIX).await;
    let admins = mapped(&fixture, "Admins", Mapping::BuiltIn(OrgRole::Admin)).await;

    let unlinked = member(&fixture, OrgRole::Member).await;
    let inactive = linked(&fixture, OrgRole::Member).await;
    let owner = linked(&fixture, OrgRole::Owner).await;
    add(&fixture, admins, &[unlinked, inactive, owner]).await;
    deactivate(fixture.pool(), fixture.organization, inactive).await;
    let stranger = account(fixture.pool()).await;

    for trigger in [Trigger::Membership, Trigger::Mapping, Trigger::Reactivation] {
        for user in [unlinked, inactive, owner, stranger] {
            assert_eq!(apply(&fixture, user, trigger).await, Applied::Skipped);
        }
    }
    assert_eq!(
        holding(&fixture, unlinked).await,
        ("member".to_owned(), None)
    );
    assert_eq!(holding(&fixture, owner).await, ("owner".to_owned(), None));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_mapped_group_assigns_its_role_and_reapplying_changes_nothing() {
    let fixture = Fixture::new(PREFIX).await;
    let admins = mapped(&fixture, "Admins", Mapping::BuiltIn(OrgRole::Admin)).await;
    let user = linked(&fixture, OrgRole::Member).await;
    add(&fixture, admins, &[user]).await;

    match apply(&fixture, user, Trigger::Membership).await {
        Applied::Changed { from, to } => {
            assert_eq!(from.role, OrgRole::Member);
            assert_eq!(to.role, OrgRole::Admin);
            assert_eq!(to.custom_role, None);
            assert_eq!(to.grant, Grant::organization(OrgRole::Admin));
        }
        other => panic!("expected a change, got {other:?}"),
    }
    assert_eq!(holding(&fixture, user).await, ("admin".to_owned(), None));
    assert_eq!(
        apply(&fixture, user, Trigger::Membership).await,
        Applied::Unchanged
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_custom_mapping_pins_the_built_in_role_to_member() {
    let fixture = Fixture::new(PREFIX).await;
    let auditor = custom_role(
        &fixture,
        fixture.organization,
        "Auditor",
        Scope::Organization,
        &[Permission::AuditLogsRead],
    )
    .await;
    let auditors = mapped(&fixture, "Auditors", Mapping::Custom(auditor.clone())).await;
    let user = linked(&fixture, OrgRole::Admin).await;
    add(&fixture, auditors, &[user]).await;

    match apply(&fixture, user, Trigger::Membership).await {
        Applied::Changed { from, to } => {
            assert_eq!(from.role, OrgRole::Admin);
            assert_eq!(from.custom_role, None);
            assert_eq!(to.role, OrgRole::Member);
            assert_eq!(to.custom_role, Some(auditor.clone()));
            assert!(to.grant.allows(Permission::AuditLogsRead));
        }
        other => panic!("expected a change, got {other:?}"),
    }
    assert_eq!(
        holding(&fixture, user).await,
        ("member".to_owned(), Some(auditor.id))
    );
    let membership = organization_members::membership(fixture.pool(), fixture.organization, user)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(membership.custom_role, Some(auditor));

    let admins = mapped(&fixture, "Admins", Mapping::BuiltIn(OrgRole::Admin)).await;
    add(&fixture, admins, &[user]).await;
    match apply(&fixture, user, Trigger::Membership).await {
        Applied::Changed { from, to } => {
            assert!(from.custom_role.is_some());
            assert_eq!(to.role, OrgRole::Admin);
            assert_eq!(to.custom_role, None);
        }
        other => panic!("expected admin to win, got {other:?}"),
    }
    assert_eq!(holding(&fixture, user).await, ("admin".to_owned(), None));
    fixture.cleanup().await;
}

#[tokio::test]
async fn between_custom_roles_the_larger_grant_wins_and_a_tie_falls_to_the_name() {
    let fixture = Fixture::new(PREFIX).await;
    let narrow = custom_role(
        &fixture,
        fixture.organization,
        "Narrow",
        Scope::Organization,
        &[Permission::AuditLogsRead],
    )
    .await;
    let wide = custom_role(
        &fixture,
        fixture.organization,
        "Wide",
        Scope::Organization,
        &[Permission::AuditLogsRead, Permission::DevicesRead],
    )
    .await;
    let beta = custom_role(
        &fixture,
        fixture.organization,
        "beta",
        Scope::Organization,
        &[Permission::DevicesRead],
    )
    .await;
    let user = linked(&fixture, OrgRole::Member).await;
    for (name, role) in [("Narrow", &narrow), ("Wide", &wide), ("Beta", &beta)] {
        let id = mapped(&fixture, name, Mapping::Custom(role.clone())).await;
        add(&fixture, id, &[user]).await;
    }
    apply(&fixture, user, Trigger::Membership).await;
    assert_eq!(
        holding(&fixture, user).await,
        ("member".to_owned(), Some(wide.id))
    );

    let tied = linked(&fixture, OrgRole::Member).await;
    for name in ["Narrow", "Beta"] {
        let id = groups::list(
            fixture.pool(),
            fixture.organization,
            &[Criterion::DisplayName(name.to_owned())],
            0,
            1,
        )
        .await
        .unwrap()
        .items[0]
            .id;
        add(&fixture, id, &[tied]).await;
    }
    apply(&fixture, tied, Trigger::Membership).await;
    assert_eq!(
        holding(&fixture, tied).await,
        ("member".to_owned(), Some(beta.id)),
        "beta sorts before Narrow ignoring case"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn outside_every_mapped_group_a_member_falls_back_once_anything_is_mapped() {
    let fixture = Fixture::new(PREFIX).await;
    let auditor = custom_role(
        &fixture,
        fixture.organization,
        "Auditor",
        Scope::Organization,
        &[Permission::AuditLogsRead],
    )
    .await;
    let restored = linked(&fixture, OrgRole::Member).await;
    assign(&fixture, restored, auditor.id).await;
    let promoted = linked(&fixture, OrgRole::Admin).await;
    let unmapped = group(&fixture, "Unmapped").await;
    add(&fixture, unmapped, &[restored, promoted]).await;

    for trigger in [Trigger::Membership, Trigger::Reactivation] {
        for user in [restored, promoted] {
            assert_eq!(
                apply(&fixture, user, trigger).await,
                Applied::Skipped,
                "nothing is mapped, so {trigger:?} keeps what the member holds"
            );
        }
    }
    assert_eq!(
        holding(&fixture, restored).await,
        ("member".to_owned(), Some(auditor.id))
    );

    mapped(&fixture, "Admins", Mapping::BuiltIn(OrgRole::Admin)).await;
    match apply(&fixture, restored, Trigger::Reactivation).await {
        Applied::Changed { from, to } => {
            assert_eq!(from.custom_role, Some(auditor.clone()));
            assert_eq!(to.role, OrgRole::Member);
            assert_eq!(to.custom_role, None);
        }
        other => panic!("a reactivated member in no mapped group falls back, got {other:?}"),
    }
    assert_eq!(
        apply(&fixture, promoted, Trigger::Membership).await,
        apply_expected_member_change(OrgRole::Admin)
    );
    assert_eq!(
        holding(&fixture, promoted).await,
        ("member".to_owned(), None)
    );
    fixture.cleanup().await;
}

fn apply_expected_member_change(from: OrgRole) -> Applied {
    Applied::Changed {
        from: custom_roles::Membership {
            role: from,
            custom_role: None,
            grant: Grant::organization(from),
        },
        to: custom_roles::Membership {
            role: OrgRole::Member,
            custom_role: None,
            grant: Grant::organization(OrgRole::Member),
        },
    }
}

#[tokio::test]
async fn removing_the_last_mapping_drops_its_members_to_member() {
    let fixture = Fixture::new(PREFIX).await;
    let admins = mapped(&fixture, "Admins", Mapping::BuiltIn(OrgRole::Admin)).await;
    let first = linked(&fixture, OrgRole::Member).await;
    let second = linked(&fixture, OrgRole::Member).await;
    let owner = linked(&fixture, OrgRole::Owner).await;
    add(&fixture, admins, &[first, second, owner]).await;

    let outcomes = roles::apply_group(
        &mut fixture.pool().acquire().await.unwrap(),
        fixture.organization,
        admins,
        Trigger::Mapping,
    )
    .await
    .unwrap();
    assert_eq!(outcomes.len(), 3);
    for outcome in &outcomes {
        if outcome.user_id == owner {
            assert_eq!(outcome.applied, Applied::Skipped);
        } else {
            assert!(
                matches!(&outcome.applied, Applied::Changed { to, .. } if to.role == OrgRole::Admin),
                "{outcome:?}"
            );
        }
    }

    groups::map(
        fixture.pool(),
        fixture.organization,
        admins,
        &Mapping::Unmapped,
        fixture.owner.id,
    )
    .await
    .unwrap();
    let outcomes = roles::apply_group(
        &mut fixture.pool().acquire().await.unwrap(),
        fixture.organization,
        admins,
        Trigger::Mapping,
    )
    .await
    .unwrap();
    for outcome in outcomes.iter().filter(|outcome| outcome.user_id != owner) {
        assert_eq!(
            outcome.applied,
            apply_expected_member_change(OrgRole::Admin),
            "{outcome:?}"
        );
    }
    assert_eq!(holding(&fixture, first).await, ("member".to_owned(), None));
    assert_eq!(holding(&fixture, owner).await, ("owner".to_owned(), None));
    fixture.cleanup().await;
}

#[tokio::test]
async fn deleting_a_mapped_group_drops_its_former_members_to_member() {
    let fixture = Fixture::new(PREFIX).await;
    let admins = mapped(&fixture, "Admins", Mapping::BuiltIn(OrgRole::Admin)).await;
    let user = linked(&fixture, OrgRole::Member).await;
    let bystander = linked(&fixture, OrgRole::Admin).await;
    add(&fixture, admins, &[user]).await;
    roles::apply_users(
        &mut fixture.pool().acquire().await.unwrap(),
        fixture.organization,
        &[user],
        Trigger::Membership,
    )
    .await
    .unwrap();
    assert_eq!(holding(&fixture, user).await, ("admin".to_owned(), None));

    let former = groups::delete(fixture.pool(), fixture.organization, admins)
        .await
        .unwrap()
        .unwrap();
    let outcomes = roles::apply_users(
        &mut fixture.pool().acquire().await.unwrap(),
        fixture.organization,
        &[former.clone(), former].concat(),
        Trigger::Mapping,
    )
    .await
    .unwrap();
    assert_eq!(outcomes.len(), 1, "each user is applied once");
    assert_eq!(outcomes[0].user_id, user);
    assert_eq!(
        outcomes[0].applied,
        apply_expected_member_change(OrgRole::Admin)
    );
    assert_eq!(holding(&fixture, user).await, ("member".to_owned(), None));
    assert_eq!(
        holding(&fixture, bystander).await,
        ("admin".to_owned(), None),
        "a member outside the deleted group keeps their role"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn apply_never_reaches_into_another_organization() {
    let fixture = Fixture::new(PREFIX).await;
    let other = fixture.another_organization().await;
    let user = linked(&fixture, OrgRole::Member).await;
    join(fixture.pool(), other, user, OrgRole::Member).await;
    link(fixture.pool(), other, user, "ada@acme.test").await;
    let admins = mapped(&fixture, "Admins", Mapping::BuiltIn(OrgRole::Admin)).await;
    add(&fixture, admins, &[user]).await;

    let mut connection = fixture.pool().acquire().await.unwrap();
    assert_eq!(
        roles::apply(&mut connection, other, user, Trigger::Membership)
            .await
            .unwrap(),
        Applied::Skipped,
        "a group of this organization grants nothing in another"
    );
    let role: String = sqlx::query_scalar(
        "SELECT role FROM organization_members WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(other)
    .bind(user)
    .fetch_one(fixture.pool())
    .await
    .unwrap();
    assert_eq!(role, "member");
    assert_eq!(
        roles::apply_group(&mut connection, other, admins, Trigger::Mapping)
            .await
            .unwrap(),
        vec![],
        "another organization's group has no members to apply"
    );
    drop(connection);
    fixture.cleanup().await;
}

#[tokio::test]
async fn errors_are_recorded_most_recent_first_and_pruned_after_thirty_days() {
    let fixture = Fixture::new(PREFIX).await;
    let other = fixture.another_organization().await;
    let (_, token) = issue(&fixture, fixture.organization, "Okta").await;

    sqlx::query(
        "INSERT INTO scim_errors (organization_id, method, path, status, detail, occurred_at) \
         VALUES ($1, 'GET', '/Users', 400, 'stale', now() - interval '31 days'), \
                ($1, 'GET', '/Users', 400, 'kept', now() - interval '29 days')",
    )
    .bind(fixture.organization)
    .execute(fixture.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO scim_errors (organization_id, method, path, status, detail, occurred_at) \
         VALUES ($1, 'GET', '/Users', 400, 'foreign', now() - interval '31 days')",
    )
    .bind(other)
    .execute(fixture.pool())
    .await
    .unwrap();

    let recorded = errors::record(
        fixture.pool(),
        &NewError {
            organization_id: fixture.organization,
            token_id: Some(token),
            method: "PATCH",
            path: "/api/scim/v2/organizations/{org_id}/Users/{user_id}",
            status: 409,
            scim_type: Some("uniqueness"),
            detail: "userName is taken",
        },
    )
    .await
    .unwrap();
    assert_eq!(recorded.status, 409);
    assert_eq!(recorded.scim_type.as_deref(), Some("uniqueness"));
    assert_eq!(recorded.token_id, Some(token));

    let details: Vec<String> = errors::recent(fixture.pool(), fixture.organization)
        .await
        .unwrap()
        .into_iter()
        .map(|error| error.detail)
        .collect();
    assert_eq!(details, vec!["userName is taken", "kept"]);
    let foreign: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM scim_errors WHERE organization_id = $1")
            .bind(other)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert_eq!(foreign, 1, "pruning one organization leaves another's rows");

    for index in 0..errors::RECENT + 5 {
        errors::record(
            fixture.pool(),
            &NewError {
                organization_id: fixture.organization,
                token_id: None,
                method: "GET",
                path: "/Users",
                status: 400,
                scim_type: None,
                detail: &format!("failure {index}"),
            },
        )
        .await
        .unwrap();
    }
    let recent = errors::recent(fixture.pool(), fixture.organization)
        .await
        .unwrap();
    assert_eq!(i64::try_from(recent.len()).unwrap(), errors::RECENT);
    assert_eq!(recent[0].detail, format!("failure {}", errors::RECENT + 4));
    assert!(recent.iter().all(|error| error.scim_type.is_none()));
    assert!(
        errors::recent(fixture.pool(), other)
            .await
            .unwrap()
            .iter()
            .all(|error| error.organization_id == other)
    );
    fixture.cleanup().await;
}
