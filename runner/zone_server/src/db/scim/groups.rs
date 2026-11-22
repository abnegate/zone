use chrono::{DateTime, Utc};
use sqlx::{Acquire, PgExecutor, Postgres, QueryBuilder};
use uuid::Uuid;

use super::criterion::Criterion;
use super::group_record::GroupRecord;
use super::listing::Listing;
use super::mapping::Mapping;
use crate::auth::Scope;
use crate::db::DbResult;
use crate::db::custom_roles::Summary;
use crate::db::organization_members::OrgRole;

pub const DISPLAY_NAME_KEY: &str = "scim_groups_display_name_key";
pub const EXTERNAL_ID_KEY: &str = "scim_groups_external_id_key";

const MAPPING_COLUMN: &str = "role";

#[derive(Debug, sqlx::FromRow)]
pub(super) struct Row {
    pub(super) id: Uuid,
    pub(super) organization_id: Uuid,
    pub(super) external_id: Option<String>,
    pub(super) display_name: String,
    pub(super) role: Option<String>,
    pub(super) custom_role_id: Option<Uuid>,
    pub(super) custom_role_name: Option<String>,
    pub(super) members: i64,
    pub(super) created_at: DateTime<Utc>,
    pub(super) updated_at: DateTime<Utc>,
}

impl Row {
    pub(super) fn record(self) -> DbResult<GroupRecord> {
        let mapping = match (self.role, self.custom_role_id, self.custom_role_name) {
            (None, None, _) | (None, Some(_), None) => Mapping::Unmapped,
            (None, Some(id), Some(name)) => Mapping::Custom(Summary { id, name }),
            (Some(role), None, _) => match role.parse::<OrgRole>() {
                Ok(role @ (OrgRole::Admin | OrgRole::Member)) => Mapping::BuiltIn(role),
                _ => return Err(undecodable(format!("{role} is not a mappable role"))),
            },
            (Some(_), Some(_), _) => {
                return Err(undecodable(
                    "a group maps to a built-in and a custom role".to_owned(),
                ));
            }
        };
        Ok(GroupRecord {
            id: self.id,
            organization_id: self.organization_id,
            external_id: self.external_id,
            display_name: self.display_name,
            mapping,
            members: self.members,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn undecodable(reason: String) -> sqlx::Error {
    sqlx::Error::ColumnDecode {
        index: MAPPING_COLUMN.to_owned(),
        source: reason.into(),
    }
}

fn select(source: &str) -> String {
    let scope = Scope::Organization.name();
    format!(
        r#"
        SELECT g.id, g.organization_id, g.external_id, g.display_name, g.role, g.custom_role_id,
            custom_role.name AS custom_role_name,
            (SELECT COUNT(*) FROM scim_group_members m WHERE m.group_id = g.id) AS members,
            g.created_at, g.updated_at
        FROM {source} g
        LEFT JOIN custom_roles custom_role
            ON custom_role.id = g.custom_role_id
            AND custom_role.organization_id = g.organization_id
            AND custom_role.scope = '{scope}'
        "#
    )
}

fn written(statement: &str) -> String {
    format!(
        "WITH written AS ({statement} RETURNING *) {}",
        select("written")
    )
}

fn filter(query: &mut QueryBuilder<Postgres>, organization_id: Uuid, criteria: &[Criterion]) {
    query
        .push(" WHERE g.organization_id = ")
        .push_bind(organization_id);
    for criterion in criteria {
        query.push(" AND ");
        match criterion {
            Criterion::Id(id) => {
                query.push("g.id = ").push_bind(*id);
            }
            Criterion::ExternalId(external_id) => {
                query
                    .push("g.external_id = ")
                    .push_bind(external_id.as_str());
            }
            Criterion::DisplayName(display_name) => {
                query
                    .push("lower(btrim(g.display_name)) = lower(btrim(")
                    .push_bind(display_name.as_str())
                    .push("))");
            }
            Criterion::Member(user_id) => {
                query
                    .push(
                        "EXISTS (SELECT 1 FROM scim_group_members m \
                         WHERE m.group_id = g.id AND m.user_id = ",
                    )
                    .push_bind(*user_id)
                    .push(")");
            }
            Criterion::UserName(_) => {
                query.push("FALSE");
            }
        }
    }
}

pub async fn create(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
    display_name: &str,
    external_id: Option<&str>,
) -> DbResult<GroupRecord> {
    let row: Row = sqlx::query_as(sqlx::AssertSqlSafe(written(
        "INSERT INTO scim_groups (organization_id, display_name, external_id) VALUES ($1, $2, $3)",
    )))
    .bind(organization_id)
    .bind(display_name)
    .bind(external_id)
    .fetch_one(executor)
    .await?;
    row.record()
}

pub async fn get(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
    id: Uuid,
) -> DbResult<Option<GroupRecord>> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} WHERE g.organization_id = $1 AND g.id = $2",
        select("scim_groups")
    )))
    .bind(organization_id)
    .bind(id)
    .fetch_optional(executor)
    .await?;
    row.map(Row::record).transpose()
}

