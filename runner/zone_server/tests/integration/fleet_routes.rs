//! The runner fleet routes: who reaches the server-wide fleet and the organization and workspace
//! runner labels, how the license and the lockout guard gate changes, what each refusal answers,
//! what every change audits, and that label responses never describe a runner. The fleet is
//! server-wide, so every fixture has a database of its own.

use std::collections::BTreeSet;
use std::fmt::Display;

use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_enterprise::LICENSE_REQUIRED;
use zone_enterprise::fleet::{
    BODY_INVALID, LABEL_INVALID, Labels, NO_RUNNER_LEFT, RUNNER_CORDONED, RUNNER_ONLINE,
    RUNNER_OUTDATED, RUNNER_REVOKED, STATE_INVALID, VERSION_INVALID, Version,
};
use zone_server::auth::{Grant, Permission, Scope};
use zone_server::db::audit::{actions, resources};
use zone_server::db::custom_roles::{self, Assignment, Creation, Definition};
use zone_server::db::fleet::runners;
use zone_server::db::fleet::state::State;
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::db::workspace_members::{self, WorkspaceRole};
use zone_server::services::fleet::Identity;
use zone_server::services::fleet::registration;

use crate::common::TestResponse;
use crate::common::policy::Fixture;
use crate::common::sso::{self, Account, lapsed, team};

const PREFIX: &str = "fleet_routes";
const POLICY: &str = "/api/fleet-policy";
const RUNNERS: &str = "/api/runners";
const LABELS: &str = "runner-labels";
const VERSION: &str = "1.4.0";
const CARRIED: &str = "gpu";
const OFFLINE: &str = "UPDATE fleet_runners SET last_seen_at = now() - interval '5 minutes' \
     WHERE id = $1";

const POLICY_FIELDS: [&str; 7] = [
    "entitled",
    "enforced",
    "edition",
    "status",
    "editable",
    "minimum_version",
    "updated_at",
];
const RUNNERS_FIELDS: [&str; 4] = ["editable", "enforced", "current", "runners"];
const RUNNER_FIELDS: [&str; 14] = [
    "id",
    "name",
    "version",
    "labels",
    "state",
    "connected",
    "outdated",
    "serving",
    "code",
    "current",
    "runs",
    "started_at",
    "last_seen_at",
    "state_changed_at",
];
const ORGANIZATION_FIELDS: [&str; 9] = [
    "entitled",
    "enforced",
    "edition",
    "status",
    "editable",
    "labels",
    "available",
    "matching",
    "updated_at",
];
const WORKSPACE_FIELDS: [&str; 11] = [
    "entitled",
    "enforced",
    "edition",
    "status",
    "editable",
    "organization",
    "workspace",
    "effective",
    "available",
    "matching",
    "updated_at",
];

#[derive(Debug, Clone, Copy)]
enum License {
    Community,
    Team,
    Enterprise,
    Lapsed,
}

#[derive(Debug, sqlx::FromRow)]
struct Entry {
    actor_id: Option<Uuid>,
    organization_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    resource_type: String,
    resource_id: Option<Uuid>,
    old_values: Option<Value>,
    new_values: Option<Value>,
}

/// A runner seeded beside this server's own, as another server process would register.
struct Seed {
    name: String,
    version: &'static str,
    labels: &'static str,
    state: State,
    connected: bool,
}

impl Seed {
    fn new(version: &'static str, labels: &'static str, state: State) -> Self {
        Self {
            name: format!("seeded-{}", Uuid::new_v4().simple()),
            version,
            labels,
            state,
            connected: true,
        }
    }

    fn active() -> Self {
        Self::new(VERSION, CARRIED, State::Active)
    }

    fn offline(mut self) -> Self {
        self.connected = false;
        self
    }
}

/// A server registered as its only runner, `1.4.0` carrying `gpu`, with the server admin owning
/// one organization and one workspace.
struct Fleet {
    fixture: Fixture,
    runner: Uuid,
}

impl Fleet {
    async fn new(license: License) -> Self {
        let server = sso::Fixture::new(PREFIX).await;
        server.state().set_identity(Identity {
            name: format!("runner-{}", Uuid::new_v4().simple()),
            version: Version::parse(VERSION).unwrap(),
            labels: Labels::parse_list(CARRIED).unwrap(),
        });
        let fixture = Fixture::on(server).await;
        match license {
            License::Community => {}
            License::Team => fixture.install(&team()).await,
            License::Enterprise => fixture.license().await,
            License::Lapsed => fixture.store_license(&lapsed()).await,
        }
        let runner = registration::register(fixture.state())
            .await
            .expect("this server registers as a runner");
        Self { fixture, runner }
    }

    async fn licensed() -> Self {
        Self::new(License::Enterprise).await
    }

    fn admin(&self) -> &str {
        &self.fixture.owner.token
    }

    async fn get(&self, path: &str, token: &str) -> TestResponse {
        self.fixture.sso.client.get_auth(path, token).await
    }

    async fn patch(&self, path: &str, token: &str, body: &Value) -> TestResponse {
        self.fixture
            .sso
            .client
            .patch_json_auth(path, body, token)
            .await
    }

    async fn delete(&self, path: &str, token: &str) -> TestResponse {
        self.fixture.sso.client.delete_auth(path, token).await
    }

    async fn policy(&self) -> Value {
        answered(&self.get(POLICY, self.admin()).await)
    }

    async fn set_minimum(&self, body: Value) -> TestResponse {
        self.patch(POLICY, self.admin(), &body).await
    }

    async fn fleet(&self) -> Value {
        answered(&self.get(RUNNERS, self.admin()).await)
    }

