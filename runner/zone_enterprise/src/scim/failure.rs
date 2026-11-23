use serde::{Serialize, Serializer};

use crate::scim::error::Error;
use crate::scim::scim_type::ScimType;
use crate::scim::urn;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    schemas: [&'static str; 1],
    #[serde(serialize_with = "as_text")]
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    scim_type: Option<ScimType>,
    detail: String,
}

impl Failure {
    pub fn new(status: u16, scim_type: Option<ScimType>, detail: impl Into<String>) -> Self {
        Self {
            schemas: [urn::ERROR],
            status,
            scim_type,
            detail: detail.into(),
        }
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn scim_type(&self) -> Option<ScimType> {
        self.scim_type
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl From<&Error> for Failure {
    fn from(error: &Error) -> Self {
        Self::new(error.status(), error.scim_type(), error.to_string())
    }
}

fn as_text<S: Serializer>(status: &u16, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(status)
}
