use serde_json::json;
use uuid::Uuid;

use super::{Fleet, INSTANCE, set, version};
use crate::db::fleet::candidate::Candidate;
use crate::db::fleet::claim::Outcome;
use crate::db::fleet::claimant::Claimant;
use crate::db::fleet::eligibility::{candidates, exists};
use crate::db::fleet::shortfall::Shortfall;
use crate::db::fleet::state::State;
use crate::db::fleet::{policy, revocation};
use crate::db::tasks::{self, TaskRunRow};

const ITERATIONS: usize = 50;

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct Held {
    owner: Option<Uuid>,
    runner_id: Option<Uuid>,
    status: String,
}

impl Fleet {
    async fn admit(&self, workspace: Uuid) -> TaskRunRow {
        let task: Uuid = sqlx::query_scalar(
            "INSERT INTO tasks (workspace_id, title, description) VALUES ($1, 'Run', 'Run') \
             RETURNING id",
        )
        .bind(workspace)
        .fetch_one(self.pool())
        .await
        .expect("a task");
        tasks::create_task_run(self.pool(), task)
            .await
            .expect("an admitted run")
    }

    async fn claim_as(
        &self,
        run: Uuid,
        owner: Uuid,
        runner: Option<Uuid>,
        enforced: bool,
    ) -> Outcome {
        tasks::claim_task_run(
            self.pool(),
            run,
            owner,
            Claimant {
                runner,
                instance: INSTANCE,
                enforced,
            },
        )
        .await
        .expect("the claim")
    }

    async fn claim(&self, run: Uuid, runner: Option<Uuid>, enforced: bool) -> Outcome {
        self.claim_as(run, Uuid::new_v4(), runner, enforced).await
    }

    async fn held(&self, run: Uuid) -> Held {
        sqlx::query_as("SELECT owner, runner_id, status FROM task_runs WHERE id = $1")
            .bind(run)
            .fetch_one(self.pool())
            .await
            .expect("the run")
    }

    async fn settle(&self, run: Uuid, seconds: f64) {
        sqlx::query(
            "UPDATE task_runs SET heartbeat_at = now() - make_interval(secs => $2) WHERE id = $1",
        )
        .bind(run)
        .bind(seconds)
        .execute(self.pool())
        .await
        .expect("a settled run");
    }

    async fn revoke(&self, runner: Uuid) {
        let mut transaction = self.pool().begin().await.unwrap();
        policy::lock(&mut transaction).await.unwrap();
        let revoked = revocation::revoke(&mut transaction, runner, None)
            .await
            .expect("the revoke")
            .expect("the runner exists");
        transaction.commit().await.unwrap();
        let _ = revoked.publish();
    }
}

#[tokio::test]
async fn an_unenforced_claim_takes_a_run_on_any_runner_and_records_it() {
    let fleet = Fleet::new().await;
    let (_, workspace) = fleet.workspace().await;
    fleet.minimum(Some("2.0.0")).await;
    fleet.require(None, Some(workspace), "gpu").await;
    let revoked = fleet.runner("revoked", "2.0.0", "gpu").await;
    let outdated = fleet.runner("outdated", "1.0.0", "").await;
    fleet.set(revoked.id, State::Revoked).await;

    for (runner, recorded) in [
        (Some(revoked.id), Some(revoked.id)),
        (Some(outdated.id), Some(outdated.id)),
        (None, None),
        (Some(Uuid::new_v4()), None),
    ] {
        let run = fleet.admit(workspace).await;
        assert_eq!(
            fleet.claim(run.id, runner, false).await,
            Outcome::Claimed,
            "{runner:?}"
        );
        assert_eq!(fleet.held(run.id).await.runner_id, recorded, "{runner:?}");
    }
    fleet.remove().await;
}