    /// The runner as the fleet lists it.
    async fn listed(&self, runner: Uuid) -> Value {
        self.fleet().await["runners"]
            .as_array()
            .unwrap()
            .iter()
            .find(|listed| listed["id"] == runner.to_string())
            .cloned()
            .unwrap_or_else(|| panic!("runner {runner} is listed"))
    }

    async fn transition(&self, runner: impl Display, body: Value) -> TestResponse {
        self.patch(&runner_path(runner), self.admin(), &body).await
    }

    async fn remove(&self, runner: impl Display) -> TestResponse {
        self.delete(&runner_path(runner), self.admin()).await
    }

    async fn seed(&self, seed: Seed) -> Uuid {
        let pool = self.fixture.pool();
        let runner = runners::upsert(
            pool,
            &seed.name,
            Uuid::new_v4(),
            &Version::parse(seed.version).unwrap(),
            &Labels::parse_list(seed.labels).unwrap(),
        )
        .await
        .expect("the runner registers")
        .runner
        .id;
        sqlx::query("UPDATE fleet_runners SET state = $2 WHERE id = $1")
            .bind(runner)
            .bind(seed.state.as_str())
            .execute(pool)
            .await
            .unwrap();
        if !seed.connected {
            self.disconnect(runner).await;
        }
        runner
    }

    async fn disconnect(&self, runner: Uuid) {
        sqlx::query(OFFLINE)
            .bind(runner)
            .execute(self.fixture.pool())
            .await
            .unwrap();
    }

    async fn entries(&self, action: &str) -> Vec<Entry> {
        sqlx::query_as(
            "SELECT actor_id, organization_id, workspace_id, resource_type, resource_id, \
                 old_values, new_values \
             FROM audit_logs WHERE action = $1 ORDER BY sequence",
        )
        .bind(action)
        .fetch_all(self.fixture.pool())
        .await
        .unwrap()
    }

    async fn fleet_entries(&self) -> usize {
        let mut count = 0;
        for action in [
            actions::FLEET_POLICY_UPDATED,
            actions::RUNNER_UPDATED,
            actions::RUNNER_REMOVED,
            actions::RUNNER_LABELS_UPDATED,
        ] {
            count += self.entries(action).await.len();
        }
        count
    }

    fn organization_labels(&self) -> String {
        organization_labels(self.fixture.organization)
    }

    fn workspace_labels(&self) -> String {
        workspace_labels(self.fixture.organization, self.fixture.workspace)
    }

    /// Someone holding a custom role of `scope` with `permission` and what it requires: in the
    /// organization for an organization role, and in the fixture's workspace, as an organization
    /// member, for a workspace role.
    async fn holder(&self, scope: Scope, permission: Permission) -> Account {
        let fixture = &self.fixture;
        let names: Vec<String> = std::iter::once(permission)
            .chain(permission.requires())
            .map(|permission| permission.name().to_string())
            .collect();
        let grant = Grant::compose(scope, &names).expect("the grant composes");
        let name = format!("Role {}", Uuid::new_v4());
        let definition = Definition {
            name: &name,
            description: None,
            scope,
            grant: &grant,
        };
        let role = match custom_roles::create(
            fixture.pool(),
            fixture.organization,
            &definition,
            fixture.owner.id,
        )
        .await
        .expect("the role is created")
        {
            Creation::Created(role) => role.id,
            Creation::NameTaken => panic!("a fresh name is never taken"),
        };
        match scope {
            Scope::Workspace => {
                let account = fixture
                    .member(OrgRole::Member, Some(WorkspaceRole::Viewer))
                    .await;
                let outcome = workspace_members::change_role(
                    fixture.pool(),
                    fixture.workspace,
                    account.id,
                    Assignment::Custom(role),
                    fixture.owner.id,
                )
                .await
                .unwrap();
                assert!(
                    matches!(outcome, workspace_members::RoleChange::Applied(_)),
                    "{outcome:?}"
                );
                account
            }
            _ => {
                let account = fixture.member(OrgRole::Member, None).await;
                let outcome = organization_members::change_role(
                    fixture.pool(),
                    fixture.organization,
                    account.id,
                    Assignment::Custom(role),
                    fixture.owner.id,
                )
                .await
                .unwrap();
                assert!(
                    matches!(outcome, organization_members::RoleChange::Applied(_)),
                    "{outcome:?}"
                );
                account
            }
        }
    }

    async fn cleanup(self) {
        self.fixture.cleanup().await;
    }
}

fn runner_path(runner: impl Display) -> String {
    format!("{RUNNERS}/{runner}")
}

fn organization_labels(organization: Uuid) -> String {
    format!("/api/organizations/{organization}/{LABELS}")
}

fn workspace_labels(organization: Uuid, workspace: Uuid) -> String {
    format!("/api/organizations/{organization}/workspaces/{workspace}/{LABELS}")
}

fn answered(response: &TestResponse) -> Value {
    response.assert_status(StatusCode::OK);
    response.json_value()
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
        "an access refusal carries no fleet code: {}",
        response.text()
    );
}

fn locked_out(response: &TestResponse) {
    refused(response, StatusCode::CONFLICT, NO_RUNNER_LEFT);
    assert_eq!(response.json_value()["remaining"], 0, "{}", response.text());
    assert!(
        response.json_value()["error"].is_string(),
        "{}",
        response.text()
    );
}

fn fields(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("{value} is an object"))
        .keys()
        .cloned()
        .collect()
}

fn expected(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

fn available(value: &Value) -> Vec<(String, i64)> {
    value["available"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            assert_eq!(fields(entry), expected(&["label", "runners"]), "{entry}");
            (
                entry["label"].as_str().unwrap().to_string(),
                entry["runners"].as_i64().unwrap(),
            )
        })
        .collect()
}

