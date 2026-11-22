use std::sync::{Arc, OnceLock};

use super::client::ScimClient;

#[derive(Debug, Clone, Default)]
pub struct Slot(Arc<OnceLock<ScimClient>>);

impl Slot {
    pub fn fill(&self, client: &ScimClient) {
        self.0.get_or_init(|| client.clone());
    }

    pub fn client(&self) -> Option<&ScimClient> {
        self.0.get()
    }
}
