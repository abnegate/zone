use serde::Deserialize;
use serde_json::{Map, Value};

use crate::scim::email::Email;
use crate::scim::error::Error;
use crate::scim::flag::Flag;
use crate::scim::op::Op;
use crate::scim::patch_request::PatchRequest;
use crate::scim::target::Target;
use crate::scim::urn;
use crate::scim::user_document::UserDocument;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserChanges {
    pub user_name: Option<String>,
    pub external_id: Option<Option<String>>,
    pub display_name: Option<Option<String>>,
    pub given_name: Option<Option<String>>,
    pub family_name: Option<Option<String>>,
    pub email: Option<String>,
    pub active: Option<bool>,
}

impl UserChanges {
    pub fn from_patch(request: &PatchRequest) -> Result<Self, Error> {
        let mut changes = Self::default();
        for operation in &request.operations {
            match (operation.op, operation.path()) {
                (Op::Remove, Some(path)) => changes.remove(&Target::parse(path)?)?,
                (Op::Remove, None) => {
                    return Err(Error::NoTarget(
                        "The remove operation needs a path.".to_owned(),
                    ));
                }
                (_, Some(path)) => {
                    changes.assign(&Target::parse(path)?, operation.required_value()?)?
                }
                (_, None) => changes.assign_all(operation.required_value()?)?,
            }
        }
        Ok(changes)
    }

    pub fn from_document(document: &UserDocument) -> Self {
        let name = document.name.clone().unwrap_or_default();
        Self {
            user_name: Some(document.user_name.clone()),
            external_id: Some(present(document.external_id.as_deref())),
            display_name: Some(present(document.display_name.as_deref())),
            given_name: Some(present(name.given_name.as_deref())),
            family_name: Some(present(name.family_name.as_deref())),
            email: document.primary_email().map(str::to_owned),
            active: document.active.map(bool::from),
        }
    }

    pub fn only_deactivation(&self) -> Self {
        Self {
            active: self.deactivates().then_some(false),
            ..Self::default()
        }
    }

    pub fn deactivates(&self) -> bool {
        self.active == Some(false)
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    fn assign_all(&mut self, value: &Value) -> Result<(), Error> {
        let object = value.as_object().ok_or_else(|| {
            Error::InvalidValue("A path-less operation needs an object as its value.".to_owned())
        })?;
        self.assign_object(object)
    }

    fn assign_object(&mut self, object: &Map<String, Value>) -> Result<(), Error> {
        for (key, value) in object {
            if key.eq_ignore_ascii_case(urn::USER) {
                if let Some(nested) = value.as_object() {
                    self.assign_object(nested)?;
                }
                continue;
            }
            self.assign(&Target::parse(key)?, value)?;
        }
        Ok(())
    }

    fn assign(&mut self, target: &Target, value: &Value) -> Result<(), Error> {
        match target {
            Target::Active => {
                let flag = Flag::parse(value).ok_or_else(|| {
                    Error::InvalidValue("active must be true or false.".to_owned())
                })?;
                self.active = Some(flag.into());
            }
            Target::UserName => {
                let user_name = text(value, "userName")?
                    .ok_or_else(|| Error::InvalidValue("userName can't be blank.".to_owned()))?;
                self.user_name = Some(user_name);
            }
            Target::ExternalId => self.external_id = Some(text(value, "externalId")?),
            Target::DisplayName => self.display_name = Some(text(value, "displayName")?),
            Target::GivenName => self.given_name = Some(text(value, "name.givenName")?),
            Target::FamilyName => self.family_name = Some(text(value, "name.familyName")?),
            Target::Formatted => {
                text(value, "name.formatted")?;
            }
            Target::Name => self.assign_name(value)?,
            Target::Email => self.email = Some(email(value)?),
            Target::Members | Target::Member(_) | Target::Ignored => {}
        }
        Ok(())
    }

    fn assign_name(&mut self, value: &Value) -> Result<(), Error> {
        let object = value
            .as_object()
            .ok_or_else(|| Error::InvalidValue("name must be an object.".to_owned()))?;
        for (key, value) in object {
            let target = if key.eq_ignore_ascii_case("givenName") {
                Target::GivenName
            } else if key.eq_ignore_ascii_case("familyName") {
                Target::FamilyName
            } else if key.eq_ignore_ascii_case("formatted") {
                Target::Formatted
            } else {
                Target::Ignored
            };
            self.assign(&target, value)?;
        }
        Ok(())
    }

    fn remove(&mut self, target: &Target) -> Result<(), Error> {
        let required = |attribute: &str| {
            Err(Error::Mutability(format!(
                "{attribute} is required, so it can't be removed."
            )))
        };
        match target {
            Target::Active => return required("active"),
            Target::UserName => return required("userName"),
            Target::Email => return required("emails"),
            Target::ExternalId => self.external_id = Some(None),
            Target::DisplayName => self.display_name = Some(None),
            Target::GivenName => self.given_name = Some(None),
            Target::FamilyName => self.family_name = Some(None),
            Target::Name => {
                self.given_name = Some(None);
                self.family_name = Some(None);
            }
            Target::Formatted | Target::Members | Target::Member(_) | Target::Ignored => {}
        }
        Ok(())
    }
}

fn present(value: Option<&str>) -> Option<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn text(value: &Value, attribute: &str) -> Result<Option<String>, Error> {
    match value {
        Value::String(text) => Ok(present(Some(text.as_str()))),
        Value::Null => Ok(None),
        _ => Err(Error::InvalidValue(format!(
            "{attribute} must be a string."
        ))),
    }
}

fn email(value: &Value) -> Result<String, Error> {
    let invalid = || Error::InvalidValue("emails needs an email address.".to_owned());
    let address = match value {
        Value::String(address) => present(Some(address.as_str())),
        Value::Object(_) => Email::deserialize(value)
            .ok()
            .and_then(|email| present(Some(email.value.as_str()))),
        Value::Array(_) => Vec::<Email>::deserialize(value)
            .ok()
            .and_then(|emails| Email::primary(&emails).map(|email| email.value.clone())),
        _ => None,
    };
    address.ok_or_else(invalid)
}
