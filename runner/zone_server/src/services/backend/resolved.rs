use zone_core::llm::LlmBackend;

use crate::services::login::identity::LoginIdentity;

/// A backend a session runs on, and the organization's login it runs under, when it runs under
/// one rather than an endpoint, the instance's agent or the host's sign-in.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub backend: LlmBackend,
    pub login: Option<LoginIdentity>,
}

impl Resolved {
    /// `backend`, run under no login of the organization's.
    pub fn unrouted(backend: LlmBackend) -> Self {
        Self {
            backend,
            login: None,
        }
    }
}
