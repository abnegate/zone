use std::num::NonZeroU32;

use crate::registry::repositories::Repositories;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrySettings {
    pub service: String,
    pub repositories: Repositories,
    pub generation: NonZeroU32,
}