fn counted(entries: &[(&str, i64)]) -> Vec<(String, i64)> {
    entries
        .iter()
        .map(|(label, runners)| ((*label).to_string(), *runners))
        .collect()
}

#[tokio::test]
async fn only_a_server_admin_reaches_the_fleet() {
    let fleet = Fleet::licensed().await;
    let organization_owner = fleet.fixture.member(OrgRole::Owner, None).await;
    let stranger = fleet.fixture.stranger().await;

    for account in [&organization_owner, &stranger] {
        let token = &account.token;
        for response in [
            fleet.get(POLICY, token).await,
            fleet
                .patch(POLICY, token, &json!({ "minimum_version": "1.0.0" }))
                .await,
            fleet.get(RUNNERS, token).await,
            fleet
                .patch(
                    &runner_path(fleet.runner),
                    token,
                    &json!({ "state": "cordoned", "confirm": true }),
                )
                .await,
            fleet.delete(&runner_path(fleet.runner), token).await,
        ] {
            uncoded(&response, StatusCode::FORBIDDEN);
        }
    }

    assert_eq!(fleet.policy().await["minimum_version"], Value::Null);
    assert_eq!(fleet.listed(fleet.runner).await["state"], "active");
    assert_eq!(fleet.fleet_entries().await, 0);
    fleet.cleanup().await;
}

#[tokio::test]
async fn the_fleet_answers_in_the_contract_shape() {
    let fleet = Fleet::licensed().await;

    let policy = fleet.policy().await;
    assert_eq!(fields(&policy), expected(&POLICY_FIELDS), "{policy}");
    assert_eq!(policy["entitled"], true);
    assert_eq!(policy["enforced"], true);
    assert_eq!(policy["edition"], "enterprise");
    assert_eq!(policy["status"], "active");
    assert_eq!(policy["editable"], true);
    assert_eq!(policy["minimum_version"], Value::Null);
    assert_eq!(policy["updated_at"], Value::Null);

    let listing = fleet.fleet().await;
    assert_eq!(fields(&listing), expected(&RUNNERS_FIELDS), "{listing}");
    assert_eq!(listing["editable"], true);
    assert_eq!(listing["enforced"], true);
    assert_eq!(listing["current"], fleet.runner.to_string());
    let runners = listing["runners"].as_array().unwrap();
    assert_eq!(runners.len(), 1, "{listing}");
    let runner = &runners[0];
    assert_eq!(fields(runner), expected(&RUNNER_FIELDS), "{runner}");
    assert_eq!(runner["id"], fleet.runner.to_string());
    assert_eq!(runner["version"], VERSION);
    assert_eq!(runner["labels"], json!([CARRIED]));
    assert_eq!(runner["state"], "active");
    assert_eq!(runner["connected"], true);
    assert_eq!(runner["outdated"], false);
    assert_eq!(runner["serving"], true);
    assert_eq!(runner["code"], Value::Null);
    assert_eq!(runner["current"], true);
    assert_eq!(runner["runs"], 0);
    assert_eq!(runner["state_changed_at"], Value::Null);
    fleet.cleanup().await;
}

#[tokio::test]
async fn the_fleet_says_which_runners_serve_why_the_others_dont_and_which_answered() {
    let fleet = Fleet::licensed().await;
    let cordoned = fleet
        .seed(Seed::new(VERSION, CARRIED, State::Cordoned))
        .await;
    let revoked = fleet
        .seed(Seed::new(VERSION, CARRIED, State::Revoked))
        .await;
    let outdated = fleet.seed(Seed::new("1.3.9", CARRIED, State::Active)).await;
    let offline = fleet.seed(Seed::active().offline()).await;
    let offline_cordoned = fleet
        .seed(Seed::new(VERSION, CARRIED, State::Cordoned).offline())
        .await;
    answered(
        &fleet
            .set_minimum(json!({ "minimum_version": VERSION }))
            .await,
    );

    let listing = fleet.fleet().await;
    assert_eq!(listing["current"], fleet.runner.to_string());
    let judged = |runner: Uuid| {
        let listed = listing["runners"]
            .as_array()
            .unwrap()
            .iter()
            .find(|listed| listed["id"] == runner.to_string())
            .unwrap_or_else(|| panic!("runner {runner} is listed"));
        (
            listed["serving"].clone(),
            listed["code"].clone(),
            listed["connected"].clone(),
            listed["outdated"].clone(),
            listed["current"].clone(),
        )
    };
    let judgement = |serving: bool, code: Option<&str>, connected: bool, outdated: bool| {
        (
            json!(serving),
            json!(code),
            json!(connected),
            json!(outdated),
            json!(false),
        )
    };
    assert_eq!(
        judged(fleet.runner),
        (
            json!(true),
            Value::Null,
            json!(true),
            json!(false),
            json!(true)
        )
    );
    assert_eq!(
        judged(cordoned),
        judgement(false, Some(RUNNER_CORDONED), true, false)
    );
    assert_eq!(
        judged(revoked),
        judgement(false, Some(RUNNER_REVOKED), true, false)
    );
    assert_eq!(
        judged(outdated),
        judgement(false, Some(RUNNER_OUTDATED), true, true)
    );
    assert_eq!(judged(offline), judgement(false, None, false, false));
    assert_eq!(
        judged(offline_cordoned),
        judgement(false, Some(RUNNER_CORDONED), false, false)
    );
    assert_eq!(listing["runners"].as_array().unwrap().len(), 6);
    fleet.cleanup().await;
}

