use std::collections::HashSet;

use uuid::Uuid;

use crate::gate::unparsable::Unparsable;

pub const SEPARATOR: char = ',';

const EXPECTED: &str = "a license ID, which is a UUID";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Revoked(HashSet<Uuid>);

impl Revoked {
    pub fn parse(list: &str) -> Result<Self, Unparsable> {
        list.split(SEPARATOR)
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                Uuid::try_parse(entry).map_err(|_| Unparsable {
                    entry: entry.to_owned(),
                    expected: EXPECTED,
                })
            })
            .collect::<Result<HashSet<Uuid>, Unparsable>>()
            .map(Self)
    }

    pub fn contains(&self, license: Uuid) -> bool {
        self.0.contains(&license)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
