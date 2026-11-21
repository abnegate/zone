use zone_enterprise::Keyring;

use crate::polar::Client;

#[derive(Debug, Clone)]
pub struct Renewer {
    pub(crate) keyring: Keyring,
    pub(crate) client: Client,
}

impl Renewer {
    pub fn new(keyring: Keyring, client: Client) -> Self {
        Self { keyring, client }
    }
}
