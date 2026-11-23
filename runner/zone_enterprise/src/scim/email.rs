use serde::{Deserialize, Deserializer, Serialize};

use crate::scim::flag::Flag;

pub const WORK: &str = "work";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Email {
    #[serde(deserialize_with = "blank_when_null")]
    pub value: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<Flag>,
}

impl Email {
    pub fn work(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            kind: Some(WORK.to_owned()),
            primary: Some(Flag(true)),
        }
    }

    pub fn primary(emails: &[Self]) -> Option<&Self> {
        let candidates: Vec<&Self> = emails
            .iter()
            .filter(|email| !email.value.trim().is_empty())
            .collect();
        candidates
            .iter()
            .find(|email| email.primary.is_some_and(bool::from))
            .or_else(|| candidates.iter().find(|email| email.is_work()))
            .or_else(|| candidates.first())
            .copied()
    }

    fn is_work(&self) -> bool {
        self.kind
            .as_deref()
            .is_some_and(|kind| kind.eq_ignore_ascii_case(WORK))
    }
}

fn blank_when_null<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Option::<String>::deserialize(deserializer).map(Option::unwrap_or_default)
}
