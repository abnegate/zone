use serde::{Deserialize, Serialize};

use crate::db::analytics::CountRow;

/// How many of something carry one name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    pub name: String,
    pub count: usize,
}

impl Tally {
    pub fn new(name: &str, count: usize) -> Self {
        Self {
            name: name.to_string(),
            count,
        }
    }

    /// One tally per name, in the order given, reading zero for a name no row
    /// counted and ignoring any row whose name is not asked for.
    pub fn fixed<'a>(names: impl IntoIterator<Item = &'a str>, rows: &[CountRow]) -> Vec<Self> {
        names
            .into_iter()
            .map(|name| {
                let count = rows
                    .iter()
                    .filter(|row| row.name == name)
                    .map(|row| usize::try_from(row.count).unwrap_or_default())
                    .sum();
                Self::new(name, count)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, count: i64) -> CountRow {
        CountRow {
            name: name.to_string(),
            count,
        }
    }

    #[test]
    fn every_name_is_listed_in_order_including_zeros() {
        let tallies = Tally::fixed(["a", "b", "c"], &[row("c", 3), row("a", 1)]);
        assert_eq!(
            tallies,
            vec![Tally::new("a", 1), Tally::new("b", 0), Tally::new("c", 3)]
        );
    }

    #[test]
    fn a_name_not_asked_for_is_ignored() {
        let tallies = Tally::fixed(["a"], &[row("a", 2), row("z", 9)]);
        assert_eq!(tallies, vec![Tally::new("a", 2)]);
    }

    #[test]
    fn a_negative_count_reads_as_zero() {
        let tallies = Tally::fixed(["a"], &[row("a", -4)]);
        assert_eq!(tallies, vec![Tally::new("a", 0)]);
    }
}
