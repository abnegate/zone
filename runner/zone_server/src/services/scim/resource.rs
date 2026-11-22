use uuid::Uuid;
use zone_enterprise::scim::{Email, GroupResource, MemberReference, Meta, Name, UserResource, urn};

use crate::db::scim::{GroupRecord, Member};
use crate::state::AppState;

pub const USERS: &str = "Users";
pub const GROUPS: &str = "Groups";

pub fn base(state: &AppState, organization_id: Uuid) -> String {
    crate::routes::scim::base_url(&state.config().app_base_url, organization_id)
}

pub fn location(base: &str, collection: &str, id: Uuid) -> String {
    format!("{base}/{collection}/{id}")
}

pub fn user(base: &str, member: &Member) -> UserResource {
    let link = member.link.as_ref();
    let name = Name {
        given_name: link.and_then(|link| link.given_name.clone()),
        family_name: link.and_then(|link| link.family_name.clone()),
        formatted: None,
    };
    let email = link
        .and_then(|link| link.email.clone())
        .unwrap_or_else(|| member.email.clone());
    UserResource {
        schemas: vec![urn::USER],
        id: member.user_id,
        external_id: link.and_then(|link| link.external_id.clone()),
        user_name: link.map_or_else(|| member.email.clone(), |link| link.user_name.clone()),
        name: (!name.is_empty()).then_some(name),
        display_name: link
            .and_then(|link| link.display_name.clone())
            .or_else(|| member.display_name.clone()),
        emails: vec![Email::work(email)],
        active: member.active,
        groups: member
            .groups
            .iter()
            .map(|group| MemberReference {
                value: group.id.to_string(),
                display: Some(group.display_name.clone()),
                reference: Some(location(base, GROUPS, group.id)),
            })
            .collect(),
        meta: Meta::user(
            member.created_at,
            member.updated_at,
            location(base, USERS, member.user_id),
        ),
    }
}

pub fn member(base: &str, user_id: Uuid, display: String) -> MemberReference {
    MemberReference {
        value: user_id.to_string(),
        display: Some(display),
        reference: Some(location(base, USERS, user_id)),
    }
}

pub fn group(
    base: &str,
    record: &GroupRecord,
    members: Option<Vec<MemberReference>>,
) -> GroupResource {
    GroupResource {
        schemas: vec![urn::GROUP],
        id: record.id,
        external_id: record.external_id.clone(),
        display_name: record.display_name.clone(),
        members,
        meta: Meta::group(
            record.created_at,
            record.updated_at,
            location(base, GROUPS, record.id),
        ),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::db::organization_members::OrgRole;
    use crate::db::scim::{GroupReference, Link, Mapping};

    const BASE: &str = "https://zone.example/api/scim/v2/organizations/org";

    fn member(link: Option<Link>) -> Member {
        let now = Utc::now();
        Member {
            user_id: Uuid::new_v4(),
            email: "ada@example.com".to_owned(),
            display_name: Some("Ada".to_owned()),
            active: true,
            role: OrgRole::Member,
            link,
            groups: vec![GroupReference {
                id: Uuid::new_v4(),
                display_name: "Engineering".to_owned(),
            }],
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn an_unlinked_member_is_named_by_their_account() {
        let member = member(None);
        let resource = user(BASE, &member);
        assert_eq!(resource.user_name, "ada@example.com");
        assert_eq!(resource.display_name.as_deref(), Some("Ada"));
        assert_eq!(resource.emails, vec![Email::work("ada@example.com")]);
        assert!(resource.name.is_none());
        assert!(resource.external_id.is_none());
        assert_eq!(
            resource.meta.location,
            format!("{BASE}/Users/{}", member.user_id)
        );
        assert_eq!(resource.groups.len(), 1);
        assert_eq!(
            resource.groups[0].reference.as_deref(),
            Some(format!("{BASE}/Groups/{}", member.groups[0].id).as_str())
        );
    }

    #[test]
    fn a_linked_member_is_named_by_what_scim_pushed() {
        let mut link = Link::new(Uuid::new_v4(), Uuid::new_v4(), "ada.lovelace");
        link.external_id = Some("00u1".to_owned());
        link.given_name = Some("Ada".to_owned());
        link.family_name = Some("Lovelace".to_owned());
        link.display_name = Some("Ada Lovelace".to_owned());
        link.email = Some("ada.lovelace@example.com".to_owned());
        let resource = user(BASE, &member(Some(link)));
        assert_eq!(resource.user_name, "ada.lovelace");
        assert_eq!(resource.external_id.as_deref(), Some("00u1"));
        assert_eq!(resource.display_name.as_deref(), Some("Ada Lovelace"));
        assert_eq!(
            resource.emails,
            vec![Email::work("ada.lovelace@example.com")]
        );
        let name = resource.name.unwrap();
        assert_eq!(name.given_name.as_deref(), Some("Ada"));
        assert_eq!(name.family_name.as_deref(), Some("Lovelace"));
    }

    #[test]
    fn a_group_lists_its_members_only_when_asked() {
        let now = Utc::now();
        let record = GroupRecord {
            id: Uuid::new_v4(),
            organization_id: Uuid::new_v4(),
            external_id: None,
            display_name: "Engineering".to_owned(),
            mapping: Mapping::Unmapped,
            members: 1,
            created_at: now,
            updated_at: now,
        };
        assert!(group(BASE, &record, None).members.is_none());
        let user_id = Uuid::new_v4();
        let listed = group(
            BASE,
            &record,
            Some(vec![super::member(BASE, user_id, "Ada".to_owned())]),
        );
        let members = listed.members.unwrap();
        assert_eq!(members[0].value, user_id.to_string());
        assert_eq!(
            members[0].reference.as_deref(),
            Some(format!("{BASE}/Users/{user_id}").as_str())
        );
    }
}
