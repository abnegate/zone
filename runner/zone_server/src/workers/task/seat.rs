use chrono::{DateTime, Utc};
use uuid::Uuid;
use zone_core::llm::{AgentKind, LlmBackend};

use super::unprepared::Unprepared;
use super::{Failure, Fault, Prepared, resolve_model, routed};
use crate::db::tasks;
use crate::services::backend::{self, Resolved};
use crate::services::login::identity::LoginIdentity;
use crate::services::login::router;
use crate::services::route::Route;
use crate::state::AppState;

/// Where a run's attempts run: the login it is on, the backend and model that
/// login runs it on, and the logins a usage limit has taken out of it.
pub(super) struct Seat {
    route: Route,
    backend: LlmBackend,
    login: Option<LoginIdentity>,
    model: String,
    /// The login a limit just handed the run over to, resolved for the
    /// attempt that runs on it.
    next: Option<Resolved>,
    /// Logins a subscription limit refused since the run last backed off.
    /// Once it has, any of them may have reset, so none is left out after.
    spent: Vec<Uuid>,
    /// Logins refused for want of usage credits, which no wait restores.
    unfunded: Vec<Uuid>,
}

/// What one attempt runs on.
pub(super) struct Seated {
    pub(super) backend: LlmBackend,
    pub(super) login: Option<LoginIdentity>,
    pub(super) model: String,
}

impl Seat {
    /// A run that starts on what `route` prepared it on.
    pub(super) fn new(route: Route, prepared: &Prepared) -> Self {
        Self {
            route,
            backend: prepared.backend.clone(),
            login: prepared.login.clone(),
            model: prepared.model.clone(),
            next: None,
            spent: Vec::new(),
            unfunded: Vec::new(),
        }
    }

    /// What the next attempt of `run` runs on: the login a limit handed it
    /// over to, else the run's own as it stands now, else the one the router
    /// picks in its place. An attempt on another agent than the last runs on
    /// a model that agent chose, which is recorded as the run's.
    pub(super) async fn take(
        &mut self,
        state: &AppState,
        task: &tasks::TaskRow,
        run: Uuid,
        owner: Uuid,
    ) -> Result<Seated, Fault> {
        let resolved = match self.next.take() {
            Some(resolved) => resolved,
            None => {
                routed(
                    state,
                    &self.route,
                    &self.backend,
                    self.login.as_ref(),
                    &self.tried(),
                )
                .await?
            }
        };
        if agent(&resolved.backend) != agent(&self.backend) {
            self.model = self.model(state, task, &resolved.backend).await?;
            if !matches!(
                tasks::record_run_model(state.db(), run, owner, &self.model).await,
                Ok(true)
            ) {
                tracing::warn!(%run, "Could not record the model the run moved to");
            }
        }
        self.backend = resolved.backend;
        self.login = resolved.login;
        Ok(Seated {
            backend: self.backend.clone(),
            login: self.login.clone(),
            model: self.model.clone(),
        })
    }

    /// `fault`, the way an attempt ended, handed over to another login when a
    /// usage limit refused the attempt and another login can run the next.
    /// A run that is not handed over waits, if it waits at all, until the
    /// earliest of its organization's logins resets.
    pub(super) async fn after(&mut self, state: &AppState, fault: Fault) -> Fault {
        let fault = self.handed(state, fault).await;
        if fault.failure != Failure::Rerouted {
            self.spent.clear();
        }
        fault
    }

    async fn handed(&mut self, state: &AppState, fault: Fault) -> Fault {
        let (Some(limit), Some(login)) = (fault.limit.as_deref(), fault.login.as_deref()) else {
            return fault;
        };
        let refused = login.id;
        router::mark_limited(state, refused, limit, &self.model).await;
        if limit.credits {
            self.unfunded.push(refused);
        } else {
            self.spent.push(refused);
        }
        match self.route.resolve(state, &self.tried(), None).await {
            Ok(Resolved {
                backend,
                login: Some(to),
            }) => {
                self.next = Some(Resolved {
                    backend,
                    login: Some(to.clone()),
                });
                fault.rerouted(to)
            }
            Ok(_) => fault,
            Err(backend::Error::Limited { resets_at, .. }) => {
                self.reset(state, refused, fault, resets_at).await
            }
            Err(error) => {
                tracing::warn!(
                    login = %refused,
                    %error,
                    "No other login could take over a run a usage limit refused"
                );
                fault
            }
        }
    }

    /// `fault`, of an attempt on `refused` once every login tried since the
    /// run last backed off is out. One of them whose limit has reset since it
    /// was tried runs the next attempt at once; otherwise the run backs off
    /// until the earliest login resets, `resets_at` when none says sooner.
    ///
    /// Taking a login back costs an attempt where a handover does not, so a
    /// login whose limit is never recorded cannot hand the run back and forth
    /// for ever.
    async fn reset(
        &mut self,
        state: &AppState,
        refused: Uuid,
        fault: Fault,
        resets_at: Option<DateTime<Utc>>,
    ) -> Fault {
        let mut out = self.unfunded.clone();
        if !out.contains(&refused) {
            out.push(refused);
        }
        match self.route.resolve(state, &out, None).await {
            Ok(resolved) if resolved.login.is_some() => {
                self.next = Some(resolved);
                fault.undelayed()
            }
            Err(backend::Error::Limited {
                resets_at: earliest,
                ..
            }) => fault.deferred(earliest.or(resets_at)),
            Ok(_) | Err(_) => fault.deferred(resets_at),
        }
    }

    /// The model `task` runs on under `backend`, chosen as the run's first was.
    async fn model(
        &self,
        state: &AppState,
        task: &tasks::TaskRow,
        backend: &LlmBackend,
    ) -> Result<String, Fault> {
        let preferences = self
            .route
            .preferences(&state.config().comfyui.classifier_model);
        let endpoint = self
            .route
            .endpoint()
            .map_err(|unusable| Unprepared::Backend(unusable.into()))?;
        resolve_model(state, task, backend, endpoint, &preferences)
            .await
            .map_err(|message| Fault::from(Unprepared::Model(message)))
    }

    fn tried(&self) -> Vec<Uuid> {
        self.spent.iter().chain(&self.unfunded).copied().collect()
    }
}

fn agent(backend: &LlmBackend) -> Option<AgentKind> {
    match backend {
        LlmBackend::Cli { agent, .. } => Some(*agent),
        LlmBackend::Http => None,
    }
}
