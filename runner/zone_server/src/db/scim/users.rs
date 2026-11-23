use chrono::{DateTime, Utc};
use sqlx::{Acquire, PgExecutor, Postgres, QueryBuilder};
use uuid::Uuid;

use super::criterion::Criterion;
use super::group_reference::GroupReference;
use super::link::Link;
use super::listing::Listing;
use super::member::Member;
use crate::db::DbResult;
use crate::db::organization_members::OrgRole;

pub const USER_NAME_KEY: &str = "scim_users_user_name_key";
pub const EXTERNAL_ID_KEY: &str = "scim_users_external_id_key";

const ROLE_COLUMN: &str = "role";

const LINK_COLUMNS: &str = "organization_id, user_id, external_id, user_name, display_name, \
    given_name, family_name, email, provisioned_password, created_at, updated_at";

const MEMBER_SOURCE: &str = r#"
    FROM organization_members om
    INNER JOIN users u ON u.id = om.user_id
    LEFT JOIN scim_users s ON s.organization_id = om.organization_id AND s.user_id = om.user_id
    WHERE om.organization_id = "#;

const MEMBER_COLUMNS: &str = r#"
    SELECT om.user_id, u.email, u.display_name, om.is_active AS active, om.role,
        COALESCE(om.created_at, now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC' AS created_at,
        GREATEST(
            COALESCE(om.updated_at, om.created_at, now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC',
            s.updated_at
        ) AS updated_at,
        s.external_id AS link_external_id,
        s.user_name AS link_user_name,
        s.display_name AS link_display_name,
        s.given_name AS link_given_name,
        s.family_name AS link_family_name,
        s.email AS link_email,
        s.provisioned_password AS link_provisioned_password,
        s.created_at AS link_created_at,
        s.updated_at AS link_updated_at,
        ARRAY(
            SELECT g.id FROM scim_group_members gm
            INNER JOIN scim_groups g ON g.id = gm.group_id
            WHERE g.organization_id = om.organization_id AND gm.user_id = om.user_id
            ORDER BY lower(g.display_name), g.id
        ) AS group_ids,
        ARRAY(
            SELECT g.display_name FROM scim_group_members gm
            INNER JOIN scim_groups g ON g.id = gm.group_id
            WHERE g.organization_id = om.organization_id AND gm.user_id = om.user_id
            ORDER BY lower(g.display_name), g.id
        ) AS group_names"#;

#[derive(sqlx::FromRow)]
struct Row {
    user_id: Uuid,
    email: String,
    display_name: Option<String>,
    active: bool,
    role: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    link_external_id: Option<String>,
    link_user_name: Option<String>,
    link_display_name: Option<String>,
    link_given_name: Option<String>,
    link_family_name: Option<String>,
    link_email: Option<String>,
    link_provisioned_password: Option<String>,
    link_created_at: Option<DateTime<Utc>>,
    link_updated_at: Option<DateTime<Utc>>,
    group_ids: Vec<Uuid>,
    group_names: Vec<String>,
}

impl Row {
    fn member(self, organization_id: Uuid) -> DbResult<Member> {
        let role = self
            .role
            .parse::<OrgRole>()
            .map_err(|_| sqlx::Error::ColumnDecode {
                index: ROLE_COLUMN.to_owned(),
                source: format!("{} is not an organization role", self.role).into(),
            })?;
        let link = match (
            self.link_user_name,
            self.link_created_at,
            self.link_updated_at,
        ) {
            (Some(user_name), Some(created_at), Some(updated_at)) => Some(Link {
                organization_id,
                user_id: self.user_id,
                external_id: self.link_external_id,
                user_name,
                display_name: self.link_display_name,
                given_name: self.link_given_name,
                family_name: self.link_family_name,
                email: self.link_email,
                provisioned_password: self.link_provisioned_password,
                created_at,
                updated_at,
            }),
            _ => None,
        };
        let groups = self
            .group_ids
            .into_iter()
            .zip(self.group_names)
            .map(|(id, display_name)| GroupReference { id, display_name })
            .collect();
        Ok(Member {
            user_id: self.user_id,
            email: self.email,
            display_name: self.display_name,
            active: self.active,
            role,
            link,
            groups,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn filter(query: &mut QueryBuilder<Postgres>, organization_id: Uuid, criteria: &[Criterion]) {
    query.push(MEMBER_SOURCE).push_bind(organization_id);
    for criterion in criteria {
        query.push(" AND ");
        match criterion {
            Criterion::Id(id) => {
                query.push("om.user_id = ").push_bind(*id);
            }
            Criterion::UserName(user_name) => {
                query
                    .push("(lower(s.user_name) = lower(")
                    .push_bind(user_name.as_str())
                    .push(") OR (s.user_id IS NULL AND lower(u.email) = lower(")
                    .push_bind(user_name.as_str())
                    .push(")))");
            }
            Criterion::ExternalId(external_id) => {
                query
                    .push("s.external_id = ")
                    .push_bind(external_id.as_str());
            }
            Criterion::DisplayName(display_name) => {
                query
                    .push("lower(COALESCE(s.display_name, u.display_name)) = lower(")
                    .push_bind(display_name.as_str())
                    .push(")");
            }
            Criterion::Member(_) => {
                query.push("FALSE");
            }
        }
    }
}

pub async fn get(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<Option<Member>> {
    let mut query = QueryBuilder::<Postgres>::new(MEMBER_COLUMNS);
    filter(&mut query, organization_id, &[Criterion::Id(user_id)]);
    let row: Option<Row> = query.build_query_as().fetch_optional(executor).await?;
    row.map(|row| row.member(organization_id)).transpose()
}

pub async fn list<'c>(
    executor: impl Acquire<'c, Database = Postgres>,
    organization_id: Uuid,
    criteria: &[Criterion],
    offset: i64,
    limit: i64,
) -> DbResult<Listing<Member>> {
    let mut connection = executor.acquire().await?;

    let mut count = QueryBuilder::<Postgres>::new("SELECT COUNT(*)");
    filter(&mut count, organization_id, criteria);
    let total: i64 = count
        .build_query_scalar()
        .fetch_one(&mut *connection)
        .await?;

    let mut page = QueryBuilder::<Postgres>::new(MEMBER_COLUMNS);
    filter(&mut page, organization_id, criteria);
    page.push(" ORDER BY om.created_at, om.user_id OFFSET ")
        .push_bind(offset.max(0))
        .push(" LIMIT ")
        .push_bind(limit.max(0));
    let rows: Vec<Row> = page.build_query_as().fetch_all(&mut *connection).await?;

    let items = rows
        .into_iter()
        .map(|row| row.member(organization_id))
        .collect::<DbResult<Vec<Member>>>()?;
    Ok(Listing { items, total })
}

pub async fn upsert(executor: impl PgExecutor<'_>, link: &Link) -> DbResult<Link> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        r#"
        INSERT INTO scim_users (organization_id, user_id, external_id, user_name, display_name,
                                given_name, family_name, email, provisioned_password)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        ON CONFLICT (organization_id, user_id) DO UPDATE
        SET external_id = EXCLUDED.external_id,
            user_name = EXCLUDED.user_name,
            display_name = EXCLUDED.display_name,
            given_name = EXCLUDED.given_name,
            family_name = EXCLUDED.family_name,
            email = EXCLUDED.email,
            provisioned_password = COALESCE(EXCLUDED.provisioned_password,
                                            scim_users.provisioned_password),
            updated_at = now()
        RETURNING {LINK_COLUMNS}
        "#
    )))
    .bind(link.organization_id)
    .bind(link.user_id)
    .bind(&link.external_id)
    .bind(&link.user_name)
    .bind(&link.display_name)
    .bind(&link.given_name)
    .bind(&link.family_name)
    .bind(&link.email)
    .bind(&link.provisioned_password)
    .fetch_one(executor)
    .await
}

pub async fn pristine(executor: impl PgExecutor<'_>, user_id: Uuid) -> DbResult<bool> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM scim_users s
            INNER JOIN users u ON u.id = s.user_id
            WHERE s.user_id = $1
              AND s.provisioned_password = u.password_hash
              AND u.last_login_at IS NULL
        )
        AND NOT EXISTS (SELECT 1 FROM sso_identities WHERE user_id = $1)
        "#,
    )
    .bind(user_id)
    .fetch_one(executor)
    .await
}

pub async fn created(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM scim_users \
         WHERE organization_id = $1 AND user_id = $2 AND provisioned_password IS NOT NULL)",
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_one(executor)
    .await
}

pub async fn sole_organization(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<bool> {
    sqlx::query_scalar(
        "SELECT NOT EXISTS (SELECT 1 FROM organization_members \
         WHERE user_id = $2 AND organization_id <> $1)",
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_one(executor)
    .await
}
