use chrono::{DateTime, TimeDelta, Utc};

use crate::sso::saml::testing::{
    ACS_URL, ASSERTION_ID, Claim, ENTITY_ID, Key, REQUEST_ID, RESPONSE_ID, SERVICE_ENTITY_ID,
};
use crate::sso::saml::uri;
use crate::sso::testing::{EMAIL, NAME, SUBJECT};

const LIFETIME: TimeDelta = TimeDelta::minutes(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub response_id: String,
    pub assertion_id: String,
    pub issuer: String,
    pub destination: Option<String>,
    pub in_response_to: Option<String>,
    pub recipient: String,
    pub audiences: Vec<String>,
    pub name_id: String,
    pub name_id_format: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub claims: Vec<Claim>,
    pub issue_instant: DateTime<Utc>,
    pub not_before: DateTime<Utc>,
    pub not_on_or_after: DateTime<Utc>,
    pub confirmation_not_on_or_after: DateTime<Utc>,
    pub confirmation_not_before: Option<DateTime<Utc>>,
    pub confirmation_in_response_to: Option<String>,
    pub confirmation_method: String,
    pub status: String,
    pub sign_assertion: bool,
    pub sign_response: bool,
    pub signing_key: Key,
}

impl Options {
    pub fn at(now: DateTime<Utc>) -> Self {
        Self {
            response_id: RESPONSE_ID.to_owned(),
            assertion_id: ASSERTION_ID.to_owned(),
            issuer: ENTITY_ID.to_owned(),
            destination: Some(ACS_URL.to_owned()),
            in_response_to: Some(REQUEST_ID.to_owned()),
            recipient: ACS_URL.to_owned(),
            audiences: vec![SERVICE_ENTITY_ID.to_owned()],
            name_id: SUBJECT.to_owned(),
            name_id_format: uri::PERSISTENT.to_owned(),
            email: Some(EMAIL.to_owned()),
            display_name: Some(NAME.to_owned()),
            claims: Vec::new(),
            issue_instant: now,
            not_before: now,
            not_on_or_after: now + LIFETIME,
            confirmation_not_on_or_after: now + LIFETIME,
            confirmation_not_before: None,
            confirmation_in_response_to: Some(REQUEST_ID.to_owned()),
            confirmation_method: uri::BEARER.to_owned(),
            status: uri::SUCCESS.to_owned(),
            sign_assertion: true,
            sign_response: false,
            signing_key: Key::Primary,
        }
    }
}

impl Default for Options {
    fn default() -> Self {
        Self::at(Utc::now())
    }
}
