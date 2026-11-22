use serde_json::{Map, Value, json};
use uuid::Uuid;
use zone_enterprise::scim::{
    Attribute, Error, Filter, ListQuery, ListResponse, Page, UserChanges, UserDocument,
    UserResource,
};

use super::client::ScimClient;
use super::entitlement::Entitlement;
use super::failure::Failure;
use super::{password, record, resource};
use crate::db::audit::{actions, resources};
use crate::db::organization_members::{self, OrgRole};
use crate::db::scim::{
    Applied, Criterion, Deactivation, Link, Removal, Trigger, accounts, membership, roles,
    users as links,
};
use crate::db::{suspensions, users};
use crate::services::account::provision;
use crate::services::members;
use crate::state::AppState;

const ALREADY_MEMBER: &str = "That person is already a member of the organization.";
const AMBIGUOUS: &str = "More than one account has that email address.";
const EMAIL_REQUIRED: &str = "A user needs an email address, in emails or as its userName.";
const EMAIL_INVALID: &str = "emails needs a valid email address.";
const UNHASHED: &str = "A new account has no provisioned password";

pub fn narrow(entitlement: Entitlement, changes: UserChanges) -> Result<UserChanges, Failure> {
    match entitlement {
        Entitlement::Full => Ok(changes),
        Entitlement::Deprovisioning if changes.deactivates() => Ok(changes.only_deactivation()),
        Entitlement::Deprovisioning => Err(Failure::Unentitled),
    }
}

pub async fn list(
    state: &AppState,
    client: &ScimClient,
    query: &ListQuery,
) -> Result<ListResponse<UserResource>, Failure> {
    let filter = query.parsed_filter()?;
    let page = Page::from(query);
    let Some(criteria) = criteria(filter.as_ref()) else {
        return Ok(ListResponse::new(Vec::new(), 0, page.start_index()));
    };
    let listing = links::list(
        state.db(),
        client.organization_id,
        &criteria,
        page.offset(),
        page.limit(),
    )
    .await?;
    let base = resource::base(state, client.organization_id);
    let resources = listing
        .items
        .iter()
        .map(|member| resource::user(&base, member))
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
    user_id: Uuid,
) -> Result<UserResource, Failure> {
    let member = links::get(state.db(), client.organization_id, user_id)
        .await?
        .ok_or(Failure::Protocol(Error::NotFound))?;
    Ok(resource::user(
        &resource::base(state, client.organization_id),
        &member,
    ))
}

pub async fn create(
    state: &AppState,
    client: &ScimClient,
    document: &UserDocument,
) -> Result<UserResource, Failure> {
    document.validate()?;
    let organization_id = client.organization_id;
    let changes = UserChanges::from_document(document);
    let email = address(document)?;

    let account = match users::find_by_email_ignoring_case(state.db(), &email)
        .await?
        .as_slice()
    {
        [] => None,
        [user] => Some(user.id),
        _ => return Err(conflict(AMBIGUOUS)),
    };
    if let Some(user_id) = account
        && organization_members::get_member(state.db(), organization_id, user_id)
            .await?
            .is_some_and(|member| member.is_active)
    {
        return Err(conflict(ALREADY_MEMBER));
    }
    let cap = members::cap(state).await?;
    members::check(state, cap, account).await?;
    let provisioned = match account {
        Some(_) => None,
        None => Some(password::unusable().await?),
    };

    let mut transaction = state.db().begin().await?;
    members::admit(&mut transaction, cap, account).await?;
    organization_members::lock_owners(&mut transaction, organization_id).await?;
    let user_id = match account {
        Some(user_id) => user_id,
        None => {
            let hash = provisioned.as_deref().ok_or(Failure::Internal(UNHASHED))?;
            provision::insert(
                &mut transaction,
                &email,
                hash,
                account_name(&changes).as_deref(),
                false,
                false,
            )
            .await
            .map_err(Failure::stored)?
            .id
        }
    };
    suspensions::forget(&mut *transaction, organization_id, user_id).await?;
    organization_members::reactivate_member(
        &mut *transaction,
        organization_id,
        user_id,
        OrgRole::Member,
        None,
    )
    .await?;
    let mut link = Link::new(organization_id, user_id, document.user_name.trim());
    apply(&mut link, &changes);
    link.email = Some(email);
    link.provisioned_password = provisioned;
    links::upsert(&mut *transaction, &link)
        .await
        .map_err(Failure::stored)?;
    let applied = roles::apply(
        &mut transaction,
        organization_id,
        user_id,
        Trigger::Membership,
    )
    .await?;
    transaction.commit().await?;

    let mut attributes = described(&UserChanges {
        active: None,
        ..changes.clone()
    });
    attributes.insert("created_account".to_owned(), json!(account.is_none()));
    describe_role(&mut attributes, &applied);
    record::audit(
        state,
        client,
        actions::SCIM_USER_CREATED,
        resources::MEMBER,
        user_id,
        attributes,
    )
    .await;

    if changes.deactivates() {
        return update(state, client, user_id, changes.only_deactivation()).await;
    }
    get(state, client, user_id).await
}

