use std::fmt;
use std::sync::Arc;

use crate::http::renewer::Renewer;
use crate::mail::Outbox;
use crate::polar::WebhookSecret;
use crate::products::Products;
use crate::signer::Signer;

pub struct Context {
    pub(crate) signer: Signer,
    pub(crate) secret: WebhookSecret,
    pub(crate) products: Products,
    pub(crate) outbox: Arc<dyn Outbox>,
    pub(crate) renewer: Option<Renewer>,
}

impl Context {
    pub fn new(
        signer: Signer,
        secret: WebhookSecret,
        products: Products,
        outbox: Arc<dyn Outbox>,
    ) -> Self {
        Self {
            signer,
            secret,
            products,
            outbox,
            renewer: None,
        }
    }

    pub fn with_renewer(self, renewer: Renewer) -> Self {
        Self {
            renewer: Some(renewer),
            ..self
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
            .field("renewer", &self.renewer)
            .finish_non_exhaustive()
    }
}
