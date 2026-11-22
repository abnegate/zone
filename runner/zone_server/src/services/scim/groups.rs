use std::collections::{BTreeSet, HashMap};

use serde_json::{Map, Value, json};
use sqlx::PgConnection;
use uuid::Uuid;
use zone_enterprise::scim::{
    Attribute, Error, Filter, GroupChanges, GroupDocument, GroupResource, ListQuery, ListResponse,
    MemberChanges, MemberReference, Page, PatchRequest,
};

use super::client::ScimClient;
use super::failure::Failure;
use super::{record, resource};
use crate::db::audit::{actions, resources};
use crate::db::scim::{
    Admission, Applied, Criterion, Outcome, Trigger, group_members, groups, roles, rosters,
};
use crate::state::AppState;

pub async fn list(
    state: &AppState,
    client: &ScimClient,
    query: &ListQuery,
) -> Result<ListResponse<GroupResource>, Failure> {
    let filter = query.parsed_filter()?;
    let page = Page::from(query);
    let Some(criteria) = criteria(filter.as_ref()) else {
        return Ok(ListResponse::new(Vec::new(), 0, page.start_index()));
    };
    let organization_id = client.organization_id;
    let listing = groups::list(
        state.db(),
        organization_id,
        &criteria,
        page.offset(),
        page.limit(),
    )
    .await?;
    let base = resource::base(state, organization_id);
    let mut rosters = if query.excludes_members() {
        None
    } else {
        let ids: Vec<Uuid> = listing.items.iter().map(|record| record.id).collect();
        Some(roster(state, organization_id, &ids, &base).await?)
    };
    let resources = listing
        .items
        .iter()
        .map(|record| {
            let members = rosters
                .as_mut()
                .map(|rosters| rosters.remove(&record.id).unwrap_or_default());
            resource::group(&base, record, members)
        })
        .collect();
    Ok(ListResponse::new(
        resources,
        u64::try_from(listing.total).unwrap_or_default(),
        page.start_index(),
    ))
}

pub async fn get(
    state: &AppState,
    client: &ScimClient,
    group_id: Uuid,
    excludes_members: bool,
) -> Result<GroupResource, Failure> {
    let organization_id = client.organization_id;
    let record = groups::get(state.db(), organization_id, group_id)
        .await?
        .ok_or(Failure::Protocol(Error::NotFound))?;
    let base = resource::base(state, organization_id);
    let members = if excludes_members {
        None
    } else {
        let mut rosters = roster(state, organization_id, &[group_id], &base).await?;
        Some(rosters.remove(&group_id).unwrap_or_default())
    };
    Ok(resource::group(&base, &record, members))
}

pub async fn create(
    state: &AppState,
    client: &ScimClient,
    document: &GroupDocument,
) -> Result<GroupResource, Failure> {
    document.validate()?;
    let changes = GroupChanges::from_document(document);
    let organization_id = client.organization_id;
    let mut transaction = state.db().begin().await?;
    let record = groups::create(
        &mut *transaction,
        organization_id,
        &document.display_name,
        changes.external_id.as_ref().and_then(Option::as_deref),
    )
    .await
    .map_err(Failure::stored)?;
    let affected = members(
        &mut transaction,
        organization_id,
        record.id,
        &changes.members,
    )
    .await?;
    transaction.commit().await?;
    finish(
        state,
        client,
        actions::SCIM_GROUP_CREATED,
        record.id,
        &changes,
        affected,
    )
    .await
}

pub async fn replace(
    state: &AppState,
    client: &ScimClient,
    group_id: Uuid,
    document: &GroupDocument,
) -> Result<GroupResource, Failure> {
    document.validate()?;
    write(
        state,
        client,
        group_id,
        GroupChanges::from_document(document),
    )
    .await
}

pub async fn patch(
    state: &AppState,
    client: &ScimClient,
    group_id: Uuid,
    request: &PatchRequest,
) -> Result<GroupResource, Failure> {
    let changes = GroupChanges::from_patch(request)?;
    write(state, client, group_id, changes).await
}

