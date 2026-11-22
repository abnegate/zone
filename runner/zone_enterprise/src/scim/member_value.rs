use serde::Deserialize;
use serde_json::Value;

use crate::scim::error::Error;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MemberValue {
    pub value: String,
}

impl MemberValue {
    pub(crate) fn list(value: &Value) -> Result<Vec<String>, Error> {
        match value {
            Value::Array(items) => items.iter().map(Self::one).collect(),
            Value::Object(_) | Value::String(_) => Self::one(value).map(|member| vec![member]),
            _ => Err(invalid()),
        }
    }

    fn one(value: &Value) -> Result<String, Error> {
        match value {
            Value::String(member) => Ok(member.clone()),
            Value::Object(_) => Self::deserialize(value)
                .map(|member| member.value)
                .map_err(|_| invalid()),
            _ => Err(invalid()),
        }
    }
}

fn invalid() -> Error {
    Error::InvalidValue("Each member needs a value holding the member's id.".to_owned())
}
