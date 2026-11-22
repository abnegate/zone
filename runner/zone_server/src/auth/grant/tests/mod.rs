use std::collections::BTreeSet;

use axum::http::StatusCode;

use super::*;
use crate::auth::jwt::Claims;
use crate::auth::middleware::require_permission;
use crate::auth::permission::holds;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Threshold {
    Server,
    Organization(OrgRole),
    Workspace(WorkspaceRole),
}

/// The "built-in" column of the catalogue, written out by hand so the grants
/// are checked against the specification rather than against themselves.
fn threshold(permission: Permission) -> Threshold {
    use Threshold::{Organization, Server, Workspace};
    match permission {
        Permission::ModelsCreate
        | Permission::ModelsDelete
        | Permission::PullRequestsUpdate
        | Permission::ReleasesCreate => Server,
        Permission::OrganizationRead => Organization(OrgRole::Member),
        Permission::OrganizationUpdate => Organization(OrgRole::Admin),
        Permission::OrganizationDelete => Organization(OrgRole::Owner),
        Permission::MembersCreate | Permission::MembersUpdate | Permission::MembersDelete => {
            Organization(OrgRole::Admin)
        }
        Permission::AdministratorsUpdate => Organization(OrgRole::Owner),
        Permission::InvitationsCreate
        | Permission::InvitationsRead
        | Permission::InvitationsDelete => Organization(OrgRole::Admin),
        Permission::WorkspacesCreate => Organization(OrgRole::Admin),
        Permission::DevicesRead | Permission::DevicesUpdate => Organization(OrgRole::Admin),
        Permission::GitHubUpdate => Organization(OrgRole::Admin),
        Permission::McpServersUpdate => Organization(OrgRole::Admin),
        Permission::AiSettingsRead | Permission::AiSettingsUpdate => Organization(OrgRole::Admin),
        Permission::AgentLoginsUpdate => Organization(OrgRole::Admin),
        Permission::PoliciesUpdate => Organization(OrgRole::Admin),
        Permission::AuditLogsRead => Organization(OrgRole::Admin),
        Permission::AuditUpdate => Organization(OrgRole::Admin),
        Permission::SpendRead | Permission::SpendUpdate => Organization(OrgRole::Admin),
        Permission::RolesUpdate => Organization(OrgRole::Admin),
        Permission::WorkspaceRead => Workspace(WorkspaceRole::Viewer),
        Permission::WorkspaceWrite => Workspace(WorkspaceRole::Member),
        Permission::WorkspaceUpdate => Workspace(WorkspaceRole::Admin),
        Permission::WorkspaceDelete => Workspace(WorkspaceRole::Owner),
        Permission::WorkspaceMembersCreate
        | Permission::WorkspaceMembersUpdate
        | Permission::WorkspaceMembersDelete => Workspace(WorkspaceRole::Admin),
        Permission::WorkspaceAdministratorsUpdate => Workspace(WorkspaceRole::Owner),
        Permission::WorkspaceAiSettingsUpdate => Workspace(WorkspaceRole::Admin),
        Permission::WorkspaceMcpServersUpdate => Workspace(WorkspaceRole::Admin),
        Permission::WorkspacePoliciesUpdate => Workspace(WorkspaceRole::Admin),
        Permission::ProjectSyncsUpdate => Workspace(WorkspaceRole::Admin),
        Permission::PullRequestsOverride => Workspace(WorkspaceRole::Admin),
    }
}

fn composable_by_specification(permission: Permission) -> bool {
    !matches!(
        permission,
        Permission::ModelsCreate
            | Permission::ModelsDelete
            | Permission::PullRequestsUpdate
            | Permission::ReleasesCreate
            | Permission::OrganizationRead
            | Permission::OrganizationDelete
            | Permission::AdministratorsUpdate
            | Permission::RolesUpdate
            | Permission::WorkspaceRead
            | Permission::WorkspaceDelete
            | Permission::WorkspaceAdministratorsUpdate
    )
}

const ORGANIZATION_ROLES: [OrgRole; 3] = [OrgRole::Member, OrgRole::Admin, OrgRole::Owner];
const WORKSPACE_ROLES: [WorkspaceRole; 4] = [
    WorkspaceRole::Viewer,
    WorkspaceRole::Member,
    WorkspaceRole::Admin,
    WorkspaceRole::Owner,
];

#[test]
fn the_threshold_table_agrees_with_the_catalogue_on_every_permission() {
    for permission in Permission::ALL {
        let expected = match threshold(permission) {
            Threshold::Server => Scope::Server,
            Threshold::Organization(_) => Scope::Organization,
            Threshold::Workspace(_) => Scope::Workspace,
        };
        assert_eq!(permission.scope(), expected, "{}", permission.name());
        assert_eq!(
            permission.composable(),
            composable_by_specification(permission),
            "{}",
            permission.name()
        );
    }
}