pub async fn delete(state: &AppState, client: &ScimClient, group_id: Uuid) -> Result<(), Failure> {
    let organization_id = client.organization_id;
    let record = groups::get(state.db(), organization_id, group_id)
        .await?
        .ok_or(Failure::Protocol(Error::NotFound))?;
    let former = groups::delete(state.db(), organization_id, group_id)
        .await?
        .ok_or(Failure::Protocol(Error::NotFound))?;
    let outcomes = if record.mapping.is_mapped() {
        roles::apply_users(state.db(), organization_id, &former, Trigger::Mapping).await?
    } else {
        Vec::new()
    };

    let mut attributes = Map::new();
    attributes.insert("display_name".to_owned(), json!(record.display_name));
    attributes.insert("members".to_owned(), json!(former));
    describe_roles(&mut attributes, &outcomes);
    record::audit(
        state,
        client,
        actions::SCIM_GROUP_DELETED,
        resources::SCIM_GROUP,
        group_id,
        attributes,
    )
    .await;
    Ok(())
}

async fn write(
    state: &AppState,
    client: &ScimClient,
    group_id: Uuid,
    changes: GroupChanges,
) -> Result<GroupResource, Failure> {
    let organization_id = client.organization_id;
    let mut transaction = state.db().begin().await?;
    groups::update(
        &mut *transaction,
        organization_id,
        group_id,
        changes.display_name.as_deref(),
        changes.external_id.as_ref().map(Option::as_deref),
    )
    .await
    .map_err(Failure::stored)?
    .ok_or(Failure::Protocol(Error::NotFound))?;
    let affected = members(
        &mut transaction,
        organization_id,
        group_id,
        &changes.members,
    )
    .await?;
    transaction.commit().await?;
    finish(
        state,
        client,
        actions::SCIM_GROUP_UPDATED,
        group_id,
        &changes,
        affected,
    )
    .await
}

async fn members(
    connection: &mut PgConnection,
    organization_id: Uuid,
    group_id: Uuid,
    changes: &MemberChanges,
) -> Result<BTreeSet<Uuid>, Failure> {
    let mut affected = BTreeSet::new();
    if changes.clear {
        let replaced = group_members::replace(connection, organization_id, group_id, &[])
            .await
            .map_err(missing)?;
        affected.extend(admitted(replaced)?);
    }
    if let Some(listed) = &changes.replace {
        let users = identifiers(listed)?;
        let replaced = group_members::replace(connection, organization_id, group_id, &users)
            .await
            .map_err(missing)?;
        affected.extend(admitted(replaced)?);
    }
    if !changes.add.is_empty() {
        let users = identifiers(&changes.add)?;
        let added = group_members::add(connection, organization_id, group_id, &users)
            .await
            .map_err(missing)?;
        affected.extend(admitted(added)?);
    }
    let removable = removable(&changes.remove);
    if !removable.is_empty() {
        let removed = group_members::remove(connection, organization_id, group_id, &removable)
            .await
            .map_err(missing)?;
        affected.extend(removed);
    }
    Ok(affected)
}

async fn finish(
    state: &AppState,
    client: &ScimClient,
    action: &str,
    group_id: Uuid,
    changes: &GroupChanges,
    affected: BTreeSet<Uuid>,
) -> Result<GroupResource, Failure> {
    let affected: Vec<Uuid> = affected.into_iter().collect();
    let outcomes = roles::apply_users(
        state.db(),
        client.organization_id,
        &affected,
        Trigger::Membership,
    )
    .await?;

    let mut attributes = Map::new();
    if let Some(display_name) = &changes.display_name {
        attributes.insert("display_name".to_owned(), json!(display_name));
    }
    if let Some(external_id) = &changes.external_id {
        attributes.insert("external_id".to_owned(), json!(external_id));
    }
    if !affected.is_empty() {
        attributes.insert("members".to_owned(), json!(affected));
    }
    describe_roles(&mut attributes, &outcomes);
    record::audit(
        state,
        client,
        action,
        resources::SCIM_GROUP,
        group_id,
        attributes,
    )
    .await;
    get(state, client, group_id, false).await
}

async fn roster(
    state: &AppState,
    organization_id: Uuid,
    groups: &[Uuid],
    base: &str,
) -> Result<HashMap<Uuid, Vec<MemberReference>>, Failure> {
    let mut rosters: HashMap<Uuid, Vec<MemberReference>> = HashMap::with_capacity(groups.len());
    if groups.is_empty() {
        return Ok(rosters);
    }
    for entry in rosters::list(state.db(), organization_id, groups).await? {
        rosters
            .entry(entry.group_id)
            .or_default()
            .push(resource::member(base, entry.user_id, entry.display));
    }
    Ok(rosters)
}

