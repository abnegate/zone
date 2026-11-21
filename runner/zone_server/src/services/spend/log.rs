//! A row of LiteLLM's `/spend/logs/v2`.

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Deserializer};

use crate::db::spend::records::Record;

pub const FAILURE: &str = "failure";
pub const UNKNOWN: &str = "unknown";

const MICRODOLLARS_PER_DOLLAR: f64 = 1_000_000.0;
const NAIVE_FORMATS: [&str; 2] = ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"];

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Log {
    pub request_id: String,
    /// The token of the key the request was sent under.
    #[serde(default)]
    pub api_key: Option<String>,
    /// US dollars, 0 for a model LiteLLM has no price for.
    #[serde(default)]
    pub spend: Option<f64>,
    #[serde(default)]
    pub prompt_tokens: Option<i64>,
    #[serde(default)]
    pub completion_tokens: Option<i64>,
    #[serde(rename = "startTime", deserialize_with = "moment")]
    pub start_time: DateTime<Utc>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub model_group: Option<String>,
    #[serde(default)]
    pub custom_llm_provider: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

impl Log {
    /// The spend rounded half away from zero to whole microdollars, none below zero.
    pub fn microdollars(&self) -> i64 {
        let spend = self.spend.unwrap_or_default();
        if !spend.is_finite() {
            return 0;
        }
        ((spend * MICRODOLLARS_PER_DOLLAR).round() as i64).max(0)
    }

    /// The model group the request asked for, or the model LiteLLM sent it to.
    pub fn model(&self) -> &str {
        named(self.model_group.as_deref())
            .or_else(|| named(self.model.as_deref()))
            .unwrap_or(UNKNOWN)
    }

    pub fn provider(&self) -> &str {
        named(self.custom_llm_provider.as_deref()).unwrap_or(UNKNOWN)
    }

    pub fn failed(&self) -> bool {
        self.status.as_deref() == Some(FAILURE)
    }

    /// The record of this request, none when it was sent under no key.
    pub fn record(&self) -> Option<Record> {
        let token = named(self.api_key.as_deref())?;
        Some(Record {
            request_id: self.request_id.clone(),
            token: token.to_string(),
            model: self.model().to_string(),
            provider: self.provider().to_string(),
            spend_microdollars: self.microdollars(),
            prompt_tokens: self.prompt_tokens.unwrap_or_default().max(0),
            completion_tokens: self.completion_tokens.unwrap_or_default().max(0),
            failed: self.failed(),
            occurred_at: self.start_time,
        })
    }
}

fn named(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// An RFC 3339 moment, or one with no offset, which LiteLLM writes in UTC.
fn moment<'de, D: Deserializer<'de>>(deserializer: D) -> Result<DateTime<Utc>, D::Error> {
    let text = String::deserialize(deserializer)?;
    if let Ok(moment) = DateTime::parse_from_rfc3339(&text) {
        return Ok(moment.with_timezone(&Utc));
    }
    NAIVE_FORMATS
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(&text, format).ok())
        .map(|naive| naive.and_utc())
        .ok_or_else(|| serde::de::Error::custom(format!("unreadable startTime {text:?}")))
}