#[test]
fn the_catalogue_holds_four_server_twenty_four_organization_and_thirteen_workspace_permissions() {
    let count = |scope| {
        Permission::ALL
            .into_iter()
            .filter(|permission| permission.scope() == scope)
            .count()
    };
    assert_eq!(count(Scope::Server), 4);
    assert_eq!(count(Scope::Organization), 24);
    assert_eq!(count(Scope::Workspace), 13);
}

#[test]
fn organization_grants_reproduce_the_built_in_thresholds() {
    for role in ORGANIZATION_ROLES {
        let grant = Grant::organization(role);
        for permission in Permission::ALL {
            let expected = matches!(threshold(permission), Threshold::Organization(minimum) if role >= minimum);
            assert_eq!(
                grant.allows(permission),
                expected,
                "organization {role:?} and {}",
                permission.name()
            );
        }
    }
}

#[test]
fn workspace_grants_reproduce_the_built_in_thresholds() {
    for role in WORKSPACE_ROLES {
        let grant = Grant::workspace(role);
        for permission in Permission::ALL {
            let expected =
                matches!(threshold(permission), Threshold::Workspace(minimum) if role >= minimum);
            assert_eq!(
                grant.allows(permission),
                expected,
                "workspace {role:?} and {}",
                permission.name()
            );
        }
    }
}

#[test]
fn built_in_sets_nest_and_rank_back_to_their_role() {
    for pair in ORGANIZATION_ROLES.windows(2) {
        let (lower, higher) = (Grant::organization(pair[0]), Grant::organization(pair[1]));
        assert!(higher.covers(&lower), "{:?} covers {:?}", pair[1], pair[0]);
        assert!(
            !lower.covers(&higher),
            "{:?} exceeds {:?}",
            pair[1],
            pair[0]
        );
    }
    for pair in WORKSPACE_ROLES.windows(2) {
        let (lower, higher) = (Grant::workspace(pair[0]), Grant::workspace(pair[1]));
        assert!(higher.covers(&lower), "{:?} covers {:?}", pair[1], pair[0]);
        assert!(
            !lower.covers(&higher),
            "{:?} exceeds {:?}",
            pair[1],
            pair[0]
        );
    }
    for role in ORGANIZATION_ROLES {
        assert_eq!(Grant::organization(role).organization_rank(), role);
    }
    for role in WORKSPACE_ROLES {
        assert_eq!(Grant::workspace(role).workspace_rank(), role);
    }
}

#[test]
fn a_partial_grant_ranks_as_the_highest_role_it_fully_covers() {
    let names = vec![Permission::WorkspaceWrite.name().to_string()];
    let writer = Grant::compose(Scope::Workspace, &names).expect("a writer composes");
    assert_eq!(writer.workspace_rank(), WorkspaceRole::Member);

    let mut almost: Vec<String> = Grant::workspace(WorkspaceRole::Admin)
        .names()
        .into_iter()
        .map(str::to_string)
        .collect();
    almost.retain(|name| name != Permission::PullRequestsOverride.name());
    let grant = Grant::compose(Scope::Workspace, &almost).expect("most of admin composes");
    assert_eq!(grant.workspace_rank(), WorkspaceRole::Member);

    let organization =
        Grant::compose(Scope::Organization, &[]).expect("an empty organization role composes");
    assert_eq!(organization.organization_rank(), OrgRole::Member);
    assert_eq!(Grant::default().organization_rank(), OrgRole::Member);
    assert_eq!(Grant::default().workspace_rank(), WorkspaceRole::Viewer);
}

#[test]
fn index_and_at_round_trip_and_names_are_unique_and_parse_back() {
    let mut seen = BTreeSet::new();
    for (position, permission) in Permission::ALL.into_iter().enumerate() {
        assert_eq!(usize::from(permission.index()), position);
        assert_eq!(Permission::at(permission.index()), permission);
        assert!(
            seen.insert(permission.name()),
            "{} repeats",
            permission.name()
        );
        assert_eq!(Permission::parse(permission.name()), Some(permission));
        let (resource, action) = permission
            .name()
            .split_once(':')
            .expect("a name is resource:action");
        assert!(!resource.is_empty() && !action.is_empty());
    }
    assert_eq!(Permission::parse("workspace:everything"), None);
}

#[test]
fn scopes_name_parse_and_carry_their_base() {
    for scope in Scope::ALL {
        assert_eq!(Scope::parse(scope.name()), Some(scope));
        if let Some(base) = scope.base() {
            assert_eq!(base.scope(), scope);
            assert!(!base.composable());
        }
    }
    assert_eq!(Scope::Server.base(), None);
    assert_eq!(Scope::parse("galaxy"), None);
    assert_eq!(
        serde_json::to_string(&Scope::Organization).expect("a scope serializes"),
        "\"organization\""
    );
}