fn criteria(filter: Option<&Filter>) -> Option<Vec<Criterion>> {
    let Some(filter) = filter else {
        return Some(Vec::new());
    };
    match filter {
        Filter::And(filters) => {
            let mut all = Vec::with_capacity(filters.len());
            for filter in filters {
                all.extend(criteria(Some(filter))?);
            }
            Some(all)
        }
        Filter::Equals { attribute, value } => Some(vec![match attribute {
            Attribute::Id => Criterion::Id(Uuid::parse_str(value.trim()).ok()?),
            Attribute::DisplayName => Criterion::DisplayName(value.clone()),
            Attribute::ExternalId => Criterion::ExternalId(value.clone()),
            Attribute::UserName => return None,
        }]),
        Filter::Member(value) => Some(vec![Criterion::Member(Uuid::parse_str(value.trim()).ok()?)]),
    }
}

fn identifiers(values: &[String]) -> Result<Vec<Uuid>, Failure> {
    values
        .iter()
        .map(|value| {
            Uuid::parse_str(value.trim()).map_err(|_| {
                Failure::Protocol(Error::InvalidValue(format!(
                    "{value} isn't the id of a member of the organization."
                )))
            })
        })
        .collect()
}

fn removable(values: &[String]) -> Vec<Uuid> {
    values
        .iter()
        .filter_map(|value| Uuid::parse_str(value.trim()).ok())
        .collect()
}

fn admitted(admission: Admission) -> Result<Vec<Uuid>, Failure> {
    match admission {
        Admission::Admitted(users) => Ok(users),
        Admission::Strangers(strangers) => {
            let listed: Vec<String> = strangers.iter().map(Uuid::to_string).collect();
            Err(Failure::Protocol(Error::InvalidValue(format!(
                "These aren't members of the organization: {}.",
                listed.join(", ")
            ))))
        }
    }
}

fn missing(error: sqlx::Error) -> Failure {
    match error {
        sqlx::Error::RowNotFound => Failure::Protocol(Error::NotFound),
        error => Failure::Storage(error),
    }
}

fn describe_roles(attributes: &mut Map<String, Value>, outcomes: &[Outcome]) {
    let changed: Vec<Value> = outcomes
        .iter()
        .filter_map(|outcome| match &outcome.applied {
            Applied::Changed { to, .. } => Some(json!({
                "user_id": outcome.user_id,
                "role": to.role.as_str(),
                "custom_role": to.custom_role,
            })),
            Applied::Skipped | Applied::Unchanged => None,
        })
        .collect();
    if !changed.is_empty() {
        attributes.insert("roles".to_owned(), Value::Array(changed));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_filters_match_a_member_and_impossible_ones_match_nothing() {
        let group = Uuid::new_v4();
        let user = Uuid::new_v4();
        assert_eq!(
            criteria(Some(&Filter::And(vec![
                Filter::Equals {
                    attribute: Attribute::Id,
                    value: group.to_string(),
                },
                Filter::Member(user.to_string()),
            ]))),
            Some(vec![Criterion::Id(group), Criterion::Member(user)])
        );
        assert_eq!(criteria(Some(&Filter::Member("jyoung".to_owned()))), None);
        assert_eq!(
            criteria(Some(&Filter::Equals {
                attribute: Attribute::UserName,
                value: "ada".to_owned(),
            })),
            None
        );
        assert_eq!(
            criteria(Some(&Filter::Equals {
                attribute: Attribute::DisplayName,
                value: "Engineering".to_owned(),
            })),
            Some(vec![Criterion::DisplayName("Engineering".to_owned())])
        );
    }

    #[test]
    fn an_added_member_must_be_an_id_but_a_removed_one_need_not_be() {
        let user = Uuid::new_v4();
        assert_eq!(identifiers(&[format!(" {user} ")]).unwrap(), vec![user]);
        assert!(matches!(
            identifiers(&["jyoung".to_owned()]),
            Err(Failure::Protocol(Error::InvalidValue(_)))
        ));
        assert_eq!(
            removable(&["jyoung".to_owned(), user.to_string()]),
            vec![user]
        );
    }

    #[test]
    fn strangers_are_refused_by_id() {
        let stranger = Uuid::new_v4();
        let refused = admitted(Admission::Strangers(vec![stranger])).unwrap_err();
        assert_eq!(refused.status(), 400);
        assert!(refused.detail().contains(&stranger.to_string()));
    }
}
