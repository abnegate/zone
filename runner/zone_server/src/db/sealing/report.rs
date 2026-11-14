use std::collections::BTreeMap;
use std::fmt;

use super::backup::Backup;
use super::class::Class;
use super::column::Column;
use super::outcome::Outcome;
use super::policy::Policy;

const COLUMN_HEADING: &str = "column";
const POLICY_HEADING: &str = "policy";
const SEPARATOR: &str = "  ";

/// What a sealing run found and did, per column, class and outcome.
#[derive(Clone, Debug)]
pub struct Report {
    backup: Backup,
    dry_run: bool,
    classes: BTreeMap<(Column, Class), u64>,
    outcomes: BTreeMap<(Column, Outcome), u64>,
}

impl Report {
    /// An empty report of a run behind `backup`.
    pub fn new(backup: Backup, dry_run: bool) -> Self {
        Self {
            backup,
            dry_run,
            classes: BTreeMap::new(),
            outcomes: BTreeMap::new(),
        }
    }

    pub(super) fn record(&mut self, column: Column, class: Class, outcome: Outcome) {
        *self.classes.entry((column, class)).or_default() += 1;
        *self.outcomes.entry((column, outcome)).or_default() += 1;
    }

    /// Whether the run only counted.
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }

    /// How many values of `class` the run found in `column`.
    pub fn count(&self, column: Column, class: Class) -> u64 {
        self.classes.get(&(column, class)).copied().unwrap_or(0)
    }

    /// How many values in `column` ended in `outcome`. In a dry run, the writes it would make.
    pub fn outcome(&self, column: Column, outcome: Outcome) -> u64 {
        self.outcomes.get(&(column, outcome)).copied().unwrap_or(0)
    }

    /// How many rows the run wrote.
    pub fn written(&self) -> u64 {
        if self.dry_run {
            return 0;
        }
        self.total(Outcome::writes)
    }

    /// Whether every envelope the run wrote reopened as its credential.
    pub fn is_clean(&self) -> bool {
        self.total(|outcome| outcome == Outcome::Mismatched) == 0
    }

    /// How many values look sealed and did not open, when the run's key is shown to be the
    /// wrong one: some look sealed and none opens, across every column or across the columns
    /// sealing writes to. The second keeps a key that opens only counted columns from sealing
    /// the columns it writes.
    pub fn wrong_key(&self) -> Option<u64> {
        [
            self.evidence(|_| true),
            self.evidence(|column| column.policy().writes()),
        ]
        .into_iter()
        .find(|(opened, unopened)| *opened == 0 && *unopened > 0)
        .map(|(_, unopened)| unopened)
    }

    /// Whether no value looks sealed, so nothing shows the run's key is the one the server
    /// seals with.
    pub fn key_untested(&self) -> bool {
        self.evidence(|_| true) == (0, 0)
    }

    /// How many values in the columns `scope` keeps opened under the run's key, and how many
    /// look sealed and did not open: unopenable envelopes anywhere, and ambiguous values in
    /// every column sealing writes to.
    fn evidence(&self, scope: impl Fn(Column) -> bool) -> (u64, u64) {
        self.classes
            .iter()
            .filter(|((column, _), _)| scope(*column))
            .fold(
                (0, 0),
                |(opened, unopened), ((column, class), count)| match class {
                    Class::Envelope | Class::Legacy => (opened + count, unopened),
                    Class::Unopenable => (opened, unopened + count),
                    Class::Ambiguous if column.policy().writes() => (opened, unopened + count),
                    _ => (opened, unopened),
                },
            )
    }

    fn total(&self, counted: impl Fn(Outcome) -> bool) -> u64 {
        self.outcomes
            .iter()
            .filter(|((_, outcome), _)| counted(*outcome))
            .map(|(_, count)| count)
            .sum()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            formatter,
            "Database {}, verified against the backup at {}",
            self.backup.database(),
            self.backup.path().display()
        )?;
        if self.dry_run {
            writeln!(
                formatter,
                "Dry run: nothing was written; sealed and resealed count what a real run would write"
            )?;
        }

        let labels: Vec<String> = Column::ALL.iter().map(Column::to_string).collect();
        let width = labels
            .iter()
            .map(String::len)
            .chain([COLUMN_HEADING.len()])
            .max()
            .unwrap_or_default();
        let policy_width = POLICY_HEADING.len().max(Policy::Counted.as_str().len());

        write!(
            formatter,
            "{COLUMN_HEADING:<width$}{SEPARATOR}{POLICY_HEADING:<policy_width$}"
        )?;
        for class in Class::ALL {
            write!(formatter, "{SEPARATOR}{class}")?;
        }
        for outcome in Outcome::ALL {
            write!(formatter, "{SEPARATOR}{outcome}")?;
        }
        writeln!(formatter)?;

        for (column, label) in Column::ALL.iter().zip(&labels) {
            write!(
                formatter,
                "{label:<width$}{SEPARATOR}{:<policy_width$}",
                column.policy().as_str()
            )?;
            for class in Class::ALL {
                let count = self.count(*column, *class);
                let class_width = class.as_str().len();
                write!(formatter, "{SEPARATOR}{count:>class_width$}")?;
            }
            for outcome in Outcome::ALL {
                let count = self.outcome(*column, *outcome);
                let outcome_width = outcome.as_str().len();
                write!(formatter, "{SEPARATOR}{count:>outcome_width$}")?;
            }
            writeln!(formatter)?;
        }

        write!(formatter, "Written: {}", self.written())
    }
}
