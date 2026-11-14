use chrono::DateTime;
use chrono::Utc;

use super::SourceOptions;
use crate::services::github::connection::ConnectedSource;
use crate::services::github::snapshot::Snapshot;

pub(crate) struct Offered {
    pub(crate) generated_at: DateTime<Utc>,
    pub(crate) sources: Vec<SourceOptions>,
    pub(crate) warnings: Vec<String>,
}

impl Offered {
    pub(crate) fn new(now: DateTime<Utc>, warnings: Vec<String>) -> Self {
        Self {
            generated_at: now,
            sources: Vec::new(),
            warnings,
        }
    }

    pub(crate) fn read(&mut self, source: &ConnectedSource, snapshot: &Snapshot<SourceOptions>) {
        self.generated_at = self.generated_at.min(snapshot.generated_at);
        self.sources.push(SourceOptions::clone(&snapshot.value));
        if let Some(warning) = &snapshot.warning {
            self.failed(source, warning);
        }
    }

    pub(crate) fn failed(&mut self, source: &ConnectedSource, warning: &str) {
        self.warnings
            .push(format!("{}: {warning}", source.source.name));
    }
}