#[test]
fn compose_adds_the_base_and_accepts_it_named() {
    let grant = Grant::compose(Scope::Workspace, &[]).expect("an empty role composes");
    assert_eq!(grant.names(), vec![Permission::WorkspaceRead.name()]);

    let named = vec![
        Permission::WorkspaceRead.name().to_string(),
        Permission::WorkspaceWrite.name().to_string(),
    ];
    let grant = Grant::compose(Scope::Workspace, &named).expect("the base may be named");
    assert!(grant.allows(Permission::WorkspaceRead));
    assert!(grant.allows(Permission::WorkspaceWrite));

    let grant = Grant::compose(
        Scope::Organization,
        &[Permission::DevicesRead.name().to_string()],
    )
    .expect("an organization role composes");
    assert!(grant.allows(Permission::OrganizationRead));
    assert!(grant.allows(Permission::DevicesRead));
}

#[test]
fn compose_refuses_unknown_names() {
    let error = Grant::compose(Scope::Workspace, &["workspace:fly".to_string()])
        .expect_err("an unknown name is refused");
    assert_eq!(error, Composition::Unknown("workspace:fly".to_string()));
    assert_eq!(error.code(), "permission_unknown");
    assert!(
        error.message().contains("workspace:fly"),
        "{}",
        error.message()
    );
}

#[test]
fn compose_refuses_every_permission_that_is_not_composable() {
    for scope in [Scope::Organization, Scope::Workspace] {
        for permission in Permission::ALL {
            if permission.scope() != scope
                || permission.composable()
                || Some(permission) == scope.base()
            {
                continue;
            }
            let error = Grant::compose(scope, &[permission.name().to_string()])
                .expect_err("a non-composable permission is refused");
            assert_eq!(error, Composition::NotComposable(permission));
            assert_eq!(error.code(), "permission_not_composable");
            assert!(error.message().contains(permission.name()));
        }
    }
}

#[test]
fn compose_refuses_every_permission_of_another_scope() {
    for scope in [Scope::Organization, Scope::Workspace] {
        for permission in Permission::ALL {
            if permission.scope() == scope {
                continue;
            }
            let error = Grant::compose(scope, &[permission.name().to_string()])
                .expect_err("an out-of-scope permission is refused");
            assert_eq!(error, Composition::OutOfScope(permission));
            assert_eq!(error.code(), "permission_out_of_scope");
            assert!(error.message().contains(permission.name()));
        }
    }
}

#[test]
fn compose_accepts_every_composable_permission_of_its_scope() {
    for scope in [Scope::Organization, Scope::Workspace] {
        let composable: Vec<String> = Permission::ALL
            .into_iter()
            .filter(|permission| permission.scope() == scope && permission.composable())
            .map(|permission| permission.name().to_string())
            .collect();
        let grant = Grant::compose(scope, &composable).expect("every composable name composes");
        assert_eq!(grant.names().len(), composable.len() + 1);
    }
}

#[test]
fn stored_keeps_only_known_composable_names_of_its_scope_and_adds_the_base() {
    let stored = Grant::stored(
        Scope::Workspace,
        [
            "workspace:write",
            "workspace:delete",
            "workspace_administrators:update",
            "mcp_servers:update",
            "models:create",
            "workspace:fly",
        ],
    );
    assert_eq!(stored.names(), vec!["workspace:read", "workspace:write"]);

    let stored = Grant::stored(
        Scope::Organization,
        vec![
            "devices:read".to_string(),
            "organization:delete".to_string(),
            "administrators:update".to_string(),
            "roles:update".to_string(),
            "workspace:write".to_string(),
        ],
    );
    assert_eq!(stored.names(), vec!["devices:read", "organization:read"]);

    assert!(Grant::stored(Scope::Server, ["models:create"]).is_empty());
}

#[test]
fn stored_never_grants_more_than_compose_would() {
    for scope in [Scope::Organization, Scope::Workspace] {
        let every: Vec<&str> = Permission::ALL.iter().map(|p| p.name()).collect();
        let stored = Grant::stored(scope, &every);
        let composable: Vec<String> = stored.names().into_iter().map(str::to_string).collect();
        assert_eq!(
            Grant::compose(scope, &composable).expect("what is stored composes"),
            stored
        );
    }
}

#[test]
fn custom_grants_never_reach_owner_only_permissions() {
    for scope in [Scope::Organization, Scope::Workspace] {
        let every: Vec<&str> = Permission::ALL.iter().map(|p| p.name()).collect();
        let stored = Grant::stored(scope, every);
        for permission in [
            Permission::OrganizationDelete,
            Permission::AdministratorsUpdate,
            Permission::RolesUpdate,
            Permission::WorkspaceDelete,
            Permission::WorkspaceAdministratorsUpdate,
        ] {
            assert!(!stored.allows(permission), "{}", permission.name());
        }
    }
}

