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
pub mod notification;
pub mod pipeline;
pub mod review;
pub mod summary;

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::Notify;
use uuid::Uuid;
use zone_vcs::conflict::ConflictService;
use zone_vcs::pull_request::PrService;

use crate::config::{AutoProjectConfig, Config};
use crate::db::auto_projects;
use crate::state::AppState;

static WAKE: Notify = Notify::const_new();

/// How long one instance may hold a project before another may take it over.
/// A drive that reviews a change can take minutes; one that died takes this.
const CLAIM_LEASE: Duration = Duration::from_secs(15 * 60);

/// Projects one tick will claim.
const PROJECTS_PER_TICK: i64 = 10;

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
    pub pr: PrService,
    pub conflicts: ConflictService,
}

impl Services {
    pub fn from_config(config: &Config) -> Self {
        Self {
            pr: PrService::configured(config.github_api_url.clone()),
            conflicts: ConflictService::new(),
        }
    }

    /// Services addressing a stand-in for GitHub, for tests.
    pub fn standing_in_for(pr: PrService) -> Self {
        Self {
            pr,
            conflicts: ConflictService::new(),
        }
    }
}

pub fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let services = Arc::new(Services::from_config(state.config()));
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
    for project_id in due {
        if driving.insert(project_id, ()).is_some() {
            continue;
        }
        let state = state.clone();
        let services = Arc::clone(services);
        let config = Arc::clone(config);
        let driving = Arc::clone(driving);
        tokio::spawn(async move {
            if let Err(error) = driver::drive_project(&state, &services, &config, project_id).await
            {
                tracing::warn!(%project_id, %error, "Auto project drive failed; retrying next tick");
            }
            if let Err(error) = auto_projects::release(state.db(), project_id).await {
                tracing::warn!(%project_id, %error, "Could not release an auto project claim");
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