#[tokio::test]
async fn an_entitled_admin_sets_and_clears_the_minimum_and_each_change_is_audited() {
    let fleet = Fleet::licensed().await;

    let set = answered(
        &fleet
            .set_minimum(json!({ "minimum_version": "1.2.0" }))
            .await,
    );
    assert_eq!(fields(&set), expected(&POLICY_FIELDS), "{set}");
    assert_eq!(set["minimum_version"], "1.2.0");
    assert!(set["updated_at"].is_string(), "{set}");
    assert_eq!(fleet.policy().await["minimum_version"], "1.2.0");
    let entries = fleet.entries(actions::FLEET_POLICY_UPDATED).await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    let entry = &entries[0];
    assert_eq!(entry.organization_id, None);
    assert_eq!(entry.workspace_id, None);
    assert_eq!(entry.actor_id, Some(fleet.fixture.owner.id));
    assert_eq!(entry.resource_type, resources::FLEET_POLICY);
    assert_eq!(entry.resource_id, None);
    assert_eq!(
        entry.old_values,
        Some(json!({ "minimum_version": Value::Null }))
    );
    assert_eq!(
        entry.new_values,
        Some(json!({ "minimum_version": "1.2.0" }))
    );

    answered(
        &fleet
            .set_minimum(json!({ "minimum_version": "1.2.0" }))
            .await,
    );
    assert_eq!(fleet.entries(actions::FLEET_POLICY_UPDATED).await.len(), 1);

    let cleared = answered(
        &fleet
            .set_minimum(json!({ "minimum_version": Value::Null }))
            .await,
    );
    assert_eq!(cleared["minimum_version"], Value::Null);
    assert_eq!(fleet.policy().await["minimum_version"], Value::Null);
    let entries = fleet.entries(actions::FLEET_POLICY_UPDATED).await;
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert_eq!(
        entries[1].old_values,
        Some(json!({ "minimum_version": "1.2.0" }))
    );
    assert_eq!(
        entries[1].new_values,
        Some(json!({ "minimum_version": Value::Null }))
    );

    answered(&fleet.set_minimum(json!({})).await);
    assert_eq!(fleet.entries(actions::FLEET_POLICY_UPDATED).await.len(), 2);
    fleet.cleanup().await;
}

#[tokio::test]
async fn a_minimum_that_is_not_a_plain_release_or_a_body_that_cannot_be_read_is_refused() {
    let fleet = Fleet::licensed().await;

    for minimum in ["1.4", "1.4.0-rc.1", "1.4.0+build.7", "garbage", ""] {
        refused(
            &fleet
                .set_minimum(json!({ "minimum_version": minimum }))
                .await,
            StatusCode::BAD_REQUEST,
            VERSION_INVALID,
        );
    }
    for body in [
        json!({ "minimum": "1.4.0" }),
        json!({ "minimum_version": "1.4.0", "confirm": true, "force": true }),
        json!({ "minimum_version": 1 }),
        json!({ "minimum_version": "1.4.0", "confirm": "yes" }),
    ] {
        refused(
            &fleet.set_minimum(body).await,
            StatusCode::BAD_REQUEST,
            BODY_INVALID,
        );
    }

    assert_eq!(fleet.policy().await["minimum_version"], Value::Null);
    assert_eq!(fleet.fleet_entries().await, 0);
    fleet.cleanup().await;
}

#[tokio::test]
async fn without_an_entitled_license_the_fleet_reads_but_nothing_changes() {
    for (license, edition, status, enforced) in [
        (License::Community, "community", "community", false),
        (License::Team, "team", "active", false),
        (License::Lapsed, "enterprise", "lapsed", true),
    ] {
        let fleet = Fleet::new(license).await;
        let seeded = fleet.seed(Seed::active().offline()).await;

        let policy = fleet.policy().await;
        assert_eq!(policy["entitled"], false, "{license:?}");
        assert_eq!(policy["enforced"], enforced, "{license:?}");
        assert_eq!(policy["edition"], edition, "{license:?}");
        assert_eq!(policy["status"], status, "{license:?}");
        assert_eq!(policy["editable"], false, "{license:?}");
        let listing = fleet.fleet().await;
        assert_eq!(listing["editable"], false, "{license:?}");
        assert_eq!(listing["enforced"], enforced, "{license:?}");
        assert_eq!(listing["runners"].as_array().unwrap().len(), 2);
        let labels = answered(&fleet.get(&fleet.organization_labels(), fleet.admin()).await);
        assert_eq!(labels["entitled"], false, "{license:?}");
        assert_eq!(labels["editable"], false, "{license:?}");

        for response in [
            fleet
                .set_minimum(json!({ "minimum_version": "1.0.0" }))
                .await,
            fleet.set_minimum(json!({ "minimum": "garbage" })).await,
            fleet
                .transition(
                    fleet.runner,
                    json!({ "state": "cordoned", "confirm": true }),
                )
                .await,
            fleet.transition(seeded, json!({ "state": "paused" })).await,
            fleet.remove(seeded).await,
            fleet
                .patch(
                    &fleet.organization_labels(),
                    fleet.admin(),
                    &json!({ "labels": [CARRIED] }),
                )
                .await,
            fleet
                .patch(
                    &fleet.workspace_labels(),
                    fleet.admin(),
                    &json!({ "labels": [CARRIED] }),
                )
                .await,
        ] {
            refused(&response, StatusCode::FORBIDDEN, LICENSE_REQUIRED);
        }

        assert_eq!(fleet.policy().await["minimum_version"], Value::Null);
        assert_eq!(fleet.listed(fleet.runner).await["state"], "active");
        assert_eq!(fleet.listed(seeded).await["state"], "active");
        assert_eq!(fleet.fleet_entries().await, 0, "{license:?}");
        fleet.cleanup().await;
    }
}

