//! The agent policy routes: who may read and change each layer, how the license gates changes,
//! how a patch is applied and validated, and what every change audits.

use std::time::{Duration, Instant};

use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use zone_enterprise::policy::{
    BODY_INVALID, CEILING_INVALID, DOMAIN_INVALID, MODEL_INVALID, Rules, SERVER_INVALID, WIDER,
};
use zone_server::db::audit::{actions, resources};
use zone_server::db::organization_members::OrgRole;
use zone_server::db::workspace_members::WorkspaceRole;

use crate::common::TestResponse;
use crate::common::policy::Fixture;
use crate::common::sso::{grace, lapsed};

const PREFIX: &str = "policy_routes";
const LICENSE_REQUIRED: &str = "license_required";
const UNVERIFIABLE: &str = "not a license key";
const WAITING: &str = "SELECT count(*) FROM pg_locks \
     WHERE NOT granted AND relation = $1::regclass \
     AND database = (SELECT oid FROM pg_database WHERE datname = current_database())";
const QUEUE_WITHIN: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy)]
enum Layer {
    Organization,
    Workspace,
}

impl Layer {
    fn table(self) -> &'static str {
        match self {
            Self::Organization => "organization_policies",
            Self::Workspace => "workspace_policies",
        }
    }

    fn freeze(self) -> &'static str {
        match self {
            Self::Organization => "LOCK TABLE organization_policies IN SHARE ROW EXCLUSIVE MODE",
            Self::Workspace => "LOCK TABLE workspace_policies IN SHARE ROW EXCLUSIVE MODE",
        }
    }

    fn resource(self) -> &'static str {
        match self {
            Self::Organization => resources::ORGANIZATION_AGENT_POLICY,
            Self::Workspace => resources::WORKSPACE_AGENT_POLICY,
        }
    }

    fn rules(self, response: &TestResponse) -> Value {
        response.assert_status(StatusCode::OK);
        let field = match self {
            Self::Organization => "policy",
            Self::Workspace => "workspace",
        };
        response.json_value()[field].clone()
    }

    async fn update(self, fixture: &Fixture, body: &Value) -> TestResponse {
        let token = &fixture.owner.token;
        match self {
            Self::Organization => fixture.update_organization(token, body).await,
            Self::Workspace => fixture.update_workspace(token, body).await,
        }
    }

    async fn read(self, fixture: &Fixture) -> Value {
        let token = &fixture.owner.token;
        let response = match self {
            Self::Organization => fixture.organization_policy(token).await,
            Self::Workspace => fixture.workspace_policy(token).await,
        };
        self.rules(&response)
    }
}

fn unrestricted() -> Value {
    serde_json::to_value(Rules::default()).unwrap()
}

fn refused(response: &TestResponse, status: StatusCode, code: &str) {
    response.assert_status(status);
    assert_eq!(
        response.json_value()["code"],
        code,
        "unexpected refusal: {}",
        response.text()
    );
}

fn uncoded(response: &TestResponse, status: StatusCode) {
    response.assert_status(status);
    assert_eq!(
        response.json_value().get("code"),
        None,
        "an access refusal carries no policy code: {}",
        response.text()
    );
}

async fn licensed() -> Fixture {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license().await;
    fixture
}

