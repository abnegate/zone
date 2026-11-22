use crate::registry::action::Action;
use crate::registry::name;
use crate::registry::resource_type::ResourceType;
use crate::registry::scope_error::ScopeError;

const FIELD_SEPARATOR: char = ':';
const ACTION_SEPARATOR: char = ',';

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub kind: ResourceType,
    pub name: String,
    pub actions: Vec<Action>,
}

impl Scope {
    pub fn parse(text: &str) -> Result<Self, ScopeError> {
        let malformed = || ScopeError::Malformed {
            scope: text.to_owned(),
        };
        let (kind, rest) = text.split_once(FIELD_SEPARATOR).ok_or_else(malformed)?;
        let (name, actions) = rest.rsplit_once(FIELD_SEPARATOR).ok_or_else(malformed)?;
        let kind = ResourceType::parse(kind).ok_or_else(malformed)?;
        if !name::resource(name) {
            return Err(malformed());
        }
        let mut parsed: Vec<Action> = Vec::new();
        for action in actions.split(ACTION_SEPARATOR) {
            let action = Action::parse(action).ok_or_else(malformed)?;
            if !parsed.contains(&action) {
                parsed.push(action);
            }
        }
        Ok(Self {
            kind,
            name: name.to_owned(),
            actions: parsed,
        })
    }
}