#[tokio::test]
async fn a_change_that_would_leave_no_connected_runner_needs_confirmation() {
    let fleet = Fleet::licensed().await;
    fleet.seed(Seed::active().offline()).await;
    fleet
        .seed(Seed::new(VERSION, CARRIED, State::Cordoned))
        .await;

    locked_out(
        &fleet
            .transition(fleet.runner, json!({ "state": "cordoned" }))
            .await,
    );
    locked_out(
        &fleet
            .transition(
                fleet.runner,
                json!({ "state": "revoked", "confirm": false }),
            )
            .await,
    );
    locked_out(
        &fleet
            .set_minimum(json!({ "minimum_version": "1.5.0" }))
            .await,
    );
    assert_eq!(fleet.listed(fleet.runner).await["state"], "active");
    assert_eq!(fleet.policy().await["minimum_version"], Value::Null);
    assert_eq!(fleet.fleet_entries().await, 0);

    let cordoned = answered(
        &fleet
            .transition(
                fleet.runner,
                json!({ "state": "cordoned", "confirm": true }),
            )
            .await,
    );
    assert_eq!(cordoned["state"], "cordoned");
    assert_eq!(cordoned["code"], RUNNER_CORDONED);
    let activated = answered(
        &fleet
            .transition(fleet.runner, json!({ "state": "active" }))
            .await,
    );
    assert_eq!(activated["state"], "active");

    let raised = answered(
        &fleet
            .set_minimum(json!({ "minimum_version": "1.5.0", "confirm": true }))
            .await,
    );
    assert_eq!(raised["minimum_version"], "1.5.0");
    assert_eq!(fleet.listed(fleet.runner).await["code"], RUNNER_OUTDATED);
    let lowered = answered(
        &fleet
            .set_minimum(json!({ "minimum_version": "1.4.5" }))
            .await,
    );
    assert_eq!(lowered["minimum_version"], "1.4.5");
    let cleared = answered(
        &fleet
            .set_minimum(json!({ "minimum_version": Value::Null }))
            .await,
    );
    assert_eq!(cleared["minimum_version"], Value::Null);

    let revoked = answered(
        &fleet
            .transition(fleet.runner, json!({ "state": "revoked", "confirm": true }))
            .await,
    );
    assert_eq!(revoked["state"], "revoked");
    assert_eq!(revoked["serving"], false);
    let restored = answered(
        &fleet
            .transition(fleet.runner, json!({ "state": "active" }))
            .await,
    );
    assert_eq!(restored["state"], "active");
    assert_eq!(restored["serving"], true);

    assert_eq!(fleet.entries(actions::RUNNER_UPDATED).await.len(), 4);
    assert_eq!(fleet.entries(actions::FLEET_POLICY_UPDATED).await.len(), 3);
    fleet.cleanup().await;
}

#[tokio::test]
async fn another_connected_current_runner_lets_a_reducing_change_through() {
    let fleet = Fleet::licensed().await;
    fleet.seed(Seed::new("1.6.0", CARRIED, State::Active)).await;

    let cordoned = answered(
        &fleet
            .transition(fleet.runner, json!({ "state": "cordoned" }))
            .await,
    );
    assert_eq!(cordoned["state"], "cordoned");
    answered(
        &fleet
            .transition(fleet.runner, json!({ "state": "active" }))
            .await,
    );
    let raised = answered(
        &fleet
            .set_minimum(json!({ "minimum_version": "1.5.0" }))
            .await,
    );
    assert_eq!(raised["minimum_version"], "1.5.0");
    fleet.cleanup().await;
}

#[tokio::test]
async fn cordoning_revoking_and_activating_a_runner_each_audit_the_change() {
    let fleet = Fleet::licensed().await;
    let seed = Seed::active();
    let name = seed.name.clone();
    let runner = fleet.seed(seed).await;

    for (from, to) in [
        ("active", "cordoned"),
        ("cordoned", "revoked"),
        ("revoked", "active"),
    ] {
        let changed = answered(&fleet.transition(runner, json!({ "state": to })).await);
        assert_eq!(fields(&changed), expected(&RUNNER_FIELDS), "{changed}");
        assert_eq!(changed["id"], runner.to_string());
        assert_eq!(changed["state"], to);
        assert!(changed["state_changed_at"].is_string(), "{changed}");
        assert_eq!(fleet.listed(runner).await["state"], to);
        let entry = fleet
            .entries(actions::RUNNER_UPDATED)
            .await
            .pop()
            .expect("the change is audited");
        assert_eq!(entry.organization_id, None);
        assert_eq!(entry.workspace_id, None);
        assert_eq!(entry.actor_id, Some(fleet.fixture.owner.id));
        assert_eq!(entry.resource_type, resources::RUNNER);
        assert_eq!(entry.resource_id, Some(runner));
        assert_eq!(
            entry.old_values,
            Some(json!({ "name": name, "state": from }))
        );
        assert_eq!(entry.new_values, Some(json!({ "name": name, "state": to })));
    }

    answered(&fleet.transition(runner, json!({ "state": "active" })).await);
    assert_eq!(fleet.entries(actions::RUNNER_UPDATED).await.len(), 3);
    fleet.cleanup().await;
}

