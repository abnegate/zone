pub mod client;
pub mod open;
pub mod s3;
pub mod syslog;
pub mod tls;
pub mod webhook;

use std::time::Duration;

use async_trait::async_trait;
use uuid::Uuid;
use zone_enterprise::audit::{Event, FailureCode};

use crate::db::audit::scope::Scope;

#[derive(Debug, Clone)]
pub struct Batch {
    pub destination_id: Uuid,
    pub scope: Scope,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: FailureCode,
    pub message: String,
    pub retry_after: Option<Duration>,
}

impl Failure {
    pub fn of(code: FailureCode) -> Self {
        Self::new(code, code.message())
    }

    pub fn new(code: FailureCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retry_after: None,
        }
    }
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn deliver(&self, batch: &Batch) -> Result<(), Failure>;
}
