use chrono::{TimeZone, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::scim::{
    Attribute, Email, Error, Failure, Filter, Flag, GroupChanges, GroupDocument, GroupResource,
    ListQuery, ListResponse, MemberChanges, MemberReference, Meta, Name, Op, Page, PatchRequest,
    ScimType, Target, UserChanges, UserDocument, UserResource, discovery, urn,
};

const USER: &str = "2819c223-7f76-453a-919d-413861904646";
const GROUP: &str = "e9e30dba-f08f-4109-8486-d5c6a331660a";
const USER_NAME: &str = "bjensen@example.com";
const EXTERNAL_ID: &str = "00ujl29u0le5T6Aj10h7";
const OKTA_STRANGER: &str = "89bb1940-b905-4575-9e7f-6f887cfb368e";

const OKTA_CREATE_USER: &str = include_str!("fixtures/okta/create_user.json");
const OKTA_REPLACE_USER: &str = include_str!("fixtures/okta/replace_user.json");
const OKTA_DEACTIVATE_USER: &str = include_str!("fixtures/okta/deactivate_user.json");
const OKTA_REACTIVATE_USER: &str = include_str!("fixtures/okta/reactivate_user.json");
const OKTA_CREATE_GROUP: &str = include_str!("fixtures/okta/create_group.json");
const OKTA_ADD_MEMBERS: &str = include_str!("fixtures/okta/add_members.json");
const OKTA_REMOVE_MEMBER: &str = include_str!("fixtures/okta/remove_member.json");
const OKTA_RENAME_GROUP: &str = include_str!("fixtures/okta/rename_group.json");
const OKTA_REPLACE_MEMBERS: &str = include_str!("fixtures/okta/replace_members.json");
const OKTA_REPLACE_GROUP: &str = include_str!("fixtures/okta/replace_group.json");

const ENTRA_CREATE_USER: &str = include_str!("fixtures/entra/create_user.json");
const ENTRA_UPDATE_USER: &str = include_str!("fixtures/entra/update_user.json");
const ENTRA_DEACTIVATE_USER: &str = include_str!("fixtures/entra/deactivate_user.json");
const ENTRA_DEACTIVATE_USER_COMPLIANT: &str =
    include_str!("fixtures/entra/deactivate_user_compliant.json");
const ENTRA_PATHLESS_REPLACE: &str = include_str!("fixtures/entra/pathless_replace.json");
const ENTRA_REPLACE_ATTRIBUTES: &str = include_str!("fixtures/entra/replace_attributes.json");
const ENTRA_REPLACE_USER_NAME: &str = include_str!("fixtures/entra/replace_user_name.json");
const ENTRA_ADD_ATTRIBUTE: &str = include_str!("fixtures/entra/add_attribute.json");
const ENTRA_CREATE_GROUP: &str = include_str!("fixtures/entra/create_group.json");
const ENTRA_ADD_MEMBERS: &str = include_str!("fixtures/entra/add_members.json");
const ENTRA_REMOVE_MEMBERS: &str = include_str!("fixtures/entra/remove_members.json");
const ENTRA_REMOVE_MEMBER_PATH: &str = include_str!("fixtures/entra/remove_member_path.json");
const ENTRA_RENAME_GROUP: &str = include_str!("fixtures/entra/rename_group.json");
const ENTRA_FILTERS: &str = include_str!("fixtures/entra/filters.txt");

const JSON_FIXTURES: [&str; 23] = [
    OKTA_CREATE_USER,
    OKTA_REPLACE_USER,
    OKTA_DEACTIVATE_USER,
    OKTA_REACTIVATE_USER,
    OKTA_CREATE_GROUP,
    OKTA_ADD_MEMBERS,
    OKTA_REMOVE_MEMBER,
    OKTA_RENAME_GROUP,
    OKTA_REPLACE_MEMBERS,
    OKTA_REPLACE_GROUP,
    ENTRA_CREATE_USER,
    ENTRA_UPDATE_USER,
    ENTRA_DEACTIVATE_USER,
    ENTRA_DEACTIVATE_USER_COMPLIANT,
    ENTRA_PATHLESS_REPLACE,
    ENTRA_REPLACE_ATTRIBUTES,
    ENTRA_REPLACE_USER_NAME,
    ENTRA_ADD_ATTRIBUTE,
    ENTRA_CREATE_GROUP,
    ENTRA_ADD_MEMBERS,
    ENTRA_REMOVE_MEMBERS,
    ENTRA_REMOVE_MEMBER_PATH,
    ENTRA_RENAME_GROUP,
];

fn substituted(fixture: &str) -> String {
    fixture
        .replace("{{user}}", USER)
        .replace("{{group}}", GROUP)
        .replace("{{user_name}}", USER_NAME)
        .replace("{{external_id}}", EXTERNAL_ID)
}

fn user_document(fixture: &str) -> UserDocument {
    serde_json::from_str(&substituted(fixture)).expect("the fixture is a user document")
}

fn group_document(fixture: &str) -> GroupDocument {
    serde_json::from_str(&substituted(fixture)).expect("the fixture is a group document")
}

fn patch(fixture: &str) -> PatchRequest {
    serde_json::from_str(&substituted(fixture)).expect("the fixture is a patch request")
}

fn patch_of(body: Value) -> PatchRequest {
    serde_json::from_value(body).expect("the body is a patch request")
}

fn user_patch(body: Value) -> Result<UserChanges, Error> {
    UserChanges::from_patch(&patch_of(body))
}

fn group_patch(body: Value) -> Result<GroupChanges, Error> {
    GroupChanges::from_patch(&patch_of(body))
}

fn operations(operations: Value) -> Value {
    json!({ "schemas": [urn::PATCH_OP], "Operations": operations })
}

fn some(value: &str) -> Option<Option<String>> {
    Some(Some(value.to_owned()))
}

fn members(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn equals(attribute: Attribute, value: &str) -> Filter {
    Filter::Equals {
        attribute,
        value: value.to_owned(),
    }
}

fn refused_filter(input: &str) -> String {
    match Filter::parse(input) {
        Err(Error::InvalidFilter(detail)) => detail,
        other => panic!("{input} should be refused as an invalid filter, got {other:?}"),
    }
}

fn query(line: &str) -> ListQuery {
    let (_, query) = line.split_once('?').unwrap_or(("", line));
    let mut parsed = ListQuery::default();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').expect("each parameter has a value");
        let value = Some(value.to_owned());
        match key {
            "filter" => parsed.filter = value,
            "attributes" => parsed.attributes = value,
            "excludedAttributes" => parsed.excluded_attributes = value,
            other => panic!("unexpected parameter {other}"),
        }
    }
    parsed
}

#[test]
fn every_fixture_is_json_with_placeholders_only_inside_strings() {
    for fixture in JSON_FIXTURES {
        serde_json::from_str::<Value>(fixture).expect("the raw fixture parses");
        serde_json::from_str::<Value>(&substituted(fixture))
            .expect("the substituted fixture parses");
    }
}

#[test]
fn okta_create_user_becomes_a_full_change_set() {
    let document = user_document(OKTA_CREATE_USER);
    assert_eq!(document.validate(), Ok(()));
    assert_eq!(document.primary_email(), Some(USER_NAME));
    assert_eq!(
        UserChanges::from_document(&document),
        UserChanges {
            user_name: Some(USER_NAME.to_owned()),
            external_id: some(EXTERNAL_ID),
            display_name: some("Test User"),
            given_name: some("Test"),
            family_name: some("User"),
            email: Some(USER_NAME.to_owned()),
            active: Some(true),
        }
    );
}

#[test]
fn okta_replace_user_clears_what_the_body_leaves_out() {
    let changes = UserChanges::from_document(&user_document(OKTA_REPLACE_USER));
    assert_eq!(
        changes,
        UserChanges {
            user_name: Some(USER_NAME.to_owned()),
            external_id: Some(None),
            display_name: Some(None),
            given_name: some("Another"),
            family_name: some("User"),
            email: Some(USER_NAME.to_owned()),
            active: Some(true),
        }
    );
}

#[test]
fn okta_deactivation_sets_only_active_false() {
    let changes = UserChanges::from_patch(&patch(OKTA_DEACTIVATE_USER)).unwrap();
    assert_eq!(
        changes,
        UserChanges {
            active: Some(false),
            ..UserChanges::default()
        }
    );
    assert!(changes.deactivates());
    assert_eq!(changes.only_deactivation(), changes);
}

#[test]
fn okta_reactivation_sets_only_active_true() {
    let changes = UserChanges::from_patch(&patch(OKTA_REACTIVATE_USER)).unwrap();
    assert_eq!(
        changes,
        UserChanges {
            active: Some(true),
            ..UserChanges::default()
        }
    );
    assert!(!changes.deactivates());
    assert!(changes.only_deactivation().is_empty());
}

#[test]
fn okta_create_group_replaces_members_with_none() {
    let document = group_document(OKTA_CREATE_GROUP);
    assert_eq!(document.validate(), Ok(()));
    assert_eq!(
        GroupChanges::from_document(&document),
        GroupChanges {
            display_name: Some("Test SCIMv2".to_owned()),
            external_id: Some(None),
            members: MemberChanges {
                replace: Some(Vec::new()),
                ..MemberChanges::default()
            },
        }
    );
}

#[test]
fn okta_membership_update_removes_by_filter_and_adds_by_list() {
    let changes = GroupChanges::from_patch(&patch(OKTA_ADD_MEMBERS)).unwrap();
    assert_eq!(
        changes,
        GroupChanges {
            members: MemberChanges {
                add: members(&[USER]),
                remove: members(&[OKTA_STRANGER]),
                ..MemberChanges::default()
            },
            ..GroupChanges::default()
        }
    );
}

#[test]
fn okta_remove_member_removes_the_filtered_member() {
    let changes = GroupChanges::from_patch(&patch(OKTA_REMOVE_MEMBER)).unwrap();
    assert_eq!(changes.members.remove, members(&[USER]));
    assert!(changes.members.add.is_empty() && changes.members.replace.is_none());
    assert_eq!(changes.display_name, None);
}

#[test]
fn okta_rename_ignores_the_echoed_id() {
    let changes = GroupChanges::from_patch(&patch(OKTA_RENAME_GROUP)).unwrap();
    assert_eq!(
        changes,
        GroupChanges {
            display_name: Some("Test SCIMv2".to_owned()),
            ..GroupChanges::default()
        }
    );
}

#[test]
fn okta_full_push_replaces_the_membership() {
    let changes = GroupChanges::from_patch(&patch(OKTA_REPLACE_MEMBERS)).unwrap();
    assert_eq!(
        changes.members.replace,
        Some(members(&[USER, OKTA_STRANGER]))
    );
    assert!(changes.members.add.is_empty() && changes.members.remove.is_empty());
}

#[test]
fn okta_group_put_replaces_the_membership() {
    let changes = GroupChanges::from_document(&group_document(OKTA_REPLACE_GROUP));
    assert_eq!(changes.display_name.as_deref(), Some("Test SCIMv2"));
    assert_eq!(changes.members.replace, Some(members(&[USER])));
}

#[test]
fn entra_create_user_takes_the_name_and_work_email() {
    let document = user_document(ENTRA_CREATE_USER);
    assert_eq!(
        UserChanges::from_document(&document),
        UserChanges {
            user_name: Some(USER_NAME.to_owned()),
            external_id: some(EXTERNAL_ID),
            display_name: Some(None),
            given_name: some("givenName"),
            family_name: some("familyName"),
            email: Some(USER_NAME.to_owned()),
            active: Some(true),
        }
    );
}

#[test]
fn entra_update_reads_the_filtered_email_and_dotted_name_paths() {
    let changes = UserChanges::from_patch(&patch(ENTRA_UPDATE_USER)).unwrap();
    assert_eq!(
        changes,
        UserChanges {
            email: Some("updatedEmail@microsoft.com".to_owned()),
            family_name: some("updatedFamilyName"),
            ..UserChanges::default()
        }
    );
}

#[test]
fn entra_deactivation_accepts_the_false_string() {
    for fixture in [ENTRA_DEACTIVATE_USER, ENTRA_DEACTIVATE_USER_COMPLIANT] {
        let changes = UserChanges::from_patch(&patch(fixture)).unwrap();
        assert_eq!(
            changes,
            UserChanges {
                active: Some(false),
                ..UserChanges::default()
            }
        );
    }
}

#[test]
fn entra_pathless_replace_reads_dotted_keys_and_ignores_the_extension() {
    let changes = UserChanges::from_patch(&patch(ENTRA_PATHLESS_REPLACE)).unwrap();
    assert_eq!(
        changes,
        UserChanges {
            email: Some("TestMhvaes@test.microsoft.com".to_owned()),
            display_name: some("Bjfe"),
            given_name: some("Kkom"),
            family_name: some("Unua"),
            ..UserChanges::default()
        }
    );
}

#[test]
fn entra_replace_of_many_attributes_ignores_the_extension_path() {
    let changes = UserChanges::from_patch(&patch(ENTRA_REPLACE_ATTRIBUTES)).unwrap();
    assert_eq!(
        changes,
        UserChanges {
            display_name: some("Pvlo"),
            email: Some("TestBcwqnm@test.microsoft.com".to_owned()),
            given_name: some("Gtfd"),
            family_name: some("Pkqf"),
            external_id: some("Eqpj"),
            ..UserChanges::default()
        }
    );
}

#[test]
fn entra_user_name_replace_changes_the_user_name() {
    let changes = UserChanges::from_patch(&patch(ENTRA_REPLACE_USER_NAME)).unwrap();
    assert_eq!(
        changes,
        UserChanges {
            user_name: Some(USER_NAME.to_owned()),
            ..UserChanges::default()
        }
    );
}

#[test]
fn entra_add_of_an_unstored_attribute_changes_nothing() {
    let changes = UserChanges::from_patch(&patch(ENTRA_ADD_ATTRIBUTE)).unwrap();
    assert!(changes.is_empty());
}

#[test]
fn entra_create_group_keeps_its_external_id_and_leaves_members_alone() {
    let changes = GroupChanges::from_document(&group_document(ENTRA_CREATE_GROUP));
    assert_eq!(
        changes,
        GroupChanges {
            display_name: Some("displayName".to_owned()),
            external_id: some("8aa1a0c0-c4c3-4bc0-b4a5-2ef676900159"),
            members: MemberChanges::default(),
        }
    );
}

#[test]
fn entra_add_members_adds_by_value() {
    let changes = GroupChanges::from_patch(&patch(ENTRA_ADD_MEMBERS)).unwrap();
    assert_eq!(changes.members.add, members(&[USER]));
    assert!(changes.members.remove.is_empty());
}

#[test]
fn entra_both_member_remove_styles_remove_the_member() {
    for fixture in [ENTRA_REMOVE_MEMBERS, ENTRA_REMOVE_MEMBER_PATH] {
        let changes = GroupChanges::from_patch(&patch(fixture)).unwrap();
        assert_eq!(
            changes.members,
            MemberChanges {
                remove: members(&[USER]),
                ..MemberChanges::default()
            }
        );
    }
}

#[test]
fn entra_rename_sets_the_display_name() {
    let changes = GroupChanges::from_patch(&patch(ENTRA_RENAME_GROUP)).unwrap();
    assert_eq!(
        changes,
        GroupChanges {
            display_name: Some("1879db59-3bdf-4490-ad68-ab880a269474updatedDisplayName".to_owned()),
            ..GroupChanges::default()
        }
    );
}

#[test]
fn every_entra_query_parses() {
    let lines: Vec<String> = ENTRA_FILTERS.lines().map(substituted).collect();
    let parsed: Vec<(Option<Filter>, bool)> = lines
        .iter()
        .map(|line| {
            let query = query(line);
            (query.parsed_filter().expect(line), query.excludes_members())
        })
        .collect();
    let membership = Filter::And(vec![
        equals(Attribute::Id, GROUP),
        Filter::Member(USER.to_owned()),
    ]);
    assert_eq!(
        parsed,
        [
            (Some(equals(Attribute::UserName, USER_NAME)), false),
            (
                Some(equals(Attribute::UserName, "non-existent user")),
                false
            ),
            (Some(equals(Attribute::ExternalId, EXTERNAL_ID)), false),
            (None, true),
            (Some(equals(Attribute::DisplayName, "displayName")), true),
            (Some(membership.clone()), true),
            (Some(membership), true),
        ]
    );
}

#[test]
fn filters_accept_eq_and_and_in_any_case() {
    assert_eq!(
        Filter::parse("userName eq \"bjensen\""),
        Ok(equals(Attribute::UserName, "bjensen"))
    );
    assert_eq!(
        Filter::parse("USERNAME EQ \"bjensen\""),
        Ok(equals(Attribute::UserName, "bjensen"))
    );
    assert_eq!(
        Filter::parse("externalId eq \"a\" AND displayName Eq \"b\" and id eq \"c\""),
        Ok(Filter::And(vec![
            equals(Attribute::ExternalId, "a"),
            equals(Attribute::DisplayName, "b"),
            equals(Attribute::Id, "c"),
        ]))
    );
}

#[test]
fn filters_accept_urn_qualified_attributes() {
    assert_eq!(
        Filter::parse("urn:ietf:params:scim:schemas:core:2.0:User:userName eq \"bjensen\""),
        Ok(equals(Attribute::UserName, "bjensen"))
    );
    assert_eq!(
        Filter::parse("urn:ietf:params:scim:schemas:core:2.0:Group:displayName eq \"Admins\""),
        Ok(equals(Attribute::DisplayName, "Admins"))
    );
}

#[test]
fn filters_accept_every_member_form() {
    for input in [
        "members[value eq \"u\"]",
        "members eq \"u\"",
        "members.value eq \"u\"",
        "Members[Value EQ \"u\"]",
        "members eq u",
    ] {
        assert_eq!(
            Filter::parse(input),
            Ok(Filter::Member("u".to_owned())),
            "{input}"
        );
    }
    assert_eq!(
        Filter::parse("id eq \"g\" and members[value eq \"u\"]"),
        Ok(Filter::And(vec![
            equals(Attribute::Id, "g"),
            Filter::Member("u".to_owned())
        ]))
    );
}

#[test]
fn filter_strings_unescape_quotes_and_backslashes() {
    assert_eq!(
        Filter::parse(r#"userName eq "a\"b\\c""#),
        Ok(equals(Attribute::UserName, "a\"b\\c"))
    );
}

#[test]
fn injection_shaped_values_stay_literal() {
    assert_eq!(
        Filter::parse("userName eq \"x' OR 1=1 --\""),
        Ok(equals(Attribute::UserName, "x' OR 1=1 --"))
    );
    assert_eq!(
        Filter::parse(r#"userName eq "x\" or userName eq \"y""#),
        Ok(equals(Attribute::UserName, "x\" or userName eq \"y"))
    );
    assert_eq!(
        Filter::parse("displayName eq \"'; DROP TABLE users; --\""),
        Ok(equals(Attribute::DisplayName, "'; DROP TABLE users; --"))
    );
}

#[test]
fn filters_refuse_what_zone_does_not_support() {
    for input in [
        "userName eq \"a\" or userName eq \"b\"",
        "userName co \"a\"",
        "userName sw \"a\"",
        "userName pr",
        "userName ne \"a\"",
        "meta.lastModified gt \"2011-05-13T04:42:34Z\"",
        "not (userName eq \"a\")",
        "(userName eq \"a\")",
        "userName eq \"a\" and (displayName eq \"b\")",
        "emails eq \"a@example.com\"",
        "emails[type eq \"work\"].value eq \"a@example.com\"",
        "title eq \"Tour Guide\"",
        "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:employeeNumber eq \"1\"",
        "members[display eq \"a\"]",
        "members[value eq \"a\"",
        "",
        "   ",
        "userName",
        "userName eq",
        "userName eq \"a",
        "userName eq \"a\\x\"",
        "userName eq \"a\" \"b\"",
        "userName eq \"a\" and",
        "and userName eq \"a\"",
    ] {
        assert!(!refused_filter(input).is_empty(), "{input}");
    }
}

#[test]
fn a_refused_filter_answers_400_invalid_filter() {
    let error = Filter::parse("userName co \"a\"").unwrap_err();
    assert_eq!(error.status(), 400);
    assert_eq!(error.scim_type(), Some(ScimType::InvalidFilter));
    assert_eq!(
        error.to_string(),
        "The co operator isn't supported; only eq and and are."
    );
}

#[test]
fn flag_accepts_booleans_and_strings_in_any_case() {
    for (input, expected) in [
        (json!(true), true),
        (json!(false), false),
        (json!("true"), true),
        (json!("True"), true),
        (json!("False"), false),
        (json!("FALSE"), false),
    ] {
        assert_eq!(
            serde_json::from_value::<Flag>(input.clone())
                .map(bool::from)
                .ok(),
            Some(expected),
            "{input}"
        );
        assert_eq!(Flag::parse(&input), Some(Flag(expected)), "{input}");
    }
}

#[test]
fn flag_refuses_anything_else() {
    for input in [
        json!("yes"),
        json!(1),
        json!(0),
        json!(null),
        json!([]),
        json!({}),
    ] {
        assert!(
            serde_json::from_value::<Flag>(input.clone()).is_err(),
            "{input}"
        );
        assert_eq!(Flag::parse(&input), None, "{input}");
    }
}

#[test]
fn flag_serializes_as_a_boolean() {
    assert_eq!(serde_json::to_value(Flag(true)).unwrap(), json!(true));
}

#[test]
fn page_defaults_to_the_first_hundred() {
    let page = Page::from(&ListQuery::default());
    assert_eq!(page.start_index(), 1);
    assert_eq!(page.count(), Page::DEFAULT_COUNT);
    assert_eq!(page.offset(), 0);
    assert_eq!(page.limit(), 100);
}

#[test]
fn page_clamps_its_bounds() {
    let page = |start_index: Option<i64>, count: Option<i64>| {
        Page::from(&ListQuery {
            start_index,
            count,
            ..ListQuery::default()
        })
    };
    assert_eq!(page(Some(0), None).start_index(), 1);
    assert_eq!(page(Some(-5), None).offset(), 0);
    assert_eq!(page(Some(i64::MIN), None).start_index(), 1);
    assert_eq!(page(None, Some(5000)).limit(), 1000);
    assert_eq!(page(None, Some(i64::MAX)).count(), Page::MAX_RESULTS);
    assert_eq!(page(None, Some(0)).limit(), 0);
    assert_eq!(page(None, Some(-3)).limit(), 0);
    assert_eq!(page(Some(101), Some(100)).offset(), 100);
    assert_eq!(page(Some(i64::MAX), None).offset(), i64::MAX - 1);
}

#[test]
fn list_query_reads_camel_case_parameters() {
    let query: ListQuery = serde_json::from_value(json!({
        "filter": "userName eq \"a\"",
        "startIndex": 3,
        "count": 7,
        "attributes": "id",
        "excludedAttributes": "members",
    }))
    .unwrap();
    assert_eq!(query.start_index, Some(3));
    assert_eq!(query.count, Some(7));
    assert_eq!(
        query.parsed_filter(),
        Ok(Some(equals(Attribute::UserName, "a")))
    );
    assert_eq!(ListQuery::default().parsed_filter(), Ok(None));
}

#[test]
fn members_are_excluded_when_asked_or_when_other_attributes_are_requested() {
    let query = |attributes: Option<&str>, excluded: Option<&str>| ListQuery {
        attributes: attributes.map(str::to_owned),
        excluded_attributes: excluded.map(str::to_owned),
        ..ListQuery::default()
    };
    assert!(query(None, Some("members")).excludes_members());
    assert!(query(None, Some("displayName, Members")).excludes_members());
    assert!(
        query(
            None,
            Some("urn:ietf:params:scim:schemas:core:2.0:Group:members")
        )
        .excludes_members()
    );
    assert!(query(Some("id"), None).excludes_members());
    assert!(!query(Some("id,members"), None).excludes_members());
    assert!(!query(Some("members.value"), None).excludes_members());
    assert!(!query(None, None).excludes_members());
    assert!(!query(None, Some("displayName")).excludes_members());
    assert!(!query(Some(" "), None).excludes_members());
}

#[test]
fn only_deactivation_keeps_nothing_but_the_deactivation() {
    let changes = UserChanges {
        user_name: Some("new@example.com".to_owned()),
        display_name: some("New"),
        email: Some("new@example.com".to_owned()),
        active: Some(false),
        ..UserChanges::default()
    };
    assert_eq!(
        changes.only_deactivation(),
        UserChanges {
            active: Some(false),
            ..UserChanges::default()
        }
    );
    let reactivation = UserChanges {
        display_name: some("New"),
        active: Some(true),
        ..UserChanges::default()
    };
    assert!(reactivation.only_deactivation().is_empty());
    assert!(
        UserChanges {
            display_name: some("New"),
            ..UserChanges::default()
        }
        .only_deactivation()
        .is_empty()
    );
}

#[test]
fn a_failure_serializes_with_a_string_status() {
    let failure = Failure::new(400, Some(ScimType::InvalidFilter), "Bad filter.");
    assert_eq!(
        serde_json::to_value(&failure).unwrap(),
        json!({
            "schemas": ["urn:ietf:params:scim:api:messages:2.0:Error"],
            "status": "400",
            "scimType": "invalidFilter",
            "detail": "Bad filter.",
        })
    );
    assert_eq!(failure.status(), 400);
    assert_eq!(failure.scim_type(), Some(ScimType::InvalidFilter));
    assert_eq!(failure.detail(), "Bad filter.");
}

#[test]
fn a_failure_without_a_type_omits_it() {
    assert_eq!(
        serde_json::to_value(Failure::from(&Error::NotFound)).unwrap(),
        json!({
            "schemas": [urn::ERROR],
            "status": "404",
            "detail": "That resource doesn't exist.",
        })
    );
    assert_eq!(
        serde_json::to_value(Failure::new(429, Some(ScimType::TooMany), "Slow down.")).unwrap(),
        json!({
            "schemas": [urn::ERROR],
            "status": "429",
            "scimType": "tooMany",
            "detail": "Slow down.",
        })
    );
}

#[test]
fn errors_map_to_their_status_and_type() {
    let cases = [
        (
            Error::InvalidFilter("a".into()),
            400,
            Some(ScimType::InvalidFilter),
        ),
        (
            Error::InvalidPath("a".into()),
            400,
            Some(ScimType::InvalidPath),
        ),
        (
            Error::InvalidSyntax("a".into()),
            400,
            Some(ScimType::InvalidSyntax),
        ),
        (
            Error::InvalidValue("a".into()),
            400,
            Some(ScimType::InvalidValue),
        ),
        (Error::NoTarget("a".into()), 400, Some(ScimType::NoTarget)),
        (
            Error::Mutability("a".into()),
            400,
            Some(ScimType::Mutability),
        ),
        (
            Error::Uniqueness("a".into()),
            409,
            Some(ScimType::Uniqueness),
        ),
        (Error::NotFound, 404, None),
    ];
    for (error, status, scim_type) in cases {
        assert_eq!(error.status(), status, "{error:?}");
        assert_eq!(error.scim_type(), scim_type, "{error:?}");
        let failure = Failure::from(&error);
        assert_eq!(failure.status(), status);
        assert_eq!(failure.detail(), error.to_string());
    }
}

#[test]
fn scim_types_serialize_as_their_wire_names() {
    for scim_type in ScimType::ALL {
        assert_eq!(
            serde_json::to_value(scim_type).unwrap(),
            json!(scim_type.as_str())
        );
    }
    assert_eq!(ScimType::InvalidVers.as_str(), "invalidVers");
    assert_eq!(ScimType::NoTarget.as_str(), "noTarget");
}

#[test]
fn targets_parse_every_supported_path() {
    let cases = [
        ("active", Target::Active),
        ("userName", Target::UserName),
        ("USERNAME", Target::UserName),
        ("externalId", Target::ExternalId),
        ("displayName", Target::DisplayName),
        ("name", Target::Name),
        ("name.givenName", Target::GivenName),
        ("name.familyName", Target::FamilyName),
        ("name.formatted", Target::Formatted),
        ("name.middleName", Target::Ignored),
        ("emails", Target::Email),
        ("emails.value", Target::Email),
        ("emails[type eq \"work\"].value", Target::Email),
        ("emails[Type EQ \"Work\"].Value", Target::Email),
        ("emails[primary eq true].value", Target::Email),
        ("emails[type eq \"work\"]", Target::Email),
        ("emails[type eq \"home\"].value", Target::Ignored),
        ("emails[type eq \"work\"].primary", Target::Ignored),
        ("members", Target::Members),
        ("members[value eq \"u\"]", Target::Member("u".to_owned())),
        (
            "urn:ietf:params:scim:schemas:core:2.0:User:userName",
            Target::UserName,
        ),
        (
            "urn:ietf:params:scim:schemas:core:2.0:User:name.givenName",
            Target::GivenName,
        ),
        (
            "urn:ietf:params:scim:schemas:core:2.0:Group:displayName",
            Target::DisplayName,
        ),
        (
            "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:employeeNumber",
            Target::Ignored,
        ),
        (
            "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:manager",
            Target::Ignored,
        ),
        (
            "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User",
            Target::Ignored,
        ),
        ("nickName", Target::Ignored),
        ("title", Target::Ignored),
        ("phoneNumbers[type eq \"mobile\"].value", Target::Ignored),
        ("addresses[type eq \"work\"].locality", Target::Ignored),
        ("id", Target::Ignored),
    ];
    for (path, expected) in cases {
        assert_eq!(Target::parse(path), Ok(expected), "{path}");
    }
}

#[test]
fn targets_refuse_malformed_paths() {
    for path in [
        "",
        "emails[type eq \"work\"",
        "emails[type \"work\"].value",
        "emails[type eq \"work\"]value",
        "emails[type eq \"work\"].value.extra",
        "members[display eq \"u\"]",
        "members[value eq \"u\"].display",
        "active.value",
        "userName[value eq \"a\"]",
        "name[givenName eq \"a\"]",
        "name.givenName[value eq \"a\"]",
        "display Name",
        "[value eq \"a\"]",
        "emails[type eq \"wo\\qrk\"].value",
        "name.",
        ".name",
        "name.givenName.extra",
        "emails.value.extra",
        "emails[type eq \"work\"].",
        "members.",
        "   ",
    ] {
        assert!(
            matches!(Target::parse(path), Err(Error::InvalidPath(_))),
            "{path}: {:?}",
            Target::parse(path)
        );
    }
}

#[test]
fn ops_are_read_in_any_case() {
    for (input, expected) in [
        ("add", Op::Add),
        ("Add", Op::Add),
        ("REPLACE", Op::Replace),
        ("Remove", Op::Remove),
    ] {
        assert_eq!(
            serde_json::from_value::<Op>(json!(input)).unwrap(),
            expected
        );
    }
    assert!(serde_json::from_value::<Op>(json!("copy")).is_err());
    assert!(serde_json::from_value::<Op>(json!(1)).is_err());
}

#[test]
fn patch_requests_accept_lowercase_operations() {
    let request = patch_of(json!({
        "operations": [{ "op": "replace", "path": "active", "value": false }]
    }));
    assert_eq!(request.operations.len(), 1);
    assert!(request.schemas.is_empty());
    assert!(serde_json::from_value::<PatchRequest>(json!({ "schemas": [] })).is_err());
}

#[test]
fn user_patches_read_nested_values_and_urn_keys() {
    let changes = user_patch(operations(json!([{
        "op": "replace",
        "value": {
            "urn:ietf:params:scim:schemas:core:2.0:User:userName": "renamed@example.com",
            "name": { "givenName": "Barbara", "familyName": "Jensen", "middleName": "Jane" },
            "emails": [
                { "value": "home@example.com", "type": "home" },
                { "value": "work@example.com", "type": "work" }
            ],
            "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User": { "department": "Tour" },
            "urn:ietf:params:scim:schemas:core:2.0:User": { "displayName": "Babs" },
            "active": "true",
            "locale": "en-US"
        }
    }])))
    .unwrap();
    assert_eq!(
        changes,
        UserChanges {
            user_name: Some("renamed@example.com".to_owned()),
            display_name: some("Babs"),
            given_name: some("Barbara"),
            family_name: some("Jensen"),
            email: Some("work@example.com".to_owned()),
            active: Some(true),
            ..UserChanges::default()
        }
    );
}

#[test]
fn user_patches_clear_optional_attributes() {
    let changes = user_patch(operations(json!([
        { "op": "remove", "path": "externalId" },
        { "op": "replace", "path": "displayName", "value": null },
        { "op": "replace", "path": "name.givenName", "value": "" },
        { "op": "remove", "path": "name.familyName" },
        { "op": "remove", "path": "nickName" }
    ])))
    .unwrap();
    assert_eq!(
        changes,
        UserChanges {
            external_id: Some(None),
            display_name: Some(None),
            given_name: Some(None),
            family_name: Some(None),
            ..UserChanges::default()
        }
    );
}

#[test]
fn later_user_operations_win() {
    let changes = user_patch(operations(json!([
        { "op": "replace", "path": "active", "value": false },
        { "op": "replace", "path": "active", "value": true }
    ])))
    .unwrap();
    assert_eq!(changes.active, Some(true));
}

#[test]
fn user_patches_refuse_bad_operations() {
    let refusals = [
        (json!([{ "op": "remove" }]), ScimType::NoTarget),
        (
            json!([{ "op": "remove", "path": "userName" }]),
            ScimType::Mutability,
        ),
        (
            json!([{ "op": "remove", "path": "active" }]),
            ScimType::Mutability,
        ),
        (
            json!([{ "op": "remove", "path": "emails" }]),
            ScimType::Mutability,
        ),
        (
            json!([{ "op": "replace", "path": "active", "value": "maybe" }]),
            ScimType::InvalidValue,
        ),
        (
            json!([{ "op": "replace", "path": "active" }]),
            ScimType::InvalidValue,
        ),
        (
            json!([{ "op": "replace", "path": "userName", "value": "" }]),
            ScimType::InvalidValue,
        ),
        (
            json!([{ "op": "replace", "path": "userName", "value": 7 }]),
            ScimType::InvalidValue,
        ),
        (
            json!([{ "op": "replace", "path": "displayName", "value": ["a"] }]),
            ScimType::InvalidValue,
        ),
        (
            json!([{ "op": "replace", "path": "emails", "value": [] }]),
            ScimType::InvalidValue,
        ),
        (
            json!([{ "op": "replace", "path": "name", "value": "Babs" }]),
            ScimType::InvalidValue,
        ),
        (
            json!([{ "op": "replace", "value": "Babs" }]),
            ScimType::InvalidValue,
        ),
        (
            json!([{ "op": "replace", "value": { "bad key": 1 } }]),
            ScimType::InvalidPath,
        ),
        (
            json!([{ "op": "replace", "path": "emails[type eq", "value": "a" }]),
            ScimType::InvalidPath,
        ),
    ];
    for (body, scim_type) in refusals {
        let error = user_patch(operations(body.clone())).unwrap_err();
        assert_eq!(error.scim_type(), Some(scim_type), "{body}: {error:?}");
        assert_eq!(error.status(), 400, "{body}");
    }
}

#[test]
fn group_patches_read_pathless_members_and_external_id() {
    let changes = group_patch(operations(json!([{
        "op": "add",
        "value": {
            "members": [{ "value": "a" }, { "value": "b" }],
            "externalId": "ext"
        }
    }])))
    .unwrap();
    assert_eq!(changes.members.add, members(&["a", "b"]));
    assert_eq!(changes.external_id, some("ext"));
    let replaced = group_patch(operations(json!([{
        "op": "replace",
        "value": { "members": [{ "value": "a" }], "id": GROUP }
    }])))
    .unwrap();
    assert_eq!(replaced.members.replace, Some(members(&["a"])));
}

#[test]
fn removing_all_members_clears_the_group() {
    let changes = group_patch(operations(json!([{ "op": "remove", "path": "members" }]))).unwrap();
    assert_eq!(
        changes.members,
        MemberChanges {
            clear: true,
            ..MemberChanges::default()
        }
    );
    let with_null = group_patch(operations(
        json!([{ "op": "remove", "path": "members", "value": null }]),
    ))
    .unwrap();
    assert_eq!(with_null, changes);
    let cleared_then_added = group_patch(operations(json!([
        { "op": "remove", "path": "members" },
        { "op": "add", "path": "members", "value": [{ "value": "a" }] },
        { "op": "remove", "path": "members[value eq \"b\"]" }
    ])))
    .unwrap();
    assert_eq!(
        cleared_then_added.members,
        MemberChanges {
            clear: true,
            add: members(&["a"]),
            ..MemberChanges::default()
        }
    );
}

#[test]
fn member_operations_fold_in_order() {
    let changes = group_patch(operations(json!([
        { "op": "add", "path": "members", "value": [{ "value": "a" }, { "value": "b" }, { "value": "a" }] },
        { "op": "remove", "path": "members[value eq \"a\"]" },
        { "op": "remove", "path": "members", "value": [{ "value": "c" }] },
        { "op": "add", "path": "members", "value": { "value": "c" } }
    ])))
    .unwrap();
    assert_eq!(
        changes.members,
        MemberChanges {
            add: members(&["b", "c"]),
            remove: members(&["a"]),
            ..MemberChanges::default()
        }
    );
    let replaced = group_patch(operations(json!([
        { "op": "add", "path": "members", "value": [{ "value": "z" }] },
        { "op": "replace", "path": "members", "value": [{ "value": "a" }, { "value": "b" }] },
        { "op": "remove", "path": "members[value eq \"a\"]" },
        { "op": "add", "path": "members", "value": [{ "value": "c" }, { "value": "b" }] }
    ])))
    .unwrap();
    assert_eq!(
        replaced.members,
        MemberChanges {
            replace: Some(members(&["b", "c"])),
            ..MemberChanges::default()
        }
    );
}

#[test]
fn group_patches_refuse_bad_operations() {
    let refusals = [
        (json!([{ "op": "remove" }]), ScimType::NoTarget),
        (
            json!([{ "op": "remove", "path": "displayName" }]),
            ScimType::Mutability,
        ),
        (
            json!([{ "op": "replace", "path": "displayName", "value": " " }]),
            ScimType::InvalidValue,
        ),
        (
            json!([{ "op": "replace", "path": "externalId", "value": 3 }]),
            ScimType::InvalidValue,
        ),
        (
            json!([{ "op": "add", "path": "members", "value": [{ "display": "a" }] }]),
            ScimType::InvalidValue,
        ),
        (
            json!([{ "op": "add", "path": "members", "value": [7] }]),
            ScimType::InvalidValue,
        ),
        (
            json!([{ "op": "add", "path": "members", "value": true }]),
            ScimType::InvalidValue,
        ),
        (
            json!([{ "op": "add", "path": "members" }]),
            ScimType::InvalidValue,
        ),
        (
            json!([{ "op": "add", "path": "members", "value": null }]),
            ScimType::InvalidValue,
        ),
        (
            json!([{ "op": "add", "path": "members[value eq \"a\"]", "value": [] }]),
            ScimType::InvalidPath,
        ),
        (
            json!([{ "op": "add", "value": [] }]),
            ScimType::InvalidValue,
        ),
    ];
    for (body, scim_type) in refusals {
        let error = group_patch(operations(body.clone())).unwrap_err();
        assert_eq!(error.scim_type(), Some(scim_type), "{body}: {error:?}");
    }
}

#[test]
fn documents_are_lenient_about_unknown_and_null_fields() {
    let document: UserDocument = serde_json::from_value(json!({
        "userName": "bjensen",
        "emails": null,
        "name": null,
        "active": null,
        "phoneNumbers": [{ "value": "555", "type": "work" }],
        "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User": { "employeeNumber": "1" }
    }))
    .unwrap();
    assert_eq!(document.primary_email(), None);
    assert_eq!(document.active, None);
    assert!(serde_json::from_value::<UserDocument>(json!({ "emails": [] })).is_err());
    assert_eq!(
        serde_json::from_value::<UserDocument>(json!({ "userName": " " }))
            .unwrap()
            .validate()
            .unwrap_err()
            .scim_type(),
        Some(ScimType::InvalidValue)
    );
    let group: GroupDocument = serde_json::from_value(json!({
        "displayName": "Admins",
        "members": null
    }))
    .unwrap();
    assert_eq!(
        GroupChanges::from_document(&group).members,
        MemberChanges::default()
    );
    assert!(serde_json::from_value::<GroupDocument>(json!({ "members": [] })).is_err());
    assert_eq!(
        serde_json::from_value::<GroupDocument>(json!({ "displayName": "" }))
            .unwrap()
            .validate()
            .unwrap_err()
            .scim_type(),
        Some(ScimType::InvalidValue)
    );
}

#[test]
fn the_primary_email_prefers_primary_then_work_then_any() {
    let email = |value: &str, kind: Option<&str>, primary: Option<bool>| Email {
        value: value.to_owned(),
        kind: kind.map(str::to_owned),
        primary: primary.map(Flag),
    };
    let document = |emails: Vec<Email>| UserDocument {
        user_name: "bjensen".to_owned(),
        external_id: None,
        name: None,
        display_name: None,
        emails,
        active: None,
    };
    assert_eq!(
        document(vec![
            email("work@example.com", Some("work"), None),
            email("primary@example.com", Some("home"), Some(true)),
        ])
        .primary_email(),
        Some("primary@example.com")
    );
    assert_eq!(
        document(vec![
            email("home@example.com", Some("home"), Some(false)),
            email("work@example.com", Some("Work"), None),
        ])
        .primary_email(),
        Some("work@example.com")
    );
    assert_eq!(
        document(vec![
            email(" ", Some("work"), Some(true)),
            email("other@example.com", None, None),
        ])
        .primary_email(),
        Some("other@example.com")
    );
}

#[test]
fn resources_serialize_as_scim_json() {
    let created = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
    let id = Uuid::parse_str(USER).unwrap();
    let user = UserResource {
        schemas: vec![urn::USER],
        id,
        external_id: None,
        user_name: USER_NAME.to_owned(),
        name: Some(Name {
            given_name: Some("Barbara".to_owned()),
            ..Name::default()
        }),
        display_name: Some("Babs".to_owned()),
        emails: vec![Email::work(USER_NAME)],
        active: true,
        groups: vec![MemberReference {
            value: GROUP.to_owned(),
            display: Some("Admins".to_owned()),
            reference: Some(format!("https://zone.example/Groups/{GROUP}")),
        }],
        meta: Meta::user(
            created,
            created,
            format!("https://zone.example/Users/{USER}"),
        ),
    };
    assert_eq!(
        serde_json::to_value(&user).unwrap(),
        json!({
            "schemas": [urn::USER],
            "id": USER,
            "userName": USER_NAME,
            "name": { "givenName": "Barbara" },
            "displayName": "Babs",
            "emails": [{ "value": USER_NAME, "type": "work", "primary": true }],
            "active": true,
            "groups": [{
                "value": GROUP,
                "display": "Admins",
                "$ref": format!("https://zone.example/Groups/{GROUP}"),
            }],
            "meta": {
                "resourceType": "User",
                "created": "2026-01-02T03:04:05Z",
                "lastModified": "2026-01-02T03:04:05Z",
                "location": format!("https://zone.example/Users/{USER}"),
            },
        })
    );
    let group = GroupResource {
        schemas: vec![urn::GROUP],
        id: Uuid::parse_str(GROUP).unwrap(),
        external_id: Some("ext".to_owned()),
        display_name: "Admins".to_owned(),
        members: None,
        meta: Meta::group(
            created,
            created,
            format!("https://zone.example/Groups/{GROUP}"),
        ),
    };
    let listed = serde_json::to_value(ListResponse::new(vec![group], 3, 2)).unwrap();
    assert_eq!(
        listed,
        json!({
            "schemas": [urn::LIST_RESPONSE],
            "totalResults": 3,
            "startIndex": 2,
            "itemsPerPage": 1,
            "Resources": [{
                "schemas": [urn::GROUP],
                "id": GROUP,
                "externalId": "ext",
                "displayName": "Admins",
                "meta": {
                    "resourceType": "Group",
                    "created": "2026-01-02T03:04:05Z",
                    "lastModified": "2026-01-02T03:04:05Z",
                    "location": format!("https://zone.example/Groups/{GROUP}"),
                },
            }],
        })
    );
}

#[test]
fn the_service_provider_config_advertises_what_zone_supports() {
    let config =
        discovery::service_provider_config("https://zone.example/api/scim/v2/organizations/o/");
    assert_eq!(config["schemas"], json!([urn::SERVICE_PROVIDER_CONFIG]));
    assert_eq!(config["patch"]["supported"], json!(true));
    for unsupported in ["bulk", "sort", "etag", "changePassword"] {
        assert_eq!(
            config[unsupported]["supported"],
            json!(false),
            "{unsupported}"
        );
    }
    assert_eq!(
        config["filter"],
        json!({ "supported": true, "maxResults": 1000 })
    );
    assert_eq!(
        config["authenticationSchemes"][0]["type"],
        json!("oauthbearertoken")
    );
    assert_eq!(
        config["meta"]["location"],
        json!("https://zone.example/api/scim/v2/organizations/o/ServiceProviderConfig")
    );
}

#[test]
fn resource_types_list_users_and_groups() {
    let base = "https://zone.example/api/scim/v2/organizations/o";
    let listed = discovery::resource_types(base);
    assert_eq!(listed.total_results, 2);
    assert_eq!(listed.items_per_page, 2);
    let user = discovery::resource_type(base, "User").unwrap();
    assert_eq!(user["endpoint"], json!("/Users"));
    assert_eq!(user["schema"], json!(urn::USER));
    assert_eq!(
        user["meta"]["location"],
        json!(format!("{base}/ResourceTypes/User"))
    );
    assert_eq!(
        discovery::resource_type(base, "group").unwrap()["endpoint"],
        json!("/Groups")
    );
    assert_eq!(
        listed.resources,
        [user, discovery::resource_type(base, "Group").unwrap()]
    );
    assert_eq!(discovery::resource_type(base, "Device"), None);
}

#[test]
fn schemas_describe_only_supported_attributes() {
    let listed = discovery::schemas();
    assert_eq!(listed.total_results, 2);
    let user = discovery::schema(urn::USER).unwrap();
    let names: Vec<&str> = user["attributes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|attribute| attribute["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "userName",
            "name",
            "displayName",
            "emails",
            "active",
            "groups"
        ]
    );
    assert_eq!(user["attributes"][0]["required"], json!(true));
    assert_eq!(user["attributes"][0]["uniqueness"], json!("server"));
    let group = discovery::schema(urn::GROUP).unwrap();
    assert_eq!(group["id"], json!(urn::GROUP));
    assert_eq!(group["attributes"][1]["name"], json!("members"));
    assert_eq!(listed.resources, [user, group]);
    assert_eq!(discovery::schema(urn::ENTERPRISE_USER), None);
}

#[test]
fn urns_are_the_wire_contract() {
    assert_eq!(urn::USER, "urn:ietf:params:scim:schemas:core:2.0:User");
    assert_eq!(urn::GROUP, "urn:ietf:params:scim:schemas:core:2.0:Group");
    assert_eq!(
        urn::ENTERPRISE_USER,
        "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User"
    );
    assert_eq!(
        urn::LIST_RESPONSE,
        "urn:ietf:params:scim:api:messages:2.0:ListResponse"
    );
    assert_eq!(
        urn::PATCH_OP,
        "urn:ietf:params:scim:api:messages:2.0:PatchOp"
    );
    assert_eq!(urn::ERROR, "urn:ietf:params:scim:api:messages:2.0:Error");
    assert_eq!(
        urn::SERVICE_PROVIDER_CONFIG,
        "urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig"
    );
    assert_eq!(
        urn::RESOURCE_TYPE,
        "urn:ietf:params:scim:schemas:core:2.0:ResourceType"
    );
    assert_eq!(urn::SCHEMA, "urn:ietf:params:scim:schemas:core:2.0:Schema");
    assert_eq!(urn::CONTENT_TYPE, "application/scim+json");
}

#[test]
fn keys_that_differ_only_in_case_apply_in_document_order() {
    let changes = |body: &str| {
        UserChanges::from_patch(&serde_json::from_str(body).expect("a patch request")).unwrap()
    };
    assert_eq!(
        changes(
            r#"{"Operations": [{"op": "replace", "value":
                {"userName": "first@example.com", "USERNAME": "last@example.com"}}]}"#
        )
        .user_name,
        Some("last@example.com".to_owned())
    );
    assert_eq!(
        changes(
            r#"{"Operations": [{"op": "replace", "value":
                {"USERNAME": "first@example.com", "userName": "last@example.com"}}]}"#
        )
        .user_name,
        Some("last@example.com".to_owned())
    );
    assert_eq!(
        changes(
            r#"{"Operations": [{"op": "replace", "path": "name", "value":
                {"familyName": "First", "FamilyName": "Last", "givenName": "Barbara"}}]}"#
        )
        .family_name,
        some("Last")
    );
}

