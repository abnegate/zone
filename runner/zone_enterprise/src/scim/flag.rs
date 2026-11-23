use serde::de::{self, Unexpected};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

const EXPECTED: &str = "true or false, as a boolean or a string";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Flag(pub bool);

impl Flag {
    pub fn parse(value: &Value) -> Option<Self> {
        match value {
            Value::Bool(value) => Some(Self(*value)),
            Value::String(text) if text.eq_ignore_ascii_case("true") => Some(Self(true)),
            Value::String(text) if text.eq_ignore_ascii_case("false") => Some(Self(false)),
            _ => None,
        }
    }
}

impl From<Flag> for bool {
    fn from(flag: Flag) -> Self {
        flag.0
    }
}

impl From<bool> for Flag {
    fn from(value: bool) -> Self {
        Self(value)
    }
}

impl Serialize for Flag {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(self.0)
    }
}

impl<'de> Deserialize<'de> for Flag {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| {
            let unexpected = match &value {
                Value::String(text) => Unexpected::Str(text),
                Value::Number(_) => Unexpected::Other("a number"),
                Value::Null => Unexpected::Unit,
                Value::Array(_) => Unexpected::Seq,
                Value::Object(_) => Unexpected::Map,
                Value::Bool(value) => Unexpected::Bool(*value),
            };
            de::Error::invalid_value(unexpected, &EXPECTED)
        })
    }
}
