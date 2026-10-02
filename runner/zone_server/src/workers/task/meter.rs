use uuid::Uuid;
use zone_core::llm::Window;

use crate::services::login::router;
use crate::state::AppState;

/// The login an attempt runs under, whose usage windows the agent reports as
/// the turn runs.
#[derive(Clone, Copy)]
pub(super) struct Meter<'a> {
    pub(super) state: &'a AppState,
    pub(super) login: Uuid,
}

impl Meter<'_> {
    /// Records `window` over the login's snapshot.
    pub(super) async fn observe(self, window: &Window) {
        router::observe(self.state, self.login, window).await;
    }
}