pub async fn update(
    state: &AppState,
    client: &ScimClient,
    user_id: Uuid,
    changes: UserChanges,
) -> Result<UserResource, Failure> {
    let organization_id = client.organization_id;
    let member = links::get(state.db(), organization_id, user_id)
        .await?
        .ok_or(Failure::Protocol(Error::NotFound))?;
    if changes
        .email
        .as_deref()
        .is_some_and(|email| !is_address(email))
    {
        return Err(Failure::Protocol(Error::InvalidValue(
            EMAIL_INVALID.to_owned(),
        )));
    }
    let attributes = UserChanges {
        active: None,
        ..changes.clone()
    };
    let deactivating = changes.deactivates() && member.active;
    let reactivating = changes.active == Some(true) && !member.active;
    let cap = if reactivating {
        Some(members::cap(state).await?)
    } else {
        None
    };

    let mut transaction = state.db().begin().await?;
    if let Some(cap) = cap {
        members::admit(&mut transaction, cap, Some(user_id)).await?;
    }
    organization_members::lock_owners(&mut transaction, organization_id).await?;
    if !attributes.is_empty() {
        let mut link = member
            .link
            .clone()
            .unwrap_or_else(|| Link::new(organization_id, user_id, member.email.clone()));
        apply(&mut link, &attributes);
        let link = links::upsert(&mut *transaction, &link)
            .await
            .map_err(Failure::stored)?;
        if link.provisioned_password.is_some()
            && links::sole_organization(&mut *transaction, organization_id, user_id).await?
        {
            accounts::update(
                &mut *transaction,
                user_id,
                attributes.email.as_deref(),
                attributes.display_name.as_ref().map(Option::as_deref),
            )
            .await
            .map_err(Failure::stored)?;
        }
    }
    let mut successors = Vec::new();
    let applied = if deactivating {
        match membership::deactivate(&mut transaction, organization_id, user_id).await? {
            Deactivation::LastOwner => return Err(Failure::LastOwner),
            Deactivation::Deactivated(suspension) => {
                successors = suspension.successors;
                Applied::Skipped
            }
        }
    } else if reactivating {
        membership::reactivate(&mut transaction, organization_id, user_id).await?;
        roles::apply(
            &mut transaction,
            organization_id,
            user_id,
            Trigger::Reactivation,
        )
        .await?
    } else if member.active {
        roles::apply(
            &mut transaction,
            organization_id,
            user_id,
            Trigger::Membership,
        )
        .await?
    } else {
        Applied::Skipped
    };
    transaction.commit().await?;

    if changes.deactivates() {
        members::deprovision(state, organization_id, user_id).await?;
    }

    if !attributes.is_empty() || !(deactivating || reactivating) {
        let mut described = described(&attributes);
        if !reactivating {
            describe_role(&mut described, &applied);
        }
        record::audit(
            state,
            client,
            actions::SCIM_USER_UPDATED,
            resources::MEMBER,
            user_id,
            described,
        )
        .await;
    }
    if deactivating {
        record::audit(
            state,
            client,
            actions::SCIM_USER_DEACTIVATED,
            resources::MEMBER,
            user_id,
            active(false),
        )
        .await;
        record::successors(state, client, &successors).await;
    }
    if reactivating {
        let mut described = active(true);
        describe_role(&mut described, &applied);
        record::audit(
            state,
            client,
            actions::SCIM_USER_REACTIVATED,
            resources::MEMBER,
            user_id,
            described,
        )
        .await;
    }
    get(state, client, user_id).await
}

