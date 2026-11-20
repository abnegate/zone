use std::fmt;
use std::sync::Arc;

use crate::mail::Outbox;
use crate::polar::WebhookSecret;
use crate::signer::Signer;

pub struct Context {
    pub(crate) signer: Signer,
    pub(crate) secret: WebhookSecret,
    pub(crate) products: Vec<String>,
    pub(crate) outbox: Arc<dyn Outbox>,
}

impl Context {
    pub fn new(
        signer: Signer,
        secret: WebhookSecret,
        products: Vec<String>,
        outbox: Arc<dyn Outbox>,
    ) -> Self {
        Self {
            signer,
            secret,
            products,
            outbox,
        }
    }
}

impl fmt::Debug for Context {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Context")
            .field("signer", &self.signer)
            .field("secret", &self.secret)
            .field("products", &self.products)
            .finish_non_exhaustive()
    }
}