#[tokio::test]
async fn a_runner_change_is_refused_for_a_bad_state_body_or_runner() {
    let fleet = Fleet::licensed().await;
    let runner = fleet.seed(Seed::active()).await;

    for state in ["paused", "Active", "draining", ""] {
        refused(
            &fleet.transition(runner, json!({ "state": state })).await,
            StatusCode::BAD_REQUEST,
            STATE_INVALID,
        );
    }
    for body in [
        json!({}),
        json!({ "state": "cordoned", "reason": "maintenance" }),
        json!({ "state": 1 }),
        json!({ "state": "cordoned", "confirm": 1 }),
    ] {
        refused(
            &fleet.transition(runner, body).await,
            StatusCode::BAD_REQUEST,
            BODY_INVALID,
        );
    }
    uncoded(
        &fleet
            .transition(Uuid::new_v4(), json!({ "state": "cordoned" }))
            .await,
        StatusCode::NOT_FOUND,
    );
    uncoded(
        &fleet
            .transition("not-a-runner", json!({ "state": "cordoned" }))
            .await,
        StatusCode::NOT_FOUND,
    );

    assert_eq!(fleet.listed(runner).await["state"], "active");
    assert_eq!(fleet.fleet_entries().await, 0);
    fleet.cleanup().await;
}

#[tokio::test]
async fn only_an_offline_runner_is_removed_and_its_removal_is_audited() {
    let fleet = Fleet::licensed().await;
    let online = fleet.seed(Seed::active()).await;
    let seed = Seed::new("1.3.0", "gpu,region=eu", State::Revoked).offline();
    let name = seed.name.clone();
    let offline = fleet.seed(seed).await;

    refused(
        &fleet.remove(fleet.runner).await,
        StatusCode::CONFLICT,
        RUNNER_ONLINE,
    );
    refused(
        &fleet.remove(online).await,
        StatusCode::CONFLICT,
        RUNNER_ONLINE,
    );
    assert!(fleet.entries(actions::RUNNER_REMOVED).await.is_empty());

    fleet
        .remove(offline)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let runners = fleet.fleet().await["runners"].clone();
    assert!(
        runners
            .as_array()
            .unwrap()
            .iter()
            .all(|listed| listed["id"] != offline.to_string()),
        "{runners}"
    );
    let entries = fleet.entries(actions::RUNNER_REMOVED).await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    let entry = &entries[0];
    assert_eq!(entry.organization_id, None);
    assert_eq!(entry.actor_id, Some(fleet.fixture.owner.id));
    assert_eq!(entry.resource_type, resources::RUNNER);
    assert_eq!(entry.resource_id, Some(offline));
    assert_eq!(
        entry.old_values,
        Some(json!({
            "name": name,
            "version": "1.3.0",
            "labels": ["gpu", "region=eu"],
            "state": "revoked",
        }))
    );
    assert_eq!(entry.new_values, None);

    uncoded(&fleet.remove(offline).await, StatusCode::NOT_FOUND);
    uncoded(&fleet.remove(Uuid::new_v4()).await, StatusCode::NOT_FOUND);
    uncoded(&fleet.remove("not-a-runner").await, StatusCode::NOT_FOUND);
    fleet.cleanup().await;
}

#[tokio::test]
async fn members_read_the_organization_labels_and_only_the_policy_permission_changes_them() {
    let fleet = Fleet::licensed().await;
    let member = fleet.fixture.member(OrgRole::Member, None).await;
    let stranger = fleet.fixture.stranger().await;
    let change = json!({ "labels": [CARRIED] });

    let read = answered(&fleet.get(&fleet.organization_labels(), &member.token).await);
    assert_eq!(fields(&read), expected(&ORGANIZATION_FIELDS), "{read}");
    assert_eq!(read["entitled"], true);
    assert_eq!(read["enforced"], true);
    assert_eq!(read["editable"], false);
    assert_eq!(read["labels"], json!([]));
    assert_eq!(read["updated_at"], Value::Null);
    uncoded(
        &fleet
            .patch(&fleet.organization_labels(), &member.token, &change)
            .await,
        StatusCode::FORBIDDEN,
    );
    uncoded(
        &fleet
            .get(&fleet.organization_labels(), &stranger.token)
            .await,
        StatusCode::NOT_FOUND,
    );
    uncoded(
        &fleet
            .patch(&fleet.organization_labels(), &stranger.token, &change)
            .await,
        StatusCode::NOT_FOUND,
    );
    uncoded(
        &fleet
            .patch(
                &organization_labels(fleet.fixture.organization),
                &stranger.token,
                &json!({ "labels": ["NOT VALID"] }),
            )
            .await,
        StatusCode::NOT_FOUND,
    );
    assert!(
        fleet
            .entries(actions::RUNNER_LABELS_UPDATED)
            .await
            .is_empty()
    );

    let holder = fleet
        .holder(Scope::Organization, Permission::PoliciesUpdate)
        .await;
    let editable = answered(&fleet.get(&fleet.organization_labels(), &holder.token).await);
    assert_eq!(editable["editable"], true);
    let saved = answered(
        &fleet
            .patch(
                &fleet.organization_labels(),
                &holder.token,
                &json!({ "labels": ["region=eu", CARRIED, CARRIED] }),
            )
            .await,
    );
    assert_eq!(fields(&saved), expected(&ORGANIZATION_FIELDS), "{saved}");
    assert_eq!(saved["labels"], json!([CARRIED, "region=eu"]));
    assert_eq!(saved["editable"], true);
    assert!(saved["updated_at"].is_string(), "{saved}");
    let read = answered(&fleet.get(&fleet.organization_labels(), &member.token).await);
    assert_eq!(read["labels"], json!([CARRIED, "region=eu"]));

    let entries = fleet.entries(actions::RUNNER_LABELS_UPDATED).await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    let entry = &entries[0];
    assert_eq!(entry.organization_id, Some(fleet.fixture.organization));
    assert_eq!(entry.workspace_id, None);
    assert_eq!(entry.actor_id, Some(holder.id));
    assert_eq!(entry.resource_type, resources::ORGANIZATION_RUNNER_LABELS);
    assert_eq!(entry.resource_id, Some(fleet.fixture.organization));
    assert_eq!(entry.old_values, Some(json!({ "labels": [] })));
    assert_eq!(
        entry.new_values,
        Some(json!({ "labels": [CARRIED, "region=eu"] }))
    );

    answered(
        &fleet
            .patch(
                &fleet.organization_labels(),
                &holder.token,
                &json!({ "labels": ["region=eu", CARRIED] }),
            )
            .await,
    );
    assert_eq!(fleet.entries(actions::RUNNER_LABELS_UPDATED).await.len(), 1);
    let cleared = answered(
        &fleet
            .patch(
                &fleet.organization_labels(),
                fleet.admin(),
                &json!({ "labels": [] }),
            )
            .await,
    );
    assert_eq!(cleared["labels"], json!([]));
    let entries = fleet.entries(actions::RUNNER_LABELS_UPDATED).await;
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert_eq!(entries[1].new_values, Some(json!({ "labels": [] })));
    fleet.cleanup().await;
}

