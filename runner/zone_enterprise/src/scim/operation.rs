use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::scim::error::Error;
use crate::scim::folded::Folded;
use crate::scim::op::Op;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Operation {
    pub op: Op,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default, deserialize_with = "present_even_when_null")]
    pub value: Option<Value>,
}

impl Operation {
    pub(crate) fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    pub(crate) fn required_value(&self) -> Result<&Value, Error> {
        self.value.as_ref().ok_or_else(|| {
            Error::InvalidValue(format!("The {} operation needs a value.", self.op.as_str()))
        })
    }
}

fn present_even_when_null<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Value>, D::Error> {
    Folded::deserialize(deserializer).map(|Folded(value)| Some(value))
}