#[test]
fn folding_a_huge_value_object_takes_linear_time() {
    const DISTINCT: usize = 200_000;
    const REPEATED: usize = 100_000;
    let keys = (0..DISTINCT)
        .map(|index| format!(r#""k{index}":0"#))
        .chain((0..REPEATED).map(|index| format!(r#""K{index}":1"#)))
        .collect::<Vec<_>>()
        .join(",");
    let body = format!(r#"{{"Operations":[{{"op":"add","value":{{{keys}}}}}]}}"#);

    let started = std::time::Instant::now();
    let request: PatchRequest = serde_json::from_str(&body).expect("a patch request");
    let elapsed = started.elapsed();

    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "folding {} keys took {elapsed:?}",
        DISTINCT + REPEATED
    );
    let object = request.operations[0]
        .value
        .as_ref()
        .and_then(Value::as_object)
        .expect("an object value");
    assert_eq!(object.len(), DISTINCT);
    assert_eq!(object.get("K0"), Some(&json!(1)));
    assert_eq!(object.get(&format!("K{}", REPEATED - 1)), Some(&json!(1)));
    assert_eq!(object.get("k0"), None);
    assert_eq!(object.get(&format!("k{REPEATED}")), Some(&json!(0)));
}

#[test]
fn an_empty_path_is_an_invalid_path() {
    for path in ["", " "] {
        let error = user_patch(operations(
            json!([{ "op": "replace", "path": path, "value": { "displayName": "Babs" } }]),
        ))
        .unwrap_err();
        assert_eq!(error.scim_type(), Some(ScimType::InvalidPath), "{path:?}");
        let error = group_patch(operations(json!([{ "op": "remove", "path": path }]))).unwrap_err();
        assert_eq!(error.scim_type(), Some(ScimType::InvalidPath), "{path:?}");
    }
}

#[test]
fn filter_strings_read_every_json_escape() {
    assert_eq!(
        Filter::parse(r#"userName eq "a\nb\tc\rd\be\ff\/gé😀""#),
        Ok(equals(
            Attribute::UserName,
            "a\nb\tc\rd\u{8}e\u{c}f/g\u{e9}\u{1f600}"
        ))
    );
    for input in [
        r#"userName eq "a\x""#,
        r#"userName eq "a\u12""#,
        r#"userName eq "a\u12zz""#,
        r#"userName eq "a\u+123""#,
        r#"userName eq "a\ud83d""#,
        r#"userName eq "a\ud83dA""#,
        r#"userName eq "a\udc00""#,
    ] {
        assert!(!refused_filter(input).is_empty(), "{input}");
    }
}

#[test]
fn an_unquoted_null_is_refused_rather_than_compared_as_text() {
    for input in ["externalId eq null", "userName eq NULL", "members eq null"] {
        assert!(!refused_filter(input).is_empty(), "{input}");
    }
    assert_eq!(
        Filter::parse("externalId eq \"null\""),
        Ok(equals(Attribute::ExternalId, "null"))
    );
}

#[test]
fn an_email_without_a_value_is_skipped() {
    let document: UserDocument = serde_json::from_value(json!({
        "userName": "bjensen",
        "emails": [
            { "value": null, "type": "work", "primary": true },
            { "value": "home@example.com", "type": "home" }
        ]
    }))
    .unwrap();
    assert_eq!(document.primary_email(), Some("home@example.com"));
    let changes = user_patch(operations(json!([{
        "op": "replace",
        "path": "emails",
        "value": [{ "value": null, "primary": true }, { "value": "work@example.com" }]
    }])))
    .unwrap();
    assert_eq!(changes.email, Some("work@example.com".to_owned()));
}

#[test]
fn replacing_the_name_with_null_clears_it() {
    let cleared = UserChanges {
        given_name: Some(None),
        family_name: Some(None),
        ..UserChanges::default()
    };
    assert_eq!(
        user_patch(operations(
            json!([{ "op": "replace", "path": "name", "value": null }])
        )),
        Ok(cleared.clone())
    );
    assert_eq!(
        user_patch(operations(
            json!([{ "op": "replace", "value": { "name": null } }])
        )),
        Ok(cleared)
    );
}

#[test]
fn discovery_marks_group_references_read_only_and_emails_optional() {
    let user = discovery::schema(urn::USER).unwrap();
    let attribute = |name: &str| {
        user["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|attribute| attribute["name"] == name)
            .unwrap_or_else(|| panic!("no {name} attribute"))
            .clone()
    };
    assert_eq!(attribute("emails")["required"], json!(false));
    let groups = attribute("groups");
    assert_eq!(groups["mutability"], json!("readOnly"));
    for sub_attribute in groups["subAttributes"].as_array().unwrap() {
        assert_eq!(
            sub_attribute["mutability"],
            json!("readOnly"),
            "{}",
            sub_attribute["name"]
        );
    }
}