#[tokio::test]
async fn an_enforced_claim_is_refused_with_what_the_runner_lacks() {
    let fleet = Fleet::new().await;
    let (organization, workspace) = fleet.workspace().await;
    fleet.require(Some(organization), None, "region=eu").await;
    fleet.require(None, Some(workspace), "gpu").await;
    fleet.minimum(Some("1.4.0")).await;
    let cordoned = fleet.runner("cordoned", "1.4.0", "gpu,region=eu").await;
    let revoked = fleet.runner("revoked", "1.4.0", "gpu,region=eu").await;
    let outdated = fleet.runner("outdated", "1.3.0", "gpu,region=eu").await;
    let unlabelled = fleet.runner("unlabelled", "1.4.0", "gpu").await;
    let eligible = fleet
        .runner("eligible", "1.4.0", "extra,gpu,region=eu")
        .await;
    fleet.set(cordoned.id, State::Cordoned).await;
    fleet.set(revoked.id, State::Revoked).await;
    let run = fleet.admit(workspace).await;

    for (runner, shortfall) in [
        (Some(cordoned.id), Shortfall::Cordoned),
        (Some(revoked.id), Shortfall::Revoked),
        (
            Some(outdated.id),
            Shortfall::Outdated {
                version: version("1.3.0"),
                minimum: version("1.4.0"),
            },
        ),
        (
            Some(unlabelled.id),
            Shortfall::LabelsMissing {
                missing: set("region=eu"),
            },
        ),
        (None, Shortfall::Unregistered),
        (Some(Uuid::new_v4()), Shortfall::Unregistered),
    ] {
        assert_eq!(
            fleet.claim(run.id, runner, true).await,
            Outcome::Refused(shortfall),
            "{runner:?}"
        );
        assert_eq!(
            fleet.held(run.id).await,
            Held {
                owner: None,
                runner_id: None,
                status: "running".into(),
            },
            "a refused claim changed the run"
        );
    }

    assert_eq!(
        fleet.claim(run.id, Some(eligible.id), true).await,
        Outcome::Claimed
    );
    assert_eq!(fleet.held(run.id).await.runner_id, Some(eligible.id));
    assert_eq!(
        fleet.claim(run.id, Some(eligible.id), true).await,
        Outcome::Unavailable,
        "a claimed run is no one's to claim, so nothing falls short of it"
    );
    assert_eq!(
        fleet.claim(run.id, Some(cordoned.id), true).await,
        Outcome::Unavailable
    );
    fleet.remove().await;
}

#[tokio::test]
async fn a_claim_is_judged_on_the_claiming_processs_own_registration() {
    let fleet = Fleet::new().await;
    let (_, workspace) = fleet.workspace().await;
    fleet.require(None, Some(workspace), "gpu").await;
    let runner = fleet.runner("shared", "1.4.0", "").await;
    let other = fleet
        .register("shared", Uuid::new_v4(), "1.4.0", "gpu")
        .await
        .runner;
    let run = fleet.admit(workspace).await;

    let taken_over = fleet.claim(run.id, Some(runner.id), true).await;
    let untouched = fleet.held(run.id).await;
    fleet.runner("shared", "1.4.0", "").await;
    let registered_again = fleet.claim(run.id, Some(runner.id), true).await;
    fleet.remove().await;

    assert_eq!(other.id, runner.id, "both processes share the runner's row");
    assert_eq!(
        taken_over,
        Outcome::Refused(Shortfall::Unregistered),
        "a process claimed on another process's labels"
    );
    assert_eq!(untouched.owner, None);
    assert_eq!(
        registered_again,
        Outcome::Refused(Shortfall::LabelsMissing {
            missing: set("gpu")
        }),
        "a process registered again is judged on its own labels"
    );
}

#[tokio::test]
async fn a_run_whose_admission_lapsed_is_unavailable_to_every_claimant() {
    let fleet = Fleet::new().await;
    let (_, workspace) = fleet.workspace().await;
    let cordoned = fleet.runner("cordoned", "0.1.0", "").await;
    fleet.set(cordoned.id, State::Cordoned).await;
    let run = fleet.admit(workspace).await;
    fleet.settle(run.id, 61.0).await;

    assert_eq!(
        fleet.claim(run.id, Some(cordoned.id), true).await,
        Outcome::Unavailable
    );
    assert_eq!(fleet.claim(run.id, None, false).await, Outcome::Unavailable);
    fleet.remove().await;
}

#[tokio::test]
async fn another_organizations_labels_never_constrain_a_run() {
    let fleet = Fleet::new().await;
    let (_, own) = fleet.workspace().await;
    let (other_organization, other) = fleet.workspace().await;
    fleet.require(Some(other_organization), None, "gpu").await;
    fleet.require(None, Some(other), "ssd").await;
    let plain = fleet.runner("plain", "0.1.0", "").await;
    let ours = fleet.admit(own).await;
    let theirs = fleet.admit(other).await;

    assert_eq!(
        fleet.claim(ours.id, Some(plain.id), true).await,
        Outcome::Claimed
    );
    assert_eq!(
        fleet.claim(theirs.id, Some(plain.id), true).await,
        Outcome::Refused(Shortfall::LabelsMissing {
            missing: set("gpu,ssd"),
        })
    );
    fleet.remove().await;
}