#[tokio::test]
async fn an_invalid_label_or_a_body_that_cannot_be_read_is_refused() {
    let fleet = Fleet::licensed().await;
    let too_long = "a".repeat(64);

    for path in [fleet.organization_labels(), fleet.workspace_labels()] {
        for label in ["GPU", "", "-gpu", "gpu label", "région", too_long.as_str()] {
            refused(
                &fleet
                    .patch(&path, fleet.admin(), &json!({ "labels": [CARRIED, label] }))
                    .await,
                StatusCode::BAD_REQUEST,
                LABEL_INVALID,
            );
        }
        for body in [
            json!({}),
            json!({ "labels": CARRIED }),
            json!({ "labels": [1] }),
            json!({ "labels": [CARRIED], "workspace": [] }),
        ] {
            refused(
                &fleet.patch(&path, fleet.admin(), &body).await,
                StatusCode::BAD_REQUEST,
                BODY_INVALID,
            );
        }
    }
    let longest = "a".repeat(63);
    let saved = answered(
        &fleet
            .patch(
                &fleet.organization_labels(),
                fleet.admin(),
                &json!({ "labels": [longest, "0.zone_tier-1=eu/west"] }),
            )
            .await,
    );
    assert_eq!(saved["labels"], json!(["0.zone_tier-1=eu/west", longest]));
    assert_eq!(fleet.entries(actions::RUNNER_LABELS_UPDATED).await.len(), 1);
    fleet.cleanup().await;
}

#[tokio::test]
async fn label_responses_never_describe_a_runner() {
    let fleet = Fleet::licensed().await;
    let seed = Seed::new("9.8.7", "gpu,region=eu", State::Active);
    let name = seed.name.clone();
    let seeded = fleet.seed(seed).await;
    let instances: Vec<Uuid> = sqlx::query_scalar("SELECT instance_id FROM fleet_runners")
        .fetch_all(fleet.fixture.pool())
        .await
        .unwrap();
    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM fleet_runners")
        .fetch_all(fleet.fixture.pool())
        .await
        .unwrap();
    assert!(names.contains(&name));
    let member = fleet
        .fixture
        .member(OrgRole::Member, Some(WorkspaceRole::Member))
        .await;
    let change = json!({ "labels": [CARRIED] });

    let responses = [
        fleet.get(&fleet.organization_labels(), &member.token).await,
        fleet.get(&fleet.workspace_labels(), &member.token).await,
        fleet
            .patch(&fleet.organization_labels(), fleet.admin(), &change)
            .await,
        fleet
            .patch(&fleet.workspace_labels(), fleet.admin(), &change)
            .await,
    ];

    for response in &responses {
        let text = answered(response).to_string();
        for name in &names {
            assert!(!text.contains(name.as_str()), "{name} leaks: {text}");
        }
        for id in [fleet.runner, seeded].into_iter().chain(instances.clone()) {
            assert!(!text.contains(&id.to_string()), "{id} leaks: {text}");
        }
        for version in [VERSION, "9.8.7"] {
            assert!(!text.contains(version), "{version} leaks: {text}");
        }
        assert!(!text.contains("\"runs\""), "{text}");
    }
    assert_eq!(
        fields(&responses[0].json_value()),
        expected(&ORGANIZATION_FIELDS)
    );
    assert_eq!(
        fields(&responses[1].json_value()),
        expected(&WORKSPACE_FIELDS)
    );
    fleet.cleanup().await;
}

#[tokio::test]
async fn a_workspace_of_another_organization_is_not_found_under_this_one() {
    let fleet = Fleet::licensed().await;
    let (other_organization, other_workspace) = fleet
        .fixture
        .another_organization(&fleet.fixture.owner)
        .await;
    let crossed = workspace_labels(fleet.fixture.organization, other_workspace);
    let change = json!({ "labels": [CARRIED] });

    uncoded(
        &fleet.get(&crossed, fleet.admin()).await,
        StatusCode::NOT_FOUND,
    );
    uncoded(
        &fleet.patch(&crossed, fleet.admin(), &change).await,
        StatusCode::NOT_FOUND,
    );
    let reversed = workspace_labels(other_organization, fleet.fixture.workspace);
    uncoded(
        &fleet.get(&reversed, fleet.admin()).await,
        StatusCode::NOT_FOUND,
    );
    uncoded(
        &fleet.patch(&reversed, fleet.admin(), &change).await,
        StatusCode::NOT_FOUND,
    );
    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workspace_runner_labels")
        .fetch_one(fleet.fixture.pool())
        .await
        .unwrap();
    assert_eq!(stored, 0);

    answered(
        &fleet
            .get(
                &workspace_labels(other_organization, other_workspace),
                fleet.admin(),
            )
            .await,
    );
    assert!(
        fleet
            .entries(actions::RUNNER_LABELS_UPDATED)
            .await
            .is_empty()
    );
    fleet.cleanup().await;
}