#[tokio::test]
async fn a_stranger_is_told_neither_layer_exists() {
    let fixture = licensed().await;
    let stranger = fixture.stranger().await;
    let change = json!({ "agents": [] });

    uncoded(
        &fixture.organization_policy(&stranger.token).await,
        StatusCode::NOT_FOUND,
    );
    uncoded(
        &fixture.update_organization(&stranger.token, &change).await,
        StatusCode::NOT_FOUND,
    );
    uncoded(
        &fixture.workspace_policy(&stranger.token).await,
        StatusCode::NOT_FOUND,
    );
    uncoded(
        &fixture.update_workspace(&stranger.token, &change).await,
        StatusCode::NOT_FOUND,
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_organization_member_outside_the_workspace_is_told_it_does_not_exist() {
    let fixture = licensed().await;
    let member = fixture.member(OrgRole::Member, None).await;

    fixture
        .organization_policy(&member.token)
        .await
        .assert_status(StatusCode::OK);
    uncoded(
        &fixture.workspace_policy(&member.token).await,
        StatusCode::NOT_FOUND,
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_member_reads_both_layers_and_changes_neither() {
    let fixture = licensed().await;
    let member = fixture
        .member(OrgRole::Member, Some(WorkspaceRole::Member))
        .await;
    let change = json!({ "agents": [] });

    let organization = fixture.organization_policy(&member.token).await;
    organization.assert_status(StatusCode::OK);
    assert_eq!(organization.json_value()["editable"], false);
    let workspace = fixture.workspace_policy(&member.token).await;
    workspace.assert_status(StatusCode::OK);
    assert_eq!(workspace.json_value()["editable"], false);

    uncoded(
        &fixture.update_organization(&member.token, &change).await,
        StatusCode::FORBIDDEN,
    );
    uncoded(
        &fixture.update_workspace(&member.token, &change).await,
        StatusCode::FORBIDDEN,
    );
    assert!(
        fixture
            .audits(actions::AGENT_POLICY_UPDATED)
            .await
            .is_empty()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_viewer_reads_the_workspace_policy() {
    let fixture = licensed().await;
    let viewer = fixture
        .member(OrgRole::Member, Some(WorkspaceRole::Viewer))
        .await;

    let response = fixture.workspace_policy(&viewer.token).await;
    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["editable"], false);
    uncoded(
        &fixture
            .update_workspace(&viewer.token, &json!({ "ceiling": "write" }))
            .await,
        StatusCode::FORBIDDEN,
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_organization_admin_changes_both_layers() {
    let fixture = licensed().await;
    let admin = fixture.member(OrgRole::Admin, None).await;

    let organization = fixture.organization_policy(&admin.token).await;
    organization.assert_status(StatusCode::OK);
    assert_eq!(organization.json_value()["editable"], true);
    let workspace = fixture.workspace_policy(&admin.token).await;
    workspace.assert_status(StatusCode::OK);
    assert_eq!(
        workspace.json_value()["editable"],
        true,
        "an organization admin edits workspaces they are not a member of"
    );

    let organization = fixture
        .update_organization(&admin.token, &json!({ "agents": ["claude"] }))
        .await;
    organization.assert_status(StatusCode::OK);
    assert_eq!(
        organization.json_value()["policy"]["agents"],
        json!(["claude"])
    );
    let workspace = fixture
        .update_workspace(&admin.token, &json!({ "agents": [] }))
        .await;
    workspace.assert_status(StatusCode::OK);
    assert_eq!(workspace.json_value()["workspace"]["agents"], json!([]));
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_organization_owner_changes_both_layers() {
    let fixture = licensed().await;
    let owner = &fixture.owner;

    fixture
        .update_organization(&owner.token, &json!({ "ceiling": "host" }))
        .await
        .assert_status(StatusCode::OK);
    fixture
        .update_workspace(&owner.token, &json!({ "ceiling": "write" }))
        .await
        .assert_status(StatusCode::OK);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_workspace_admin_changes_only_their_own_workspace() {
    let fixture = licensed().await;
    let admin = fixture
        .member(OrgRole::Member, Some(WorkspaceRole::Admin))
        .await;
    let change = json!({ "models": ["claude-*"] });

    let own = fixture.workspace_policy(&admin.token).await;
    own.assert_status(StatusCode::OK);
    assert_eq!(own.json_value()["editable"], true);
    let changed = fixture.update_workspace(&admin.token, &change).await;
    changed.assert_status(StatusCode::OK);
    assert_eq!(
        changed.json_value()["workspace"]["models"],
        json!(["claude-*"])
    );

    let organization = fixture.organization_policy(&admin.token).await;
    organization.assert_status(StatusCode::OK);
    assert_eq!(organization.json_value()["editable"], false);
    uncoded(
        &fixture.update_organization(&admin.token, &change).await,
        StatusCode::FORBIDDEN,
    );

    let other = fixture.another_workspace().await;
    uncoded(
        &fixture
            .update_workspace_of(fixture.organization, other, &admin.token, &change)
            .await,
        StatusCode::NOT_FOUND,
    );
    fixture.seat(other, &admin, WorkspaceRole::Member).await;
    uncoded(
        &fixture
            .update_workspace_of(fixture.organization, other, &admin.token, &change)
            .await,
        StatusCode::FORBIDDEN,
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_workspace_in_another_organization_is_not_found() {
    let fixture = licensed().await;
    let (_, foreign) = fixture.another_organization(&fixture.owner).await;
    let change = json!({ "agents": [] });

    uncoded(
        &fixture
            .workspace_policy_of(fixture.organization, foreign, &fixture.owner.token)
            .await,
        StatusCode::NOT_FOUND,
    );
    uncoded(
        &fixture
            .update_workspace_of(fixture.organization, foreign, &fixture.owner.token, &change)
            .await,
        StatusCode::NOT_FOUND,
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn community_reads_an_unenforced_policy_and_cannot_change_it() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.write_organization(json!({ "agents": [] })).await;

    let response = fixture.organization_policy(&fixture.owner.token).await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["entitled"], false);
    assert_eq!(body["enforced"], false);
    assert_eq!(body["edition"], "community");
    assert_eq!(body["status"], "community");
    assert_eq!(body["editable"], false);
    assert_eq!(body["policy"]["agents"], json!([]));

    let workspace = fixture.workspace_policy(&fixture.owner.token).await;
    workspace.assert_status(StatusCode::OK);
    assert_eq!(workspace.json_value()["enforced"], false);
    assert_eq!(workspace.json_value()["editable"], false);

    refused(
        &fixture
            .update_organization(&fixture.owner.token, &json!({ "agents": null }))
            .await,
        StatusCode::FORBIDDEN,
        LICENSE_REQUIRED,
    );
    refused(
        &fixture
            .update_workspace(&fixture.owner.token, &json!({ "agents": [] }))
            .await,
        StatusCode::FORBIDDEN,
        LICENSE_REQUIRED,
    );
    assert!(
        fixture
            .audits(actions::AGENT_POLICY_UPDATED)
            .await
            .is_empty()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_role_is_checked_before_the_license() {
    let fixture = Fixture::new(PREFIX).await;
    let member = fixture
        .member(OrgRole::Member, Some(WorkspaceRole::Member))
        .await;

    uncoded(
        &fixture
            .update_organization(&member.token, &json!({ "agents": [] }))
            .await,
        StatusCode::FORBIDDEN,
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_license_is_checked_before_the_body_is_read() {
    let fixture = Fixture::new(PREFIX).await;

    for body in [
        json!({ "unknown": true }),
        json!({ "providers": "anthropic" }),
        json!({ "models": ["a b"] }),
    ] {
        refused(
            &fixture
                .update_organization(&fixture.owner.token, &body)
                .await,
            StatusCode::FORBIDDEN,
            LICENSE_REQUIRED,
        );
        refused(
            &fixture.update_workspace(&fixture.owner.token, &body).await,
            StatusCode::FORBIDDEN,
            LICENSE_REQUIRED,
        );
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_license_in_its_grace_period_can_change_the_policy() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.install(&grace()).await;

    let response = fixture
        .update_organization(&fixture.owner.token, &json!({ "agents": ["codex"] }))
        .await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["status"], "grace");
    assert_eq!(body["edition"], "enterprise");
    assert_eq!(body["entitled"], true);
    assert_eq!(body["enforced"], true);
    assert_eq!(body["editable"], true);
    assert_eq!(body["policy"]["agents"], json!(["codex"]));
    fixture
        .update_workspace(&fixture.owner.token, &json!({ "agents": [] }))
        .await
        .assert_status(StatusCode::OK);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_keeps_enforcing_and_refuses_changes() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.store_license(&lapsed()).await;

    for response in [
        fixture.organization_policy(&fixture.owner.token).await,
        fixture.workspace_policy(&fixture.owner.token).await,
    ] {
        response.assert_status(StatusCode::OK);
        let body = response.json_value();
        assert_eq!(body["status"], "lapsed");
        assert_eq!(body["entitled"], false);
        assert_eq!(body["enforced"], true);
        assert_eq!(body["editable"], false);
    }
    refused(
        &fixture
            .update_organization(&fixture.owner.token, &json!({ "agents": [] }))
            .await,
        StatusCode::FORBIDDEN,
        LICENSE_REQUIRED,
    );
    refused(
        &fixture
            .update_workspace(&fixture.owner.token, &json!({ "agents": [] }))
            .await,
        StatusCode::FORBIDDEN,
        LICENSE_REQUIRED,
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_stored_key_that_fails_verification_shows_the_policy_enforced_and_uneditable() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.store_license(UNVERIFIABLE).await;

    for response in [
        fixture.organization_policy(&fixture.owner.token).await,
        fixture.workspace_policy(&fixture.owner.token).await,
    ] {
        response.assert_status(StatusCode::OK);
        let body = response.json_value();
        assert_eq!(body["edition"], "community");
        assert_eq!(body["entitled"], false);
        assert_eq!(body["enforced"], true);
        assert_eq!(body["editable"], false);
    }
    refused(
        &fixture
            .update_workspace(&fixture.owner.token, &json!({ "agents": [] }))
            .await,
        StatusCode::FORBIDDEN,
        LICENSE_REQUIRED,
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_unset_policy_restricts_nothing() {
    let fixture = licensed().await;

    let organization = fixture.organization_policy(&fixture.owner.token).await;
    organization.assert_status(StatusCode::OK);
    let body = organization.json_value();
    assert_eq!(body["policy"], unrestricted());
    assert_eq!(body["updated_at"], Value::Null);
    assert_eq!(body["entitled"], true);
    assert_eq!(body["enforced"], true);
    assert_eq!(body["edition"], "enterprise");
    assert_eq!(body["status"], "active");

    let workspace = fixture.workspace_policy(&fixture.owner.token).await;
    workspace.assert_status(StatusCode::OK);
    let body = workspace.json_value();
    assert_eq!(body["organization"], unrestricted());
    assert_eq!(body["workspace"], unrestricted());
    assert_eq!(body["effective"], unrestricted());
    assert_eq!(body["updated_at"], Value::Null);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_sparse_update_keeps_what_it_leaves_out_and_null_clears() {
    let fixture = licensed().await;
    let token = &fixture.owner.token;

    let first = fixture
        .update_organization(
            token,
            &json!({
                "providers": ["bedrock", "anthropic", "bedrock"],
                "models": [" sonnet ", "claude-*"],
                "domains": ["*.Example.com."],
                "ceiling": "host",
            }),
        )
        .await;
    first.assert_status(StatusCode::OK);
    assert_eq!(
        first.json_value()["policy"],
        json!({
            "providers": ["anthropic", "bedrock"],
            "agents": null,
            "models": ["claude-*", "sonnet"],
            "ceiling": "host",
            "mcp_servers": null,
            "domains": ["example.com"],
        })
    );
    assert!(first.json_value()["updated_at"].is_string());

    let second = fixture
        .update_organization(token, &json!({ "agents": [], "ceiling": null }))
        .await;
    second.assert_status(StatusCode::OK);
    let expected = json!({
        "providers": ["anthropic", "bedrock"],
        "agents": [],
        "models": ["claude-*", "sonnet"],
        "ceiling": null,
        "mcp_servers": null,
        "domains": ["example.com"],
    });
    assert_eq!(second.json_value()["policy"], expected);
    assert_eq!(
        fixture.organization_policy(token).await.json_value()["policy"],
        expected
    );

    fixture
        .update_workspace(token, &json!({ "mcp_servers": ["github"] }))
        .await
        .assert_status(StatusCode::OK);
    let workspace = fixture
        .update_workspace(token, &json!({ "domains": ["api.example.com"] }))
        .await;
    workspace.assert_status(StatusCode::OK);
    assert_eq!(
        workspace.json_value()["workspace"],
        json!({
            "providers": null,
            "agents": null,
            "models": null,
            "ceiling": null,
            "mcp_servers": ["github"],
            "domains": ["api.example.com"],
        })
    );
    let cleared = fixture
        .update_workspace(token, &json!({ "mcp_servers": null }))
        .await;
    cleared.assert_status(StatusCode::OK);
    assert_eq!(
        cleared.json_value()["workspace"]["mcp_servers"],
        Value::Null
    );
    assert_eq!(
        cleared.json_value()["workspace"]["domains"],
        json!(["api.example.com"])
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_body_that_cannot_be_read_is_refused() {
    let fixture = licensed().await;
    let token = &fixture.owner.token;

    for body in [
        json!({ "unknown": true }),
        json!({ "providers": ["gemini"] }),
        json!({ "agents": ["claude_code"] }),
        json!({ "ceiling": "none" }),
        json!({ "models": "sonnet" }),
        json!("agents"),
    ] {
        refused(
            &fixture.update_organization(token, &body).await,
            StatusCode::BAD_REQUEST,
            BODY_INVALID,
        );
        refused(
            &fixture.update_workspace(token, &body).await,
            StatusCode::BAD_REQUEST,
            BODY_INVALID,
        );
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn every_invalid_rule_is_refused_with_its_code() {
    let fixture = licensed().await;
    let token = &fixture.owner.token;

    for (body, code) in [
        (json!({ "models": ["claude opus"] }), MODEL_INVALID),
        (json!({ "models": ["*-opus"] }), MODEL_INVALID),
        (
            json!({ "domains": ["https://example.com"] }),
            DOMAIN_INVALID,
        ),
        (json!({ "domains": ["example.com:8080"] }), DOMAIN_INVALID),
        (json!({ "mcp_servers": ["git hub"] }), SERVER_INVALID),
        (json!({ "ceiling": "read" }), CEILING_INVALID),
        (json!({ "ceiling": "outward" }), CEILING_INVALID),
    ] {
        refused(
            &fixture.update_organization(token, &body).await,
            StatusCode::BAD_REQUEST,
            code,
        );
        refused(
            &fixture.update_workspace(token, &body).await,
            StatusCode::BAD_REQUEST,
            code,
        );
    }
    assert_eq!(
        fixture.organization_policy(token).await.json_value()["policy"],
        unrestricted(),
        "a refused change stores nothing"
    );
    assert!(
        fixture
            .audits(actions::AGENT_POLICY_UPDATED)
            .await
            .is_empty()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_workspace_cannot_widen_a_patched_field() {
    let fixture = licensed().await;
    let token = &fixture.owner.token;
    fixture
        .update_organization(
            token,
            &json!({
                "providers": ["anthropic", "bedrock"],
                "agents": ["claude"],
                "models": ["claude-*"],
                "ceiling": "write",
                "mcp_servers": ["github"],
                "domains": ["example.com"],
            }),
        )
        .await
        .assert_status(StatusCode::OK);

    for body in [
        json!({ "providers": ["openai"] }),
        json!({ "agents": ["codex"] }),
        json!({ "models": ["gpt-*"] }),
        json!({ "ceiling": "host" }),
        json!({ "mcp_servers": ["linear"] }),
        json!({ "domains": ["example.org"] }),
    ] {
        refused(
            &fixture.update_workspace(token, &body).await,
            StatusCode::BAD_REQUEST,
            WIDER,
        );
    }
    fixture
        .update_workspace(
            token,
            &json!({
                "providers": ["bedrock"],
                "agents": [],
                "models": ["claude-opus-*"],
                "ceiling": "write",
                "mcp_servers": [],
                "domains": ["api.example.com"],
            }),
        )
        .await
        .assert_status(StatusCode::OK);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_wider_field_left_out_of_the_patch_is_not_checked() {
    let fixture = licensed().await;
    let token = &fixture.owner.token;
    fixture
        .write_organization(json!({ "providers": ["anthropic"], "agents": ["claude"] }))
        .await;
    fixture
        .write_workspace(json!({ "providers": ["openai"] }))
        .await;

    let response = fixture
        .update_workspace(token, &json!({ "agents": [] }))
        .await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["workspace"]["providers"], json!(["openai"]));
    assert_eq!(body["workspace"]["agents"], json!([]));
    assert_eq!(
        body["effective"]["providers"],
        json!([]),
        "enforcement still intersects both layers"
    );

    refused(
        &fixture
            .update_workspace(token, &json!({ "providers": ["openai"] }))
            .await,
        StatusCode::BAD_REQUEST,
        WIDER,
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_workspace_view_shows_both_layers_and_their_effect() {
    let fixture = licensed().await;
    let token = &fixture.owner.token;
    let organization = json!({
        "providers": ["anthropic", "bedrock"],
        "agents": ["claude", "codex"],
        "models": ["claude-*", "sonnet"],
        "ceiling": "host",
        "mcp_servers": ["github", "linear"],
        "domains": ["example.com"],
    });
    fixture
        .update_organization(token, &organization)
        .await
        .assert_status(StatusCode::OK);
    let workspace = json!({
        "providers": ["bedrock"],
        "agents": null,
        "models": ["claude-opus-*"],
        "ceiling": "write",
        "mcp_servers": ["github"],
        "domains": null,
    });
    fixture
        .update_workspace(token, &workspace)
        .await
        .assert_status(StatusCode::OK);

    let response = fixture.workspace_policy(token).await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["organization"], organization);
    assert_eq!(body["workspace"], workspace);
    assert_eq!(
        body["effective"],
        json!({
            "providers": ["bedrock"],
            "agents": ["claude", "codex"],
            "models": ["claude-opus-*"],
            "ceiling": "write",
            "mcp_servers": ["github"],
            "domains": ["example.com"],
        })
    );
    assert!(body["updated_at"].is_string());
    assert_eq!(body["editable"], true);
    fixture.cleanup().await;
}

#[tokio::test]
async fn every_change_is_audited_with_the_old_and_new_rules() {
    let fixture = licensed().await;
    let token = &fixture.owner.token;

    let changed = fixture
        .update_organization(token, &json!({ "agents": ["claude"] }))
        .await;
    changed.assert_status(StatusCode::OK);
    let organization_rules = changed.json_value()["policy"].clone();
    let workspace = fixture
        .update_workspace(token, &json!({ "agents": [] }))
        .await;
    workspace.assert_status(StatusCode::OK);
    let workspace_rules = workspace.json_value()["workspace"].clone();

    let audits = fixture.audits(actions::AGENT_POLICY_UPDATED).await;
    assert_eq!(audits.len(), 2);
    let organization = &audits[0];
    assert_eq!(
        organization.resource_type,
        resources::ORGANIZATION_AGENT_POLICY
    );
    assert_eq!(organization.resource_id, Some(fixture.organization));
    assert_eq!(organization.organization_id, Some(fixture.organization));
    assert_eq!(organization.actor_id, Some(fixture.owner.id));
    assert_eq!(organization.old_values, Some(unrestricted()));
    assert_eq!(organization.new_values, Some(organization_rules.clone()));
    let workspace = &audits[1];
    assert_eq!(workspace.resource_type, resources::WORKSPACE_AGENT_POLICY);
    assert_eq!(workspace.resource_id, Some(fixture.workspace));
    assert_eq!(workspace.organization_id, Some(fixture.organization));
    assert_eq!(workspace.old_values, Some(unrestricted()));
    assert_eq!(workspace.new_values, Some(workspace_rules));

    let second = fixture
        .update_organization(token, &json!({ "agents": null }))
        .await;
    second.assert_status(StatusCode::OK);
    let audits = fixture.audits(actions::AGENT_POLICY_UPDATED).await;
    assert_eq!(audits.len(), 3);
    assert_eq!(audits[2].old_values, Some(organization_rules));
    assert_eq!(audits[2].new_values, Some(unrestricted()));
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_unchanged_policy_is_not_audited() {
    let fixture = licensed().await;
    let token = &fixture.owner.token;

    for body in [json!({}), json!({ "agents": null })] {
        let response = fixture.update_organization(token, &body).await;
        response.assert_status(StatusCode::OK);
        assert_eq!(response.json_value()["updated_at"], Value::Null);
        fixture
            .update_workspace(token, &body)
            .await
            .assert_status(StatusCode::OK);
    }
    let first = fixture
        .update_organization(token, &json!({ "models": ["sonnet", "claude-*"] }))
        .await;
    first.assert_status(StatusCode::OK);
    let updated_at = first.json_value()["updated_at"].clone();
    let repeated = fixture
        .update_organization(token, &json!({ "models": ["claude-*", " sonnet"] }))
        .await;
    repeated.assert_status(StatusCode::OK);
    assert_eq!(repeated.json_value()["updated_at"], updated_at);
    fixture
        .update_workspace(token, &json!({ "ceiling": "write" }))
        .await
        .assert_status(StatusCode::OK);
    fixture
        .update_workspace(token, &json!({ "ceiling": "write" }))
        .await
        .assert_status(StatusCode::OK);

    let audits = fixture.audits(actions::AGENT_POLICY_UPDATED).await;
    assert_eq!(audits.len(), 2, "only the two real changes are audited");
    fixture.cleanup().await;
}

/// Commits `freeze` once both writers queue on `layer`'s table, so neither has written the layer
/// before the other starts.
async fn release(fixture: &Fixture, layer: Layer, freeze: Transaction<'_, Postgres>) {
    let deadline = Instant::now() + QUEUE_WITHIN;
    loop {
        let waiting: i64 = sqlx::query_scalar(WAITING)
            .bind(layer.table())
            .fetch_one(fixture.pool())
            .await
            .unwrap();
        if waiting == 2 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "both {layer:?} changes wait on the frozen table, {waiting} do"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    freeze.commit().await.unwrap();
}

#[tokio::test]
async fn concurrent_first_changes_to_a_layer_both_land_and_audit_what_they_replaced() {
    let fixture = licensed().await;

    for (layer, first, second) in [
        (
            Layer::Organization,
            json!({ "agents": ["claude"] }),
            json!({ "models": ["sonnet"] }),
        ),
        (
            Layer::Workspace,
            json!({ "ceiling": "write" }),
            json!({ "agents": [] }),
        ),
    ] {
        let mut freeze = fixture.pool().begin().await.unwrap();
        sqlx::query(layer.freeze())
            .execute(&mut *freeze)
            .await
            .unwrap();
        let (one, two, ()) = tokio::join!(
            layer.update(&fixture, &first),
            layer.update(&fixture, &second),
            release(&fixture, layer, freeze),
        );
        one.assert_status(StatusCode::OK);
        two.assert_status(StatusCode::OK);

        let stored = layer.read(&fixture).await;
        for patch in [&first, &second] {
            for (field, value) in patch.as_object().unwrap() {
                assert_eq!(&stored[field], value, "{layer:?} kept {field}: {stored}");
            }
        }
        let audits: Vec<_> = fixture
            .audits(actions::AGENT_POLICY_UPDATED)
            .await
            .into_iter()
            .filter(|audit| audit.resource_type == layer.resource())
            .collect();
        assert_eq!(audits.len(), 2, "{layer:?}: {audits:?}");
        let earlier = audits
            .iter()
            .find(|audit| audit.old_values == Some(unrestricted()))
            .unwrap_or_else(|| panic!("one {layer:?} change replaced nothing: {audits:?}"));
        let later = audits
            .iter()
            .find(|audit| audit.old_values == earlier.new_values)
            .unwrap_or_else(|| panic!("the other {layer:?} change replaced it: {audits:?}"));
        assert_eq!(later.new_values, Some(stored));
    }
    fixture.cleanup().await;
}
