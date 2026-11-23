use abnegate_secret::REDACTED;
use chrono::Utc;
use uuid::Uuid;

use super::groups::Row;
use super::precedence::choose;
use super::{Candidate, Link, Mapping, NewToken, TokenCredential};
use crate::auth::{Grant, Permission, Scope};
use crate::db::custom_roles::Summary;
use crate::db::organization_members::OrgRole;

const HASH: &str = "4f2d0c1b-stored-hash";

fn custom(name: &str, id: u128, permissions: &[Permission]) -> Candidate {
    let names: Vec<&str> = permissions
        .iter()
        .map(|permission| permission.name())
        .collect();
    Candidate::Custom {
        role: Summary {
            id: Uuid::from_u128(id),
            name: name.to_owned(),
        },
        grant: Grant::stored(Scope::Organization, names),
    }
}

fn group_row(role: Option<&str>, custom_role_id: Option<Uuid>, name: Option<&str>) -> Row {
    Row {
        id: Uuid::nil(),
        organization_id: Uuid::nil(),
        external_id: None,
        display_name: "Engineering".to_owned(),
        role: role.map(str::to_owned),
        custom_role_id,
        custom_role_name: name.map(str::to_owned),
        members: 2,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[test]
fn nothing_is_chosen_from_no_candidates() {
    assert_eq!(choose(&[]), None);
}

#[test]
fn the_larger_grant_wins() {
    let auditor = custom("Auditor", 1, &[Permission::AuditLogsRead]);
    let operator = custom(
        "Operator",
        2,
        &[Permission::AuditLogsRead, Permission::DevicesRead],
    );
    let candidates = [
        Candidate::BuiltIn(OrgRole::Member),
        auditor,
        operator.clone(),
    ];
    assert_eq!(choose(&candidates), Some(&operator));
    let candidates = [
        operator,
        Candidate::BuiltIn(OrgRole::Admin),
        Candidate::BuiltIn(OrgRole::Member),
    ];
    assert_eq!(
        choose(&candidates),
        Some(&Candidate::BuiltIn(OrgRole::Admin))
    );
}

#[test]
fn admin_outranks_every_custom_role_and_every_custom_role_outranks_member() {
    let composable: Vec<Permission> = Permission::ALL
        .into_iter()
        .filter(|permission| permission.scope() == Scope::Organization && permission.composable())
        .collect();
    let widest = custom("Widest", 1, &composable);
    let candidates = [widest.clone(), Candidate::BuiltIn(OrgRole::Admin)];
    assert_eq!(
        choose(&candidates),
        Some(&Candidate::BuiltIn(OrgRole::Admin))
    );
    let narrowest = custom("Narrowest", 2, &[Permission::DevicesRead]);
    let candidates = [Candidate::BuiltIn(OrgRole::Member), narrowest.clone()];
    assert_eq!(choose(&candidates), Some(&narrowest));
}

#[test]
fn a_built_in_role_beats_a_custom_role_with_as_many_permissions() {
    let base = custom("Base", 1, &[]);
    assert_eq!(
        base.permissions(),
        Candidate::BuiltIn(OrgRole::Member).permissions()
    );
    let candidates = [base, Candidate::BuiltIn(OrgRole::Member)];
    assert_eq!(
        choose(&candidates),
        Some(&Candidate::BuiltIn(OrgRole::Member))
    );
}

#[test]
fn a_tie_falls_to_the_lower_name_ignoring_case_then_the_lower_id() {
    let zebra = custom("zebra", 1, &[Permission::DevicesRead]);
    let apple = custom("Apple", 9, &[Permission::AuditLogsRead]);
    assert_eq!(choose(&[zebra.clone(), apple.clone()]), Some(&apple));
    assert_eq!(choose(&[apple.clone(), zebra]), Some(&apple));

    let first = custom("Twin", 3, &[Permission::DevicesRead]);
    let second = custom("twin", 4, &[Permission::AuditLogsRead]);
    assert_eq!(choose(&[second.clone(), first.clone()]), Some(&first));
    assert_eq!(choose(&[first.clone(), second]), Some(&first));
}

#[test]
fn a_candidate_assigns_what_it_names() {
    let admin = Candidate::BuiltIn(OrgRole::Admin).membership();
    assert_eq!(admin.role, OrgRole::Admin);
    assert_eq!(admin.custom_role, None);
    assert_eq!(admin.grant, Grant::organization(OrgRole::Admin));

    let auditor = custom("Auditor", 7, &[Permission::AuditLogsRead]).membership();
    assert_eq!(auditor.role, OrgRole::Member);
    assert_eq!(
        auditor.custom_role.map(|summary| summary.id),
        Some(Uuid::from_u128(7))
    );
    assert!(auditor.grant.allows(Permission::AuditLogsRead));
    assert!(auditor.grant.allows(Permission::OrganizationRead));
}

#[test]
fn a_group_row_decodes_its_mapping() {
    let role = Uuid::new_v4();
    let cases = [
        (group_row(None, None, None), Mapping::Unmapped),
        (
            group_row(Some("admin"), None, None),
            Mapping::BuiltIn(OrgRole::Admin),
        ),
        (
            group_row(Some("member"), None, None),
            Mapping::BuiltIn(OrgRole::Member),
        ),
        (
            group_row(None, Some(role), Some("Auditor")),
            Mapping::Custom(Summary {
                id: role,
                name: "Auditor".to_owned(),
            }),
        ),
        (group_row(None, Some(role), None), Mapping::Unmapped),
    ];
    for (row, expected) in cases {
        let record = row.record().expect("the group decodes");
        assert_eq!(record.mapping, expected);
        assert_eq!(record.members, 2);
    }
    assert!(!Mapping::Unmapped.is_mapped());
    assert!(Mapping::BuiltIn(OrgRole::Member).is_mapped());
}

#[test]
fn a_group_row_that_maps_to_owner_or_to_two_roles_is_refused() {
    for row in [
        group_row(Some("owner"), None, None),
        group_row(Some("planet"), None, None),
        group_row(Some("admin"), Some(Uuid::new_v4()), Some("Auditor")),
    ] {
        let error = row.record().expect_err("the mapping is refused");
        assert!(
            matches!(&error, sqlx::Error::ColumnDecode { index, .. } if index == "role"),
            "{error:?}"
        );
    }
}

#[test]
fn debug_never_shows_a_token_hash_or_a_provisioned_password() {
    let credential = TokenCredential {
        id: Uuid::nil(),
        name: "Okta".to_owned(),
        token_hash: HASH.to_owned(),
    };
    let new = NewToken {
        organization_id: Uuid::nil(),
        name: "Okta",
        token_hash: HASH,
        hint: "abcd",
        created_by: None,
    };
    let link = Link {
        provisioned_password: Some(HASH.to_owned()),
        ..Link::new(Uuid::nil(), Uuid::nil(), "ada@acme.test")
    };
    for shown in [
        format!("{credential:?}"),
        format!("{new:?}"),
        format!("{link:?}"),
    ] {
        assert!(!shown.contains(HASH), "{shown}");
        assert!(shown.contains(REDACTED), "{shown}");
    }
    assert!(format!("{link:?}").contains("ada@acme.test"));
}

#[test]
fn a_new_link_carries_only_its_user_name() {
    let organization = Uuid::new_v4();
    let user = Uuid::new_v4();
    let link = Link::new(organization, user, "ada@acme.test");
    assert_eq!(link.organization_id, organization);
    assert_eq!(link.user_id, user);
    assert_eq!(link.user_name, "ada@acme.test");
    assert_eq!(link.external_id, None);
    assert_eq!(link.email, None);
    assert_eq!(link.provisioned_password, None);
}
