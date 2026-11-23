use std::fmt;
use std::sync::Arc;

use crate::http::distributor::Distributor;
use crate::http::registrar::Registrar;
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
    pub(crate) registrar: Option<Registrar>,
    pub(crate) distributor: Option<Distributor>,
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
            registrar: None,
            distributor: None,
        }
    }

    pub fn with_renewer(self, renewer: Renewer) -> Self {
        Self {
            renewer: Some(renewer),
            ..self
        }
    }

    pub fn with_registrar(self, registrar: Registrar) -> Self {
        Self {
            registrar: Some(registrar),
            ..self
        }
    }

    pub fn with_distributor(self, distributor: Distributor) -> Self {
        Self {
            distributor: Some(distributor),
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
            .field("registrar", &self.registrar)
            .field("distributor", &self.distributor)
            .finish_non_exhaustive()
    }
}
