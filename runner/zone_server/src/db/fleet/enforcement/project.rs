use std::time::Duration;

use uuid::Uuid;

use super::{Fleet, UNENFORCED, enforced};
use crate::db::auto_projects::{self, Claim};
use crate::db::fleet::claimant::Claimant;
use crate::db::fleet::state::State;

const LEASE: Duration = Duration::from_secs(3600);

impl Fleet {
    async fn project(&self, workspace: Option<Uuid>) -> Uuid {
        sqlx::query_scalar(
            "INSERT INTO projects (workspace_id, name, auto) VALUES ($1, 'Fleet', true) \
             RETURNING id",
        )
        .bind(workspace)
        .fetch_one(self.pool())
        .await
        .expect("an auto project")
    }

    async fn claim_due(&self, claimant: Claimant) -> Vec<Claim> {
        auto_projects::claim_due(self.pool(), LEASE, 10, &[], claimant)
            .await
            .expect("the claim")
    }

    async fn claimed_by(&self, project: Uuid) -> Option<Uuid> {
        sqlx::query_scalar("SELECT auto_claim_runner_id FROM projects WHERE id = $1")
            .bind(project)
            .fetch_one(self.pool())
            .await
            .expect("the project")
    }

    async fn release(&self, claim: Claim) {
        assert!(
            auto_projects::release(self.pool(), claim)
                .await
                .expect("the release"),
            "the claim was released"
        );
    }
}

#[tokio::test]
async fn a_project_is_claimed_only_by_a_runner_that_may_drive_it() {
    let fleet = Fleet::new().await;
    let short = fleet.short().await;
    let eligible = fleet.runner(State::Active, "2.0.0", "gpu").await;
    let project = fleet.project(Some(fleet.workspace)).await;

    let mut refused = Vec::new();
    for (why, runner) in short {
        refused.push((why, fleet.claim_due(enforced(runner)).await));
    }
    let unregistered = fleet
        .claim_due(Claimant {
            runner: None,
            enforced: true,
        })
        .await;
    let claimed = fleet.claim_due(enforced(eligible)).await;
    let recorded = fleet.claimed_by(project).await;
    for claim in &claimed {
        fleet.release(*claim).await;
    }
    let (_, revoked) = short[1];
    let unenforced = fleet
        .claim_due(Claimant {
            runner: Some(revoked),
            enforced: false,
        })
        .await;
    let recorded_unenforced = fleet.claimed_by(project).await;
    for claim in &unenforced {
        fleet.release(*claim).await;
    }
    let unknown = fleet
        .claim_due(Claimant {
            runner: Some(Uuid::new_v4()),
            enforced: false,
        })
        .await;
    let recorded_unknown = fleet.claimed_by(project).await;
    fleet.remove().await;

    for (why, claims) in refused {
        assert!(claims.is_empty(), "a {why} runner claimed a project");
    }
    assert!(unregistered.is_empty(), "an unregistered runner claimed");
    assert_eq!(
        claimed
            .iter()
            .map(|claim| claim.project_id)
            .collect::<Vec<_>>(),
        [project]
    );
    assert_eq!(recorded, Some(eligible));
    assert_eq!(unenforced.len(), 1, "an unenforced claimant claims");
    assert_eq!(recorded_unenforced, Some(revoked));
    assert_eq!(unknown.len(), 1);
    assert_eq!(
        recorded_unknown, None,
        "an unknown runner is recorded as none"
    );
}

#[tokio::test]
async fn a_project_without_a_workspace_is_judged_on_the_runner_alone() {
    let fleet = Fleet::new().await;
    let active = fleet.runner(State::Active, "2.0.0", "").await;
    let revoked = fleet.runner(State::Revoked, "2.0.0", "").await;
    let project = fleet.project(None).await;

    let refused = fleet.claim_due(enforced(revoked)).await;
    let claimed = fleet.claim_due(enforced(active)).await;
    fleet.remove().await;

    assert!(refused.is_empty(), "a revoked runner claimed a project");
    assert_eq!(
        claimed
            .iter()
            .map(|claim| claim.project_id)
            .collect::<Vec<_>>(),
        [project],
        "a project without a workspace is driven under fleet policy with nothing set"
    );
}

#[tokio::test]
async fn a_revoked_runner_cannot_renew_its_drive() {
    let fleet = Fleet::new().await;
    let runner = fleet.runner(State::Active, "2.0.0", "").await;
    fleet.project(Some(fleet.workspace)).await;
    let [claim] = fleet.claim_due(enforced(runner)).await[..] else {
        panic!("the runner claims the project");
    };

    let before = auto_projects::renew(fleet.pool(), claim, enforced(runner))
        .await
        .expect("the renewal");
    sqlx::query("UPDATE fleet_runners SET state = $2 WHERE id = $1")
        .bind(runner)
        .bind(State::Revoked.as_str())
        .execute(fleet.pool())
        .await
        .expect("the runner is revoked");
    let after = auto_projects::renew(fleet.pool(), claim, enforced(runner))
        .await
        .expect("the renewal");
    let unenforced = auto_projects::renew(fleet.pool(), claim, UNENFORCED)
        .await
        .expect("the renewal");
    fleet.revoke(runner).await;
    let after_revoke = auto_projects::renew(fleet.pool(), claim, UNENFORCED)
        .await
        .expect("the renewal");
    fleet.remove().await;

    assert!(before, "an active runner renews its drive");
    assert!(!after, "a revoked runner renewed its drive");
    assert!(
        unenforced,
        "an unenforced renewal is fenced on the claim alone"
    );
    assert!(!after_revoke, "a revoke clears the drive's claim");
}
