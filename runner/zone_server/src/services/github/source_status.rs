use chrono::DateTime;
use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use super::connection::ConnectedSource;
use super::freshness::Freshness;
use super::snapshot::Snapshot;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceStatus {
    pub source_id: Uuid,
    pub name: String,
    pub repository: String,
    pub repository_url: String,
    pub freshness: Freshness,
    pub generated_at: Option<DateTime<Utc>>,
    pub warning: Option<String>,
}

impl SourceStatus {
    pub(crate) fn read<T>(
        source: &ConnectedSource,
        origin: &str,
        snapshot: &Snapshot<T>,
        freshness: Freshness,
    ) -> Self {
        Self {
            generated_at: Some(snapshot.generated_at),
            warning: snapshot.warning.clone(),
            ..Self::unread(source, origin, freshness, None)
        }
    }

    pub(crate) fn failed(source: &ConnectedSource, origin: &str, warning: String) -> Self {
        Self::unread(source, origin, Freshness::Stale, Some(warning))
    }

    fn unread(
        source: &ConnectedSource,
        origin: &str,
        freshness: Freshness,
        warning: Option<String>,
    ) -> Self {
        Self {
            source_id: source.source.id,
            name: source.source.name.clone(),
            repository: source.configuration.full_name(),
            repository_url: source.configuration.web_url(origin),
            freshness,
            generated_at: None,
            warning,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::services::github::testing::connected_source;

    const SOURCE: &str = "6f2c1f8e-0d55-4a3c-9a43-2f1d4c1e7b10";
    const ORIGIN: &str = "https://github.example.com/";
    const GENERATED_AT: &str = "2026-11-14T03:04:05Z";
    const WARNING: &str = "GitHub request failed or timed out.";

    fn source() -> ConnectedSource {
        connected_source(SOURCE.parse().unwrap(), "Zone")
    }

    #[test]
    fn a_read_source_reports_when_and_how_it_was_read() {
        let snapshot =
            Snapshot::new((), GENERATED_AT.parse().unwrap()).warned(Some(WARNING.into()));

        let status = SourceStatus::read(&source(), ORIGIN, &snapshot, Freshness::Stale);

        assert_eq!(
            serde_json::to_value(status).unwrap(),
            json!({
                "source_id": SOURCE,
                "name": "Zone",
                "repository": "owner/repository",
                "repository_url": "https://github.example.com/owner/repository",
                "freshness": "stale",
                "generated_at": GENERATED_AT,
                "warning": WARNING,
            })
        );
    }

    #[test]
    fn a_source_never_read_reports_stale_with_why_and_no_moment() {
        let status = SourceStatus::failed(&source(), ORIGIN, WARNING.into());

        let encoded = serde_json::to_value(status).unwrap();

        assert_eq!(encoded["freshness"], json!("stale"));
        assert_eq!(encoded["generated_at"], json!(null));
        assert_eq!(encoded["warning"], json!(WARNING));
    }

    #[test]
    fn every_freshness_is_spelled_as_the_console_reads_it() {
        for (freshness, spelled) in [
            (Freshness::Cached, "cached"),
            (Freshness::Refreshed, "refreshed"),
            (Freshness::Floored, "floored"),
            (Freshness::Stale, "stale"),
        ] {
            assert_eq!(serde_json::to_value(freshness).unwrap(), json!(spelled));
        }
    }
}
