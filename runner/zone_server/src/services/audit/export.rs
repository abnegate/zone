use bytes::Bytes;
use futures::{Stream, stream};
use sqlx::PgPool;
use zone_enterprise::audit::{Format, SERVER, csv, ndjson};

use super::events::event;
use crate::db::audit::events::{self, Bookmark, Range, Row};
use crate::db::audit::scope::Scope;

pub const PAGE: usize = 1_000;

const DATE_FORMAT: &str = "%Y-%m-%d";

enum Cursor {
    Header,
    Page(Option<Bookmark>),
    Done,
}

/// The events of `scope` within `range`, oldest first, read one keyset page at a time on the
/// pool so no snapshot stays open while the client downloads.
pub fn stream(
    pool: PgPool,
    scope: Scope,
    format: Format,
    range: Range,
) -> impl Stream<Item = Result<Bytes, sqlx::Error>> + Send + 'static {
    let first = match format {
        Format::Csv => Cursor::Header,
        Format::Ndjson => Cursor::Page(None),
    };
    stream::try_unfold(first, move |cursor| {
        let pool = pool.clone();
        async move {
            match cursor {
                Cursor::Header => Ok(Some((
                    Bytes::from_static(csv::HEADER.as_bytes()),
                    Cursor::Page(None),
                ))),
                Cursor::Page(after) => page(&pool, scope, format, range, after).await,
                Cursor::Done => Ok(None),
            }
        }
    })
}

pub fn filename(scope: Scope, range: Range, format: Format) -> String {
    let owner = scope
        .organization_id()
        .map_or_else(|| SERVER.to_owned(), |id| id.to_string());
    format!(
        "audit-{owner}-{}-{}.{}",
        range.start.format(DATE_FORMAT),
        range.end.format(DATE_FORMAT),
        format.extension()
    )
}

async fn page(
    pool: &PgPool,
    scope: Scope,
    format: Format,
    range: Range,
    after: Option<Bookmark>,
) -> Result<Option<(Bytes, Cursor)>, sqlx::Error> {
    let rows = events::page(pool, scope, range, after, PAGE as i64).await?;
    let Some(last) = rows.last() else {
        return Ok(None);
    };
    let next = if rows.len() < PAGE {
        Cursor::Done
    } else {
        Cursor::Page(Some(last.bookmark()))
    };
    let chunk = encode(format, rows).map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
    Ok(Some((chunk, next)))
}

fn encode(format: Format, rows: Vec<Row>) -> serde_json::Result<Bytes> {
    let mut chunk = Vec::new();
    for row in rows {
        let event = event(row);
        match format {
            Format::Ndjson => chunk.extend(ndjson::line(&event)?),
            Format::Csv => chunk.extend_from_slice(csv::row(&event).as_bytes()),
        }
    }
    Ok(Bytes::from(chunk))
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use uuid::Uuid;
    use zone_enterprise::audit::Event;

    use super::*;

    fn range() -> Range {
        Range {
            start: Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 11, 15, 23, 59, 59).unwrap(),
        }
    }

    fn row(action: &str) -> Row {
        Row {
            transaction_id: 7,
            sequence: 3,
            id: Uuid::new_v4(),
            created_at: Utc.with_ymd_and_hms(2026, 11, 2, 9, 30, 0).unwrap(),
            organization_id: Some(Uuid::new_v4()),
            workspace_id: None,
            actor_id: Some(Uuid::new_v4()),
            actor_email: Some("admin@example.com".to_owned()),
            action: action.to_owned(),
            resource_type: "member".to_owned(),
            resource_id: None,
            old_values: None,
            new_values: Some(json!({ "role": "admin" })),
            ip_address: Some("10.0.0.5".to_owned()),
            user_agent: None,
        }
    }

    #[test]
    fn an_organization_export_is_named_after_the_organization_and_its_dates() {
        let organization = Uuid::new_v4();
        assert_eq!(
            filename(Scope::Organization(organization), range(), Format::Csv),
            format!("audit-{organization}-2026-11-01-2026-11-15.csv")
        );
    }

    #[test]
    fn a_server_export_is_named_after_the_server() {
        assert_eq!(
            filename(Scope::Server, range(), Format::Ndjson),
            "audit-server-2026-11-01-2026-11-15.ndjson"
        );
    }

    #[test]
    fn ndjson_writes_one_event_per_line_in_order() {
        let rows = vec![row("member.added"), row("member.removed")];
        let expected: Vec<Event> = rows.iter().cloned().map(event).collect();
        let chunk = encode(Format::Ndjson, rows).unwrap();
        let parsed: Vec<Event> = serde_json::Deserializer::from_slice(&chunk)
            .into_iter()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(parsed, expected);
        assert!(chunk.ends_with(b"\n"));
    }

    #[test]
    fn csv_writes_rows_without_repeating_the_header() {
        let rows = vec![row("member.added"), row("=cmd()")];
        let expected: String = rows
            .iter()
            .cloned()
            .map(event)
            .map(|event| csv::row(&event))
            .collect();
        let chunk = encode(Format::Csv, rows).unwrap();
        let text = std::str::from_utf8(&chunk).unwrap();
        assert_eq!(text, expected);
        assert!(!text.contains(csv::HEADER));
        assert!(text.contains("\"'=cmd()\""), "{text}");
    }
}
