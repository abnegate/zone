use chrono::{TimeDelta, Utc};
use uuid::Uuid;
use zone_enterprise::fleet::Labels;
use zone_enterprise::{Keyring, Payload, Plan, SigningKey, TrustedKey, sign};

use crate::db::audit::{actions, resources};
use crate::db::fleet::claimant::Claimant;
use crate::db::fleet::runners;
use crate::db::fleet::state::State;
use crate::db::testing::Database;
use crate::services::fleet::identity::version;
use crate::services::fleet::{Identity, claimant, registration};
use crate::state::{AppState, test_config, unreachable_pool};

const KEY_ID: &str = "test-1";

pub(crate) struct Server {
    pub(crate) database: Database,
    pub(crate) state: AppState,
}

impl Server {
    pub(crate) async fn new(plan: Option<Plan>) -> Self {
        let database = Database::create("fleet_claimant").await;
        let state = AppState::new(test_config(), database.pool.clone(), None);
        state.disable_mcp();
        state.set_identity(Identity {
            name: format!("runner-{}", Uuid::new_v4().simple()),
            version: version(),
            labels: Labels::parse_list("gpu").unwrap(),
        });
        let signing = SigningKey::from_bytes(&[0x11; 32]);
        state.install_keyring(
            Keyring::new([TrustedKey::new(KEY_ID, signing.verifying_key().to_bytes()).unwrap()])
                .unwrap(),
        );
        if let Some(plan) = plan {
            let now = Utc::now();
            let license = sign(
                &Payload {
                    id: Uuid::new_v4(),
                    customer: "Acme Ltd".into(),
                    seats: 3,
                    issued_at: now - TimeDelta::days(1),
                    expires_at: now + TimeDelta::days(365),
                    key_id: KEY_ID.into(),
                    subscription: None,
                    plan: Some(plan),
                },
                &signing,
            );
            crate::db::licenses::replace(&database.pool, &license)
                .await
                .expect("the license");
        }
        Self { database, state }
    }

    async fn registered(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM fleet_runners")
            .fetch_one(&self.database.pool)
            .await
            .unwrap()
    }

    async fn registrations(&self) -> Vec<(Option<Uuid>, Option<Uuid>, Option<Uuid>)> {
        sqlx::query_as(
            "SELECT organization_id, actor_id, resource_id FROM audit_logs \
             WHERE action = $1 AND resource_type = $2",
        )
        .bind(actions::RUNNER_REGISTERED)
        .bind(resources::RUNNER)
        .fetch_all(&self.database.pool)
        .await
        .unwrap()
    }

    pub(crate) async fn remove(self) {
        drop(self.state);
        self.database.remove().await;
    }
}

#[tokio::test]
async fn an_enterprise_server_registers_on_its_first_claim() {
    let server = Server::new(Some(Plan::Enterprise)).await;
    assert_eq!(server.state.runner(), None);

    let claimant = claimant(&server.state).await;
    assert!(claimant.enforced);
    let runner = claimant.runner.expect("a registered runner");
    assert_eq!(server.state.runner(), Some(runner));
    let row = runners::get(&server.database.pool, runner)
        .await
        .unwrap()
        .expect("the runner row");
    assert_eq!(row.name, server.state.identity().name);
    assert_eq!(row.instance, server.state.instance());
    assert_eq!(row.version, version());
    assert_eq!(row.labels, Labels::parse_list("gpu").unwrap());
    assert_eq!(row.state, State::Active);
    assert_eq!(server.registrations().await, [(None, None, Some(runner))]);

    assert_eq!(
        crate::services::fleet::claimant(&server.state).await,
        Claimant {
            runner: Some(runner),
            instance: server.state.instance(),
            enforced: true,
        }
    );
    assert_eq!(server.registered().await, 1);
    server.remove().await;
}

#[tokio::test]
async fn a_server_without_enterprise_neither_enforces_nor_registers_to_claim() {
    for plan in [None, Some(Plan::Team)] {
        let server = Server::new(plan).await;
        assert_eq!(
            claimant(&server.state).await,
            Claimant {
                runner: None,
                instance: server.state.instance(),
                enforced: false,
            },
            "{plan:?}"
        );
        assert_eq!(server.registered().await, 0, "{plan:?}");
        server.remove().await;
    }
}

#[tokio::test]
async fn an_unenforced_claimant_still_names_its_registered_runner() {
    let server = Server::new(None).await;
    let runner = registration::register(&server.state).await.unwrap();
    assert_eq!(
        claimant(&server.state).await,
        Claimant {
            runner: Some(runner),
            instance: server.state.instance(),
            enforced: false,
        }
    );
    server.remove().await;
}

#[tokio::test]
async fn registering_again_refreshes_the_row_and_audits_only_the_first_time() {
    let server = Server::new(None).await;
    let first = registration::register(&server.state).await.unwrap();
    let seen: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT last_seen_at FROM fleet_runners WHERE id = $1")
            .bind(first)
            .fetch_one(&server.database.pool)
            .await
            .unwrap();
    let again = registration::register(&server.state).await.unwrap();
    assert_eq!(again, first);
    let refreshed: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT last_seen_at FROM fleet_runners WHERE id = $1")
            .bind(first)
            .fetch_one(&server.database.pool)
            .await
            .unwrap();
    assert!(refreshed > seen);
    assert_eq!(server.registrations().await.len(), 1);

    sqlx::query("DELETE FROM fleet_runners WHERE id = $1")
        .bind(first)
        .execute(&server.database.pool)
        .await
        .unwrap();
    let returned = registration::register(&server.state).await.unwrap();
    assert_ne!(returned, first);
    assert_eq!(server.state.runner(), Some(returned));
    assert_eq!(server.registrations().await.len(), 2);
    server.remove().await;
}

#[tokio::test]
async fn an_unreadable_license_enforces_and_an_unregistered_claimant_has_no_runner() {
    let state = AppState::new(test_config(), unreachable_pool(), None);
    state.disable_mcp();
    assert_eq!(
        claimant(&state).await,
        Claimant {
            runner: None,
            instance: state.instance(),
            enforced: true,
        }
    );
    assert_eq!(state.runner(), None);
}
