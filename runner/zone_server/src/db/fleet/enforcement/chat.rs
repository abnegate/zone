use std::time::Duration;

use uuid::Uuid;
use zone_enterprise::fleet::Version;

use super::{Fleet, UNENFORCED, enforced};
use crate::db::context::{Error, Store};
use crate::db::fleet::claimant::Claimant;
use crate::db::fleet::shortfall::Shortfall;
use crate::db::fleet::state::State;

const LIFETIME: Duration = Duration::from_secs(30);

impl Fleet {
    fn store(&self, chat: Uuid) -> Store {
        Store::new(self.pool().clone(), chat, Some(self.workspace))
    }

    async fn acquire(&self, claimant: Claimant) -> (Uuid, Result<(), Error>) {
        let chat = self.chat().await;
        let acquired = self
            .store(chat)
            .claiming(claimant)
            .acquire(Uuid::new_v4(), LIFETIME)
            .await
            .map(|_| ());
        (chat, acquired)
    }

    async fn leased_to(&self, chat: Uuid) -> Option<Option<Uuid>> {
        sqlx::query_scalar("SELECT runner_id FROM chat_leases WHERE chat_id = $1")
            .bind(chat)
            .fetch_optional(self.pool())
            .await
            .expect("the chat's lease")
    }

    async fn live_leases(&self, runner: Uuid) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM chat_leases \
             WHERE runner_id = $1 AND expires_at > clock_timestamp()",
        )
        .bind(runner)
        .fetch_one(self.pool())
        .await
        .expect("the runner's leases")
    }
}

#[tokio::test]
async fn a_store_without_a_claimant_takes_no_lease() {
    let fleet = Fleet::new().await;
    let chat = fleet.chat().await;

    let acquired = fleet.store(chat).acquire(Uuid::new_v4(), LIFETIME).await;
    let leased = fleet.leased_to(chat).await;
    fleet.remove().await;

    assert!(
        matches!(acquired, Err(Error::Unclaimed)),
        "an unclaimed store took a lease: {acquired:?}"
    );
    assert_eq!(leased, None);
}

#[tokio::test]
async fn a_chat_runs_on_a_cordoned_runner_but_not_a_revoked_or_outdated_one() {
    let fleet = Fleet::new().await;
    let cordoned = fleet.runner(State::Cordoned, "2.0.0", "").await;
    let revoked = fleet.runner(State::Revoked, "2.0.0", "").await;
    let outdated = fleet.runner(State::Active, "1.9.0", "").await;
    fleet.minimum("2.0.0").await;
    fleet.require(fleet.workspace, "gpu").await;

    let (on_cordoned, on_cordoned_result) = fleet.acquire(enforced(cordoned)).await;
    let (on_revoked, on_revoked_result) = fleet.acquire(enforced(revoked)).await;
    let (_, on_outdated) = fleet.acquire(enforced(outdated)).await;
    let (_, unregistered) = fleet
        .acquire(Claimant {
            runner: None,
            enforced: true,
        })
        .await;
    let (unenforced, unenforced_result) = fleet
        .acquire(Claimant {
            runner: Some(revoked),
            enforced: false,
        })
        .await;
    let (_, unknown) = fleet.acquire(enforced(Uuid::new_v4())).await;
    let (unrecorded, unrecorded_result) = fleet.acquire(UNENFORCED).await;
    let leases = (
        fleet.leased_to(on_cordoned).await,
        fleet.leased_to(on_revoked).await,
        fleet.leased_to(unenforced).await,
        fleet.leased_to(unrecorded).await,
    );
    fleet.remove().await;

    assert!(
        on_cordoned_result.is_ok(),
        "a cordoned runner was refused a chat: {on_cordoned_result:?}"
    );
    assert!(
        matches!(on_revoked_result, Err(Error::Refused(Shortfall::Revoked))),
        "{on_revoked_result:?}"
    );
    assert!(
        matches!(
            &on_outdated,
            Err(Error::Refused(Shortfall::Outdated { version, minimum }))
                if *version == Version::parse("1.9.0").unwrap()
                    && *minimum == Version::parse("2.0.0").unwrap()
        ),
        "{on_outdated:?}"
    );
    assert!(
        matches!(unregistered, Err(Error::Refused(Shortfall::Unregistered))),
        "{unregistered:?}"
    );
    assert!(
        matches!(unknown, Err(Error::Refused(Shortfall::Unregistered))),
        "{unknown:?}"
    );
    assert!(
        unenforced_result.is_ok() && unrecorded_result.is_ok(),
        "an unenforced claimant was refused: {unenforced_result:?} {unrecorded_result:?}"
    );
    assert_eq!(
        leases,
        (Some(Some(cordoned)), None, Some(Some(revoked)), Some(None)),
        "a lease records the runner that took it, and a refusal writes none"
    );
}

#[tokio::test]
async fn a_refusal_reads_as_the_fleet_explains_it() {
    let fleet = Fleet::new().await;
    let revoked = fleet.runner(State::Revoked, "2.0.0", "").await;
    let (_, refused) = fleet.acquire(enforced(revoked)).await;
    fleet.remove().await;

    let refused = refused.expect_err("a revoked runner takes no chat");
    assert_eq!(
        refused.to_string(),
        crate::services::fleet::refusal::of(Shortfall::Revoked).message()
    );
    assert!(matches!(
        abnegate_agent::chat::ChatError::from(refused),
        abnegate_agent::chat::ChatError::Backend(_)
    ));
}

#[tokio::test]
async fn a_lease_racing_a_revoke_never_outlives_it() {
    let fleet = Fleet::new().await;
    for attempt in 0..50 {
        let runner = fleet.runner(State::Active, "2.0.0", "").await;
        let chat = fleet.chat().await;
        let store = fleet.store(chat).claiming(enforced(runner));

        let (acquired, ()) = tokio::join!(
            store.acquire(Uuid::new_v4(), LIFETIME),
            fleet.revoke(runner)
        );

        assert!(
            matches!(acquired, Ok(_) | Err(Error::Refused(Shortfall::Revoked))),
            "attempt {attempt}: {acquired:?}"
        );
        assert_eq!(
            fleet.live_leases(runner).await,
            0,
            "attempt {attempt}: a lease outlived its runner's revoke"
        );
    }
    fleet.remove().await;
}
