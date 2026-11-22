use std::ops::Deref;

use crate::registry::scope::Scope;
use crate::registry::scope_error::ScopeError;

const SCOPE_SEPARATOR: char = ' ';

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scopes(Vec<Scope>);

impl Scopes {
    pub fn parse<'a>(values: impl IntoIterator<Item = &'a str>) -> Result<Self, ScopeError> {
        values
            .into_iter()
            .flat_map(|value| value.split(SCOPE_SEPARATOR))
            .filter(|scope| !scope.is_empty())
            .map(Scope::parse)
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }
}

impl Deref for Scopes {
    type Target = [Scope];

    fn deref(&self) -> &[Scope] {
        &self.0
    }
}