pub async fn list<'c>(
    executor: impl Acquire<'c, Database = Postgres>,
    organization_id: Uuid,
    criteria: &[Criterion],
    offset: i64,
    limit: i64,
) -> DbResult<Listing<GroupRecord>> {
    let mut connection = executor.acquire().await?;

    let mut count = QueryBuilder::<Postgres>::new("SELECT COUNT(*) FROM scim_groups g");
    filter(&mut count, organization_id, criteria);
    let total: i64 = count
        .build_query_scalar()
        .fetch_one(&mut *connection)
        .await?;

    let mut page = QueryBuilder::<Postgres>::new(select("scim_groups"));
    filter(&mut page, organization_id, criteria);
    page.push(" ORDER BY g.created_at, g.id OFFSET ")
        .push_bind(offset.max(0))
        .push(" LIMIT ")
        .push_bind(limit.max(0));
    let rows: Vec<Row> = page.build_query_as().fetch_all(&mut *connection).await?;

    let items = rows
        .into_iter()
        .map(Row::record)
        .collect::<DbResult<Vec<GroupRecord>>>()?;
    Ok(Listing { items, total })
}

pub async fn update(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
    id: Uuid,
    display_name: Option<&str>,
    external_id: Option<Option<&str>>,
) -> DbResult<Option<GroupRecord>> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(written(
        r#"
        UPDATE scim_groups
        SET display_name = COALESCE($3, display_name),
            external_id = CASE WHEN $4 THEN $5 ELSE external_id END,
            updated_at = now()
        WHERE organization_id = $1 AND id = $2
        "#,
    )))
    .bind(organization_id)
    .bind(id)
    .bind(display_name)
    .bind(external_id.is_some())
    .bind(external_id.flatten())
    .fetch_optional(executor)
    .await?;
    row.map(Row::record).transpose()
}

pub async fn delete(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
    id: Uuid,
) -> DbResult<Option<Vec<Uuid>>> {
    sqlx::query_scalar(
        r#"
        WITH deleted AS (
            DELETE FROM scim_groups WHERE organization_id = $1 AND id = $2 RETURNING id
        )
        SELECT ARRAY(
            SELECT m.user_id FROM scim_group_members m WHERE m.group_id = deleted.id
            ORDER BY m.user_id
        )
        FROM deleted
        "#,
    )
    .bind(organization_id)
    .bind(id)
    .fetch_optional(executor)
    .await
}

pub async fn map(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
    id: Uuid,
    mapping: &Mapping,
    mapped_by: Uuid,
) -> DbResult<Option<GroupRecord>> {
    let (role, custom_role_id) = match mapping {
        Mapping::Unmapped => (None, None),
        Mapping::BuiltIn(role) => (Some(role.as_str()), None),
        Mapping::Custom(summary) => (None, Some(summary.id)),
    };
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(written(&format!(
        r#"
        UPDATE scim_groups
        SET role = $3, custom_role_id = $4, mapped_by = $5, updated_at = now()
        WHERE organization_id = $1 AND id = $2
          AND ($4::uuid IS NULL OR EXISTS (
              SELECT 1 FROM custom_roles
              WHERE id = $4 AND organization_id = $1 AND scope = '{}'
          ))
        "#,
        Scope::Organization.name()
    ))))
    .bind(organization_id)
    .bind(id)
    .bind(role)
    .bind(custom_role_id)
    .bind(mapped_by)
    .fetch_optional(executor)
    .await?;
    row.map(Row::record).transpose()
}