pub async fn delete(state: &AppState, client: &ScimClient, user_id: Uuid) -> Result<(), Failure> {
    let organization_id = client.organization_id;
    let mut transaction = state.db().begin().await?;
    let successors = match membership::remove(&mut transaction, organization_id, user_id).await? {
        Removal::Missing => return Err(Failure::Protocol(Error::NotFound)),
        Removal::LastOwner => return Err(Failure::LastOwner),
        Removal::Removed(successors) => successors,
    };
    transaction.commit().await?;

    members::deprovision(state, organization_id, user_id).await?;
    record::audit(
        state,
        client,
        actions::SCIM_USER_DELETED,
        resources::MEMBER,
        user_id,
        Map::new(),
    )
    .await;
    record::successors(state, client, &successors).await;
    Ok(())
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
            Attribute::UserName => Criterion::UserName(value.clone()),
            Attribute::ExternalId => Criterion::ExternalId(value.clone()),
            Attribute::DisplayName => Criterion::DisplayName(value.clone()),
        }]),
        Filter::Member(_) => None,
    }
}

fn address(document: &UserDocument) -> Result<String, Failure> {
    document
        .primary_email()
        .map(str::trim)
        .filter(|email| is_address(email))
        .or_else(|| Some(document.user_name.trim()).filter(|name| is_address(name)))
        .map(str::to_owned)
        .ok_or_else(|| Failure::Protocol(Error::InvalidValue(EMAIL_REQUIRED.to_owned())))
}

fn is_address(candidate: &str) -> bool {
    let Some((local, domain)) = candidate.trim().split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !candidate.trim().chars().any(char::is_whitespace)
}

