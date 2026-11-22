use chrono::{DateTime, Utc};

use crate::sso::saml::service_provider::ServiceProvider;

#[derive(Debug, Clone, Copy)]
pub(super) struct Expected<'a> {
    pub(super) entity_id: &'a str,
    pub(super) service: &'a ServiceProvider,
    pub(super) request_id: &'a str,
    pub(super) now: DateTime<Utc>,
}