#[tokio::test]
async fn a_claim_racing_a_revoke_leaves_no_live_run_on_the_revoked_runner() {
    let fleet = Fleet::new().await;
    let (_, workspace) = fleet.workspace().await;

    for iteration in 0..ITERATIONS {
        let runner = fleet
            .runner(&format!("racer-{iteration}"), "0.1.0", "")
            .await;
        let run = fleet.admit(workspace).await;
        let (claim, ()) = tokio::join!(
            fleet.claim(run.id, Some(runner.id), true),
            fleet.revoke(runner.id)
        );
        assert!(
            matches!(
                claim,
                Outcome::Claimed | Outcome::Refused(Shortfall::Revoked)
            ),
            "{claim:?}"
        );
    }

    let live: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_runs run JOIN fleet_runners runner ON runner.id = run.runner_id \
         WHERE runner.state = $1 AND run.status IN ('running', 'waiting')",
    )
    .bind(State::Revoked.as_str())
    .fetch_one(fleet.pool())
    .await
    .unwrap();
    fleet.remove().await;
    assert_eq!(live, 0, "a revoked runner kept a live run");
}

#[tokio::test]
async fn two_claims_of_one_run_take_it_once() {
    let fleet = Fleet::new().await;
    let (_, workspace) = fleet.workspace().await;
    let first = fleet.runner("first", "0.1.0", "").await;
    let second = fleet.runner("second", "0.1.0", "").await;

    for _ in 0..ITERATIONS {
        let run = fleet.admit(workspace).await;
        let claims = tokio::join!(
            fleet.claim(run.id, Some(first.id), true),
            fleet.claim(run.id, Some(second.id), true)
        );
        let winner = match claims {
            (Outcome::Claimed, Outcome::Unavailable) => first.id,
            (Outcome::Unavailable, Outcome::Claimed) => second.id,
            claims => panic!("not exactly one claim: {claims:?}"),
        };
        assert_eq!(fleet.held(run.id).await.runner_id, Some(winner));
    }
    fleet.remove().await;
}

#[tokio::test]
async fn failing_a_refused_run_leaves_a_pickup_its_claim() {
    let fleet = Fleet::new().await;
    let (_, workspace) = fleet.workspace().await;
    fleet.require(None, Some(workspace), "gpu").await;
    let refused = fleet.runner("refused", "0.1.0", "").await;
    let picker = fleet.runner("picker", "0.1.0", "gpu").await;
    let fail = |run: Uuid| {
        let pool = fleet.pool().clone();
        async move {
            tasks::complete_owned_task_run(&pool, run, None, tasks::FAILED, Some("No runner"), None)
                .await
                .expect("the fail")
                .is_some()
        }
    };

    let run = fleet.admit(workspace).await;
    assert!(matches!(
        fleet.claim(run.id, Some(refused.id), true).await,
        Outcome::Refused(Shortfall::LabelsMissing { .. })
    ));
    let owner = Uuid::new_v4();
    assert_eq!(
        fleet.claim_as(run.id, owner, Some(picker.id), true).await,
        Outcome::Claimed
    );
    assert!(!fail(run.id).await, "the fail landed on a picked-up run");
    assert_eq!(
        fleet.held(run.id).await,
        Held {
            owner: Some(owner),
            runner_id: Some(picker.id),
            status: "running".into(),
        }
    );

    for _ in 0..ITERATIONS {
        let run = fleet.admit(workspace).await;
        let (failed, claim) =
            tokio::join!(fail(run.id), fleet.claim(run.id, Some(picker.id), true));
        let held = fleet.held(run.id).await;
        match (failed, claim) {
            (true, Outcome::Unavailable) => assert_eq!(held.status, tasks::FAILED),
            (false, Outcome::Claimed) => assert_eq!(
                (held.status.as_str(), held.runner_id),
                ("running", Some(picker.id))
            ),
            outcome => panic!("the fail and the pickup both or neither landed: {outcome:?}"),
        }
    }
    fleet.remove().await;
}