fn account_name(changes: &UserChanges) -> Option<String> {
    if let Some(Some(display_name)) = &changes.display_name {
        return Some(display_name.clone());
    }
    let parts: Vec<&str> = [&changes.given_name, &changes.family_name]
        .into_iter()
        .filter_map(|part| part.as_ref().and_then(Option::as_deref))
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn apply(link: &mut Link, changes: &UserChanges) {
    if let Some(user_name) = &changes.user_name {
        link.user_name = user_name.trim().to_owned();
    }
    if let Some(external_id) = &changes.external_id {
        link.external_id = external_id.clone();
    }
    if let Some(display_name) = &changes.display_name {
        link.display_name = display_name.clone();
    }
    if let Some(given_name) = &changes.given_name {
        link.given_name = given_name.clone();
    }
    if let Some(family_name) = &changes.family_name {
        link.family_name = family_name.clone();
    }
    if let Some(email) = &changes.email {
        link.email = Some(email.trim().to_owned());
    }
}

fn described(changes: &UserChanges) -> Map<String, Value> {
    let mut attributes = Map::new();
    if let Some(user_name) = &changes.user_name {
        attributes.insert("user_name".to_owned(), json!(user_name));
    }
    if let Some(external_id) = &changes.external_id {
        attributes.insert("external_id".to_owned(), json!(external_id));
    }
    if let Some(display_name) = &changes.display_name {
        attributes.insert("display_name".to_owned(), json!(display_name));
    }
    if let Some(given_name) = &changes.given_name {
        attributes.insert("given_name".to_owned(), json!(given_name));
    }
    if let Some(family_name) = &changes.family_name {
        attributes.insert("family_name".to_owned(), json!(family_name));
    }
    if let Some(email) = &changes.email {
        attributes.insert("email".to_owned(), json!(email));
    }
    attributes
}

fn active(active: bool) -> Map<String, Value> {
    let mut attributes = Map::new();
    attributes.insert("active".to_owned(), json!(active));
    attributes
}

fn describe_role(attributes: &mut Map<String, Value>, applied: &Applied) {
    if let Applied::Changed { to, .. } = applied {
        attributes.insert("role".to_owned(), json!(to.role.as_str()));
        attributes.insert("custom_role".to_owned(), json!(to.custom_role));
    }
}

fn conflict(detail: &str) -> Failure {
    Failure::Protocol(Error::Uniqueness(detail.to_owned()))
}

#[cfg(test)]
mod tests {
    use zone_enterprise::scim::{Email, Flag};

    use super::*;

    fn document(user_name: &str, emails: Vec<Email>) -> UserDocument {
        UserDocument {
            user_name: user_name.to_owned(),
            external_id: None,
            name: None,
            display_name: None,
            emails,
            active: Some(Flag(true)),
        }
    }

    #[test]
    fn a_full_license_applies_every_change() {
        let changes = UserChanges {
            display_name: Some(Some("Ada".to_owned())),
            active: Some(true),
            ..UserChanges::default()
        };
        assert_eq!(narrow(Entitlement::Full, changes.clone()).unwrap(), changes);
    }

    #[test]
    fn a_lapsed_license_applies_only_a_deactivation() {
        let changes = UserChanges {
            display_name: Some(Some("Ada".to_owned())),
            email: Some("ada@example.com".to_owned()),
            active: Some(false),
            ..UserChanges::default()
        };
        assert_eq!(
            narrow(Entitlement::Deprovisioning, changes).unwrap(),
            UserChanges {
                active: Some(false),
                ..UserChanges::default()
            }
        );
    }

    #[test]
    fn a_lapsed_license_refuses_everything_else() {
        for changes in [
            UserChanges {
                active: Some(true),
                ..UserChanges::default()
            },
            UserChanges {
                display_name: Some(None),
                ..UserChanges::default()
            },
            UserChanges::default(),
        ] {
            assert!(matches!(
                narrow(Entitlement::Deprovisioning, changes),
                Err(Failure::Unentitled)
            ));
        }
    }

    #[test]
    fn the_address_is_the_primary_email_then_the_user_name() {
        let primary = document("ada", vec![Email::work("ada@example.com")]);
        assert_eq!(address(&primary).unwrap(), "ada@example.com");

        let named = document("ada@example.com", Vec::new());
        assert_eq!(address(&named).unwrap(), "ada@example.com");

        let invalid = document("ada@example.com", vec![Email::work("not an address")]);
        assert_eq!(address(&invalid).unwrap(), "ada@example.com");

        let neither = document("ada", Vec::new());
        assert!(matches!(
            address(&neither),
            Err(Failure::Protocol(Error::InvalidValue(_)))
        ));
    }

    #[test]
    fn an_address_has_one_at_sign_and_no_spaces() {
        for valid in ["ada@example.com", "ada+scim@sub.example.co"] {
            assert!(is_address(valid), "{valid}");
        }
        for invalid in [
            "ada",
            "@example.com",
            "ada@",
            "ada@@example.com",
            "ada@.example.com",
            "ada@example.com.",
            "a da@example.com",
        ] {
            assert!(!is_address(invalid), "{invalid}");
        }
    }

    #[test]
    fn filters_become_criteria_and_impossible_ones_match_nothing() {
        let id = Uuid::new_v4();
        assert_eq!(criteria(None), Some(Vec::new()));
        assert_eq!(
            criteria(Some(&Filter::Equals {
                attribute: Attribute::Id,
                value: id.to_string(),
            })),
            Some(vec![Criterion::Id(id)])
        );
        assert_eq!(
            criteria(Some(&Filter::And(vec![
                Filter::Equals {
                    attribute: Attribute::UserName,
                    value: "ada".to_owned(),
                },
                Filter::Equals {
                    attribute: Attribute::ExternalId,
                    value: "00u1".to_owned(),
                },
            ]))),
            Some(vec![
                Criterion::UserName("ada".to_owned()),
                Criterion::ExternalId("00u1".to_owned()),
            ])
        );
        assert_eq!(
            criteria(Some(&Filter::Equals {
                attribute: Attribute::Id,
                value: "jyoung".to_owned(),
            })),
            None
        );
        assert_eq!(criteria(Some(&Filter::Member(id.to_string()))), None);
    }

    #[test]
    fn the_account_is_named_by_its_display_name_then_its_name() {
        assert_eq!(
            account_name(&UserChanges {
                display_name: Some(Some("Ada L".to_owned())),
                given_name: Some(Some("Ada".to_owned())),
                ..UserChanges::default()
            })
            .as_deref(),
            Some("Ada L")
        );
        assert_eq!(
            account_name(&UserChanges {
                display_name: Some(None),
                given_name: Some(Some("Ada".to_owned())),
                family_name: Some(Some("Lovelace".to_owned())),
                ..UserChanges::default()
            })
            .as_deref(),
            Some("Ada Lovelace")
        );
        assert_eq!(account_name(&UserChanges::default()), None);
    }

    #[test]
    fn only_the_attributes_that_changed_are_described() {
        let described = described(&UserChanges {
            user_name: Some("ada".to_owned()),
            external_id: Some(None),
            ..UserChanges::default()
        });
        assert_eq!(
            Value::Object(described),
            json!({ "user_name": "ada", "external_id": null })
        );
    }
}
