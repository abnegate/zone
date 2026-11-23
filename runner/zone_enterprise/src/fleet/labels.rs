use std::collections::BTreeSet;
use std::fmt;

use crate::fleet::error::Error;
use crate::fleet::label::Label;

const SEPARATOR: char = ',';

/// A sorted set of runner labels. An empty set requires nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Labels(BTreeSet<Label>);

impl Labels {
    /// Labels written as a comma-separated list, as `ZONE_RUNNER_LABELS` holds them. Blank
    /// entries are skipped and repeats collapse.
    pub fn parse_list(text: &str) -> Result<Self, Error> {
        text.split(SEPARATOR)
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(Label::parse)
            .collect()
    }

    pub fn from_strings<I, S>(strings: I) -> Result<Self, Error>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        strings
            .into_iter()
            .map(|label| Label::parse(label.as_ref()))
            .collect()
    }

    pub fn union(&self, other: &Self) -> Self {
        Self(self.0.union(&other.0).cloned().collect())
    }

    /// The labels of this set that `carried` lacks.
    pub fn missing_from(&self, carried: &Self) -> Self {
        Self(self.0.difference(&carried.0).cloned().collect())
    }

    pub fn contains_all(&self, required: &Self) -> bool {
        required.0.is_subset(&self.0)
    }

    pub fn to_vec(&self) -> Vec<String> {
        self.0
            .iter()
            .map(|label| label.as_str().to_string())
            .collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Label> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromIterator<Label> for Labels {
    fn from_iter<I: IntoIterator<Item = Label>>(labels: I) -> Self {
        Self(labels.into_iter().collect())
    }
}

impl fmt::Display for Labels {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, label) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(", ")?;
            }
            formatter.write_str(label.as_str())?;
        }
        Ok(())
    }
}
