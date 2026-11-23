use serde::Deserialize;

use crate::scim::error::Error;
use crate::scim::member_value::MemberValue;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupDocument {
    pub display_name: String,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub members: Option<Vec<MemberValue>>,
}

impl GroupDocument {
    pub fn validate(&self) -> Result<(), Error> {
        if self.display_name.trim().is_empty() {
            return Err(Error::InvalidValue(
                "displayName can't be blank.".to_owned(),
            ));
        }
        Ok(())
    }
}
