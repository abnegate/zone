//! Driving an auto project: every agentic task it holds is run, reviewed and
//! merged without anyone in the loop.
//!
//! The driver is a dispatch loop like [`crate::workers::reminders`], not a
//! sweep in the housekeeping registry: it has to notice a finished run within
//! seconds, it mutates, and it claims each project through the database so
//! two instances never drive one project at once. A run that ends pokes it;
//! the tick covers what a poke cannot reach.

pub mod driver;
pub mod guidance;
pub mod lease;
pub mod notification;
pub mod pipeline;
pub mod review;
pub mod summary;

use std::sync::Arc;
use std::time::Duration;

use abnegate_notify::Notifier;
use abnegate_vcs::pull_request::PullRequestResult;
use abnegate_vcs::{ConflictService, PullRequestService};
use dashmap::DashMap;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::config::{AutoProjectConfig, Config};
use crate::db::auto_projects;
use crate::state::AppState;
use crate::workers::auto_project::review::outage::Outages;
use crate::workers::conflict;
use crate::workers::notify::{self, NotifyEnvironment, NotifySettings};

static WAKE: Notify = Notify::const_new();

/// How long a claim is left alone without a renewal before its instance is
/// taken to have died and another may drive the project.
pub const CLAIM_LEASE: Duration = Duration::from_secs(15 * 60);

/// How often a running drive renews its claim. Fifteen renewals fit in one
/// lease, so a slow or failed round trip to the database does not cost it.
const CLAIM_RENEWAL: Duration = Duration::from_secs(60);

/// Projects one tick will claim.
pub const PROJECTS_PER_TICK: i64 = 10;

/// Ask the driver to look again before its next tick.
pub fn poke(project_id: Uuid) {
    tracing::debug!(%project_id, "Auto project poked");
    WAKE.notify_one();
}

/// Poke the driver for every project a task belongs to, once the run that
/// ended has been written down. Fire and forget: a poke that is lost costs one
/// tick, and the tick is short.
pub fn poke_task(state: &AppState, task_id: Uuid) {
    let pool = state.db().clone();
    tokio::spawn(async move {
        match crate::db::tasks::get_task_project_ids(&pool, task_id).await {
            Ok(projects) => {
                for project in projects {
                    poke(project);
                }
            }
            Err(error) => tracing::debug!(%task_id, %error, "Could not poke the task's projects"),
        }
    });
}

/// What the driver talks to GitHub and git through.
#[derive(Clone)]
pub struct Services {
    pub pr: PullRequestService,
    pub conflicts: ConflictService,
    /// The operator's notification channels, built once from the environment
    /// rather than once per project per tick.
    pub channels: Vec<Arc<dyn Notifier>>,
    /// How long a delivery may take before it is given up on.
    pub notify_timeout: std::time::Duration,
    pub outages: Arc<Outages>,
}

impl Services {
    /// The pull request and conflict services the driver talks to GitHub
    /// through, and the channels its notices go out on.
    pub fn from_config(config: &Config) -> PullRequestResult<Self> {
        let environment = NotifyEnvironment::from_process();
        Ok(Self {
            pr: config.pull_requests()?,
            conflicts: conflict::service(),
            channels: notify::channels(&environment),
            notify_timeout: NotifySettings::resolve(&environment).timeout,
            outages: Arc::default(),
        })
    }

    /// Services addressing a stand-in for GitHub, for tests: notices reach the
    /// updates chat only.
    pub fn standing_in_for(pr: PullRequestService) -> Self {
        Self {
            pr,
            conflicts: conflict::service(),
            channels: Vec::new(),
            notify_timeout: std::time::Duration::from_secs(5),
            outages: Arc::default(),
        }
    }
}

/// Start the driver loop: a tick every interval and whenever a project is poked.
pub fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let services = match Services::from_config(state.config()) {
            Ok(services) => Arc::new(services),
            Err(error) => {
                tracing::error!(%error, "Auto project driver not started: GitHub origin is misconfigured");
                return;
            }
        };
        let config = Arc::new(state.config().auto.clone());
        let driving: Arc<DashMap<Uuid, ()>> = Arc::new(DashMap::new());
        let mut interval = tokio::time::interval(config.tick());
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        tracing::info!(tick_secs = config.tick_secs, "Auto project driver started");
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = WAKE.notified() => {}
            }
            tick(&state, &services, &config, &driving).await;
        }
    })
}

/// Claim what is due and drive each project on a task of its own, so one
/// slow review does not hold every other project's tick.
pub async fn tick(
    state: &AppState,
    services: &Arc<Services>,
    config: &Arc<AutoProjectConfig>,
    driving: &Arc<DashMap<Uuid, ()>>,
) {
    let due = match auto_projects::claim_due(state.db(), CLAIM_LEASE, PROJECTS_PER_TICK).await {
        Ok(due) => due,
        Err(error) => {
            tracing::warn!(%error, "Could not claim auto projects; retrying next tick");
            return;
        }
    };
    for claim in due {
        let project_id = claim.project_id;
        if driving.insert(project_id, ()).is_some() {
            continue;
        }
        let state = state.clone();
        let services = Arc::clone(services);
        let config = Arc::clone(config);
        let driving = Arc::clone(driving);
        tokio::spawn(async move {
            let drive = driver::drive_project(&state, &services, &config, project_id);
            if let Err(error) =
                lease::holding(state.db().clone(), claim, CLAIM_RENEWAL, drive).await
            {
                tracing::warn!(%project_id, %error, "Auto project drive failed; retrying next tick");
            }
            match auto_projects::release(state.db(), claim).await {
                Ok(true) => {}
                Ok(false) => tracing::warn!(
                    %project_id,
                    "An auto project claim was taken over before its drive ended"
                ),
                Err(error) => {
                    tracing::warn!(%project_id, %error, "Could not release an auto project claim")
                }
            }
            driving.remove(&project_id);
        });
    }
}

/// One drive of one project, for a caller that holds the claim itself.
pub async fn drive_once(
    state: &AppState,
    services: &Services,
    config: &AutoProjectConfig,
    project_id: Uuid,
) -> Result<(), String> {
    driver::drive_project(state, services, config, project_id).await
}