#[tokio::test]
async fn a_resumed_run_keeps_its_runner_and_the_next_run_is_checked_again() {
    let fleet = Fleet::new().await;
    let (_, workspace) = fleet.workspace().await;
    let runner = fleet.runner("runner", "0.1.0", "").await;
    let run = fleet.admit(workspace).await;
    let owner = Uuid::new_v4();
    assert_eq!(
        fleet.claim_as(run.id, owner, Some(runner.id), true).await,
        Outcome::Claimed
    );
    assert!(
        tasks::park_task_run(fleet.pool(), run.id, owner, json!({"questions": []}))
            .await
            .unwrap()
    );
    fleet.set(runner.id, State::Cordoned).await;

    assert!(
        tasks::resume_task_run(fleet.pool(), run.id, owner)
            .await
            .unwrap(),
        "a cordon stopped a parked run resuming"
    );
    assert_eq!(
        fleet.held(run.id).await,
        Held {
            owner: Some(owner),
            runner_id: Some(runner.id),
            status: "running".into(),
        }
    );
    assert!(
        tasks::complete_owned_task_run(
            fleet.pool(),
            run.id,
            Some(owner),
            tasks::COMPLETED,
            None,
            None
        )
        .await
        .unwrap()
        .is_some()
    );
    let next = tasks::create_task_run(fleet.pool(), run.task_id)
        .await
        .unwrap();
    assert_eq!(
        fleet.claim(next.id, Some(runner.id), true).await,
        Outcome::Refused(Shortfall::Cordoned)
    );
    fleet.remove().await;
}

#[tokio::test]
async fn pickup_candidates_are_settled_unclaimed_runs_the_runner_may_take() {
    let fleet = Fleet::new().await;
    let (_, labelled) = fleet.workspace().await;
    let (_, plain) = fleet.workspace().await;
    let (_, unplaceable) = fleet.workspace().await;
    fleet.require(None, Some(labelled), "gpu").await;
    fleet.require(None, Some(unplaceable), "ssd").await;
    let picker = fleet.runner("picker", "0.1.0", "gpu").await;
    let cordoned = fleet.runner("cordoned", "0.1.0", "gpu").await;
    fleet.set(cordoned.id, State::Cordoned).await;

    let oldest = fleet.admit(labelled).await;
    fleet.settle(oldest.id, 10.0).await;
    let settled = fleet.admit(plain).await;
    fleet.settle(settled.id, 5.0).await;
    fleet.admit(labelled).await;
    let claimed = fleet.admit(labelled).await;
    assert!(fleet.claim(claimed.id, None, false).await.claimed());
    fleet.settle(claimed.id, 5.0).await;
    let lapsed = fleet.admit(labelled).await;
    fleet.settle(lapsed.id, 61.0).await;
    let placed_elsewhere = fleet.admit(unplaceable).await;
    fleet.settle(placed_elsewhere.id, 5.0).await;

    let candidate = |run: &TaskRunRow| Candidate {
        run: run.id,
        task: run.task_id,
    };
    assert_eq!(
        candidates(fleet.pool(), picker.id, 10).await.unwrap(),
        [candidate(&oldest), candidate(&settled)]
    );
    assert_eq!(
        candidates(fleet.pool(), picker.id, 1).await.unwrap(),
        [candidate(&oldest)]
    );
    assert!(
        candidates(fleet.pool(), cordoned.id, 10)
            .await
            .unwrap()
            .is_empty()
    );
    fleet.remove().await;
}

#[tokio::test]
async fn another_runner_exists_only_when_it_is_connected_active_current_and_labelled() {
    let fleet = Fleet::new().await;
    let (_, workspace) = fleet.workspace().await;
    fleet.require(None, Some(workspace), "gpu").await;
    fleet.minimum(Some("1.4.0")).await;
    let refused = fleet.runner("refused", "1.4.0", "").await.id;
    let run = fleet.admit(workspace).await.id;
    let another = || {
        let pool = fleet.pool().clone();
        async move { exists(&pool, run, Some(refused)).await.unwrap() }
    };

    assert!(!another().await);
    let cordoned = fleet.runner("cordoned", "1.4.0", "gpu").await;
    fleet.set(cordoned.id, State::Cordoned).await;
    let revoked = fleet.runner("revoked", "1.4.0", "gpu").await;
    fleet.set(revoked.id, State::Revoked).await;
    fleet.runner("outdated", "1.3.0", "gpu").await;
    fleet.runner("unlabelled", "1.4.0", "ssd").await;
    let offline = fleet.runner("offline", "1.4.0", "gpu").await;
    fleet.age(offline.id, 61.0).await;
    assert!(!another().await, "an ineligible runner counted");

    let eligible = fleet.runner("eligible", "1.4.0", "gpu").await;
    assert!(another().await);
    assert!(
        !exists(fleet.pool(), run, Some(eligible.id)).await.unwrap(),
        "the runner asking counted itself"
    );
    assert!(!exists(fleet.pool(), Uuid::new_v4(), None).await.unwrap());
    fleet.remove().await;
}