#[test]
fn names_and_serialization_are_sorted_names() {
    let grant = Grant::workspace(WorkspaceRole::Member);
    assert_eq!(grant.names(), vec!["workspace:read", "workspace:write"]);
    assert_eq!(
        serde_json::to_value(&grant).expect("a grant serializes"),
        serde_json::json!(["workspace:read", "workspace:write"])
    );
    let names = Grant::organization(OrgRole::Owner).names();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
}

#[test]
fn covers_is_a_subset_test() {
    let admin = Grant::workspace(WorkspaceRole::Admin);
    assert!(admin.covers(&Grant::default()));
    assert!(admin.covers(&admin));
    assert!(!Grant::default().covers(&admin));
    assert!(!admin.covers(&Grant::organization(OrgRole::Member)));
}

#[test]
fn refusals_keep_the_legacy_guard_texts() {
    let expected = |permission: Permission| -> String {
        match permission {
            Permission::OrganizationRead => "Not a member of this organization".to_string(),
            Permission::OrganizationUpdate
            | Permission::MembersCreate
            | Permission::MembersUpdate
            | Permission::MembersDelete
            | Permission::InvitationsCreate
            | Permission::InvitationsRead
            | Permission::InvitationsDelete
            | Permission::WorkspacesCreate
            | Permission::DevicesRead
            | Permission::DevicesUpdate
            | Permission::GitHubUpdate => "Organization admin access required".to_string(),
            Permission::OrganizationDelete => "Organization owner access required".to_string(),
            Permission::SpendRead | Permission::SpendUpdate => {
                "Only organization owners and admins can see spend or change budgets".to_string()
            }
            Permission::WorkspaceRead => "Not a member of this workspace".to_string(),
            Permission::WorkspaceWrite => "Workspace write access required".to_string(),
            Permission::WorkspaceUpdate
            | Permission::WorkspaceMembersCreate
            | Permission::WorkspaceMembersUpdate
            | Permission::WorkspaceMembersDelete => "Workspace admin access required".to_string(),
            Permission::WorkspaceDelete => "Workspace owner access required".to_string(),
            Permission::ModelsCreate => "Installing or training models on this server requires the models:create permission. Ask a server admin for access.".to_string(),
            Permission::ModelsDelete => "Removing models from this server requires the models:delete permission. Ask a server admin for access.".to_string(),
            Permission::PullRequestsUpdate => "Merging pull requests requires the pull_requests:update permission. Ask a server admin for access.".to_string(),
            Permission::ReleasesCreate => "Creating releases requires the releases:create permission. Ask a server admin for access.".to_string(),
            other if other.scope() == Scope::Organization => format!(
                "This needs the {} permission in this organization.",
                other.name()
            ),
            other => format!(
                "This needs the {} permission in this workspace.",
                other.name()
            ),
        }
    };
    for permission in Permission::ALL {
        assert_eq!(
            permission.refusal(),
            expected(permission),
            "{}",
            permission.name()
        );
    }
}

fn claims(is_admin: bool, permissions: Vec<String>) -> Claims {
    Claims {
        sub: uuid::Uuid::new_v4().to_string(),
        email: "someone@example.test".to_string(),
        roles: Vec::new(),
        permissions,
        is_admin,
        exp: 0,
        iat: 0,
        jti: uuid::Uuid::new_v4().to_string(),
    }
}

#[test]
fn require_permission_refuses_membership_permissions_even_to_a_server_admin() {
    let every: Vec<String> = Permission::ALL
        .iter()
        .map(|permission| permission.name().to_string())
        .collect();
    for is_admin in [false, true] {
        let claims = claims(is_admin, every.clone());
        for permission in Permission::ALL {
            let outcome = require_permission(&claims, permission);
            if permission.scope() == Scope::Server {
                assert!(outcome.is_ok(), "{}", permission.name());
            } else {
                let error = outcome.expect_err("a membership permission is refused");
                assert_eq!(error.status, StatusCode::FORBIDDEN);
                assert_eq!(error.message, permission.refusal());
            }
        }
    }
}

#[tokio::test]
async fn holds_refuses_membership_permissions_without_reading_the_database() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://nobody@127.0.0.1:1/none")
        .expect("a lazy pool never connects");
    for permission in Permission::ALL {
        if permission.scope() == Scope::Server {
            continue;
        }
        assert!(
            !holds(&pool, uuid::Uuid::new_v4(), permission)
                .await
                .expect("a membership permission is refused before any query"),
            "{} must not be granted through global roles",
            permission.name()
        );
    }
}