#[tokio::test]
async fn the_workspace_policy_permission_changes_the_workspace_labels() {
    let fleet = Fleet::licensed().await;
    let viewer = fleet
        .fixture
        .member(OrgRole::Member, Some(WorkspaceRole::Viewer))
        .await;
    let outsider = fleet.fixture.member(OrgRole::Member, None).await;
    let change = json!({ "labels": ["region=eu"] });

    let read = answered(&fleet.get(&fleet.workspace_labels(), &viewer.token).await);
    assert_eq!(read["editable"], false);
    uncoded(
        &fleet
            .patch(&fleet.workspace_labels(), &viewer.token, &change)
            .await,
        StatusCode::FORBIDDEN,
    );
    uncoded(
        &fleet.get(&fleet.workspace_labels(), &outsider.token).await,
        StatusCode::NOT_FOUND,
    );
    uncoded(
        &fleet
            .patch(&fleet.workspace_labels(), &outsider.token, &change)
            .await,
        StatusCode::NOT_FOUND,
    );

    let holder = fleet
        .holder(Scope::Workspace, Permission::WorkspacePoliciesUpdate)
        .await;
    let saved = answered(
        &fleet
            .patch(&fleet.workspace_labels(), &holder.token, &change)
            .await,
    );
    assert_eq!(saved["editable"], true);
    assert_eq!(saved["workspace"], json!(["region=eu"]));
    assert!(saved["updated_at"].is_string(), "{saved}");
    uncoded(
        &fleet
            .patch(&fleet.organization_labels(), &holder.token, &change)
            .await,
        StatusCode::FORBIDDEN,
    );

    let entries = fleet.entries(actions::RUNNER_LABELS_UPDATED).await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    let entry = &entries[0];
    assert_eq!(entry.organization_id, Some(fleet.fixture.organization));
    assert_eq!(entry.workspace_id, Some(fleet.fixture.workspace));
    assert_eq!(entry.actor_id, Some(holder.id));
    assert_eq!(entry.resource_type, resources::WORKSPACE_RUNNER_LABELS);
    assert_eq!(entry.resource_id, Some(fleet.fixture.workspace));
    assert_eq!(entry.old_values, Some(json!({ "labels": [] })));
    assert_eq!(entry.new_values, Some(json!({ "labels": ["region=eu"] })));
    fleet.cleanup().await;
}

#[tokio::test]
async fn the_workspace_view_combines_both_layers_and_counts_the_runners_that_match() {
    let fleet = Fleet::licensed().await;
    let both = "gpu,region=eu";
    fleet.seed(Seed::new(VERSION, both, State::Active)).await;
    fleet.seed(Seed::new("1.5.0", CARRIED, State::Active)).await;
    fleet.seed(Seed::new(VERSION, both, State::Cordoned)).await;
    fleet.seed(Seed::new(VERSION, both, State::Revoked)).await;
    fleet
        .seed(Seed::new(VERSION, both, State::Active).offline())
        .await;
    fleet.seed(Seed::new("1.3.0", both, State::Active)).await;
    fleet
        .seed(Seed::new(VERSION, "region=us", State::Active))
        .await;
    answered(
        &fleet
            .set_minimum(json!({ "minimum_version": VERSION }))
            .await,
    );

    let unset = answered(&fleet.get(&fleet.workspace_labels(), fleet.admin()).await);
    assert_eq!(unset["organization"], json!([]));
    assert_eq!(unset["workspace"], json!([]));
    assert_eq!(unset["effective"], json!([]));
    assert_eq!(unset["matching"], 4);
    assert_eq!(unset["updated_at"], Value::Null);

    let organization = answered(
        &fleet
            .patch(
                &fleet.organization_labels(),
                fleet.admin(),
                &json!({ "labels": [CARRIED] }),
            )
            .await,
    );
    assert_eq!(organization["matching"], 3);
    let saved = answered(
        &fleet
            .patch(
                &fleet.workspace_labels(),
                fleet.admin(),
                &json!({ "labels": ["region=eu", CARRIED] }),
            )
            .await,
    );

    for view in [
        saved,
        answered(&fleet.get(&fleet.workspace_labels(), fleet.admin()).await),
    ] {
        assert_eq!(fields(&view), expected(&WORKSPACE_FIELDS), "{view}");
        assert_eq!(view["organization"], json!([CARRIED]));
        assert_eq!(view["workspace"], json!([CARRIED, "region=eu"]));
        assert_eq!(view["effective"], json!([CARRIED, "region=eu"]));
        assert_eq!(
            available(&view),
            counted(&[(CARRIED, 5), ("region=eu", 3), ("region=us", 1)])
        );
        assert_eq!(view["matching"], 1);
        assert!(view["updated_at"].is_string(), "{view}");
    }
    let organization = answered(&fleet.get(&fleet.organization_labels(), fleet.admin()).await);
    assert_eq!(organization["labels"], json!([CARRIED]));
    assert_eq!(organization["matching"], 3);
    assert_eq!(
        available(&organization),
        counted(&[(CARRIED, 5), ("region=eu", 3), ("region=us", 1)])
    );

    let other = fleet.fixture.another_workspace().await;
    let inherited = answered(
        &fleet
            .get(
                &workspace_labels(fleet.fixture.organization, other),
                fleet.admin(),
            )
            .await,
    );
    assert_eq!(inherited["organization"], json!([CARRIED]));
    assert_eq!(inherited["workspace"], json!([]));
    assert_eq!(inherited["effective"], json!([CARRIED]));
    assert_eq!(inherited["matching"], 3);
    assert_eq!(inherited["updated_at"], Value::Null);
    fleet.cleanup().await;
}
