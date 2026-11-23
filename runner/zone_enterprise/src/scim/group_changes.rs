use serde_json::{Map, Value};

use crate::scim::error::Error;
use crate::scim::group_document::GroupDocument;
use crate::scim::member_changes::MemberChanges;
use crate::scim::member_value::MemberValue;
use crate::scim::op::Op;
use crate::scim::patch_request::PatchRequest;
use crate::scim::target::Target;
use crate::scim::urn;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupChanges {
    pub display_name: Option<String>,
    pub external_id: Option<Option<String>>,
    pub members: MemberChanges,
}

impl GroupChanges {
    pub fn from_patch(request: &PatchRequest) -> Result<Self, Error> {
        let mut changes = Self::default();
        for operation in &request.operations {
            match (operation.op, operation.path()) {
                (Op::Remove, Some(path)) => {
                    changes.remove(&Target::parse(path)?, operation.value.as_ref())?;
                }
                (Op::Remove, None) => {
                    return Err(Error::NoTarget(
                        "The remove operation needs a path.".to_owned(),
                    ));
                }
                (op, Some(path)) => {
                    changes.assign(op, &Target::parse(path)?, operation.required_value()?)?;
                }
                (op, None) => {
                    let object = operation.required_value()?.as_object().ok_or_else(|| {
                        Error::InvalidValue(
                            "A path-less operation needs an object as its value.".to_owned(),
                        )
                    })?;
                    changes.assign_object(op, object)?;
                }
            }
        }
        Ok(changes)
    }

    pub fn from_document(document: &GroupDocument) -> Self {
        let mut members = MemberChanges::default();
        if let Some(listed) = &document.members {
            members.replace_with(listed.iter().map(|member| member.value.clone()).collect());
        }
        Self {
            display_name: Some(document.display_name.clone()),
            external_id: Some(
                document
                    .external_id
                    .as_deref()
                    .filter(|external_id| !external_id.trim().is_empty())
                    .map(str::to_owned),
            ),
            members,
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    fn assign_object(&mut self, op: Op, object: &Map<String, Value>) -> Result<(), Error> {
        for (key, value) in object {
            if key.eq_ignore_ascii_case(urn::GROUP) {
                if let Some(nested) = value.as_object() {
                    self.assign_object(op, nested)?;
                }
                continue;
            }
            self.assign(op, &Target::parse(key)?, value)?;
        }
        Ok(())
    }

    fn assign(&mut self, op: Op, target: &Target, value: &Value) -> Result<(), Error> {
        match target {
            Target::DisplayName => {
                let display_name = value
                    .as_str()
                    .filter(|display_name| !display_name.trim().is_empty())
                    .ok_or_else(|| {
                        Error::InvalidValue("displayName must be a non-blank string.".to_owned())
                    })?;
                self.display_name = Some(display_name.to_owned());
            }
            Target::ExternalId => {
                self.external_id = Some(match value {
                    Value::String(external_id) if !external_id.trim().is_empty() => {
                        Some(external_id.clone())
                    }
                    Value::String(_) | Value::Null => None,
                    _ => {
                        return Err(Error::InvalidValue(
                            "externalId must be a string.".to_owned(),
                        ));
                    }
                });
            }
            Target::Members if op == Op::Replace => {
                self.members.replace_with(MemberValue::list(value)?);
            }
            Target::Members => self.members.add_all(MemberValue::list(value)?),
            Target::Member(_) => {
                return Err(Error::InvalidPath(
                    "A members filter path can only be used to remove a member.".to_owned(),
                ));
            }
            Target::Active
            | Target::UserName
            | Target::Name
            | Target::GivenName
            | Target::FamilyName
            | Target::Formatted
            | Target::Email
            | Target::Ignored => {}
        }
        Ok(())
    }

    fn remove(&mut self, target: &Target, value: Option<&Value>) -> Result<(), Error> {
        match target {
            Target::DisplayName => {
                return Err(Error::Mutability(
                    "displayName is required, so it can't be removed.".to_owned(),
                ));
            }
            Target::ExternalId => self.external_id = Some(None),
            Target::Members => match value {
                Some(value) if !value.is_null() => {
                    self.members.remove_all(MemberValue::list(value)?);
                }
                _ => self.members.clear_all(),
            },
            Target::Member(member) => self.members.remove_all(vec![member.clone()]),
            Target::Active
            | Target::UserName
            | Target::Name
            | Target::GivenName
            | Target::FamilyName
            | Target::Formatted
            | Target::Email
            | Target::Ignored => {}
        }
        Ok(())
    }
}
