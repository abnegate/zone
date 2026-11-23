use serde::{Deserialize, Deserializer};

use crate::scim::email::Email;
use crate::scim::error::Error;
use crate::scim::flag::Flag;
use crate::scim::name::Name;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDocument {
    pub user_name: String,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub name: Option<Name>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub emails: Vec<Email>,
    #[serde(default)]
    pub active: Option<Flag>,
}

impl UserDocument {
    pub fn primary_email(&self) -> Option<&str> {
        Email::primary(&self.emails).map(|email| email.value.as_str())
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.user_name.trim().is_empty() {
            return Err(Error::InvalidValue("userName can't be blank.".to_owned()));
        }
        Ok(())
    }
}

fn null_as_empty<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Email>, D::Error> {
    Ok(Option::<Vec<Email>>::deserialize(deserializer)?.unwrap_or_default())
}
