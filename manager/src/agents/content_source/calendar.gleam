/// Calendar content source
/// Provides access to calendar events from iCal and other calendar sources
import agents/content_source/types.{
  type ContentSourceError, type SourceHandler, SourceHandler,
}
import gleam/dynamic
import gleam/http/request
import gleam/httpc
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/order
import gleam/result
import gleam/string
import models/content.{
  type ContentItem, type ListQuery, type ListResult, type SearchQuery,
  type WriteResult, CalendarMetadata, ContentItem, ListResult,
}
import models/source.{
  type ICalConfig, type Source, CalendarCategory, ICalConfig, ICalSourceConfig,
}

/// List events from a calendar source
pub fn list_content(
  source: Source,
  query: ListQuery,
) -> Result(ListResult, ContentSourceError) {
  case source.config {
    ICalSourceConfig(cfg) -> list_ical_events(source.id, cfg, query)
    _ -> Error(types.InvalidSource("Expected calendar source"))
  }
}

/// Get a specific event by UID
pub fn get_content(
  source: Source,
  item_id: String,
) -> Result(ContentItem, ContentSourceError) {
  case source.config {
    ICalSourceConfig(cfg) -> get_ical_event(source.id, cfg, item_id)
    _ -> Error(types.InvalidSource("Expected calendar source"))
  }
}

/// Search events by text
pub fn search_content(
  source: Source,
  query: SearchQuery,
) -> Result(List(ContentItem), ContentSourceError) {
  case source.config {
    ICalSourceConfig(cfg) -> search_ical_events(source.id, cfg, query)
    _ -> Error(types.InvalidSource("Expected calendar source"))
  }
}

/// Write/create events
/// Note: iCal URLs are typically read-only
pub fn write_content(
  _source: Source,
  _item: ContentItem,
) -> Result(WriteResult, ContentSourceError) {
  Error(types.UnsupportedOperation(
    "iCal sources are read-only. Use CalDAV for read-write calendar access.",
  ))
}

/// Fetch and parse iCal events
fn list_ical_events(
  source_id: String,
  cfg: ICalConfig,
  query: ListQuery,
) -> Result(ListResult, ContentSourceError) {
  case fetch_ical(cfg.url) {
    Ok(ical_content) -> {
      let events = parse_ical_events(source_id, ical_content)

      // Filter by date range if provided
      let filtered =
        filter_events_by_date(events, query.start_date, query.end_date)

      // Apply pagination
      let paginated =
        filtered
        |> list.drop(query.offset)
        |> list.take(query.limit)

      let total = list.length(filtered)
      let has_more = query.offset + query.limit < total

      Ok(ListResult(items: paginated, total: total, has_more: has_more))
    }
    Error(err) -> Error(err)
  }
}

/// Get a specific event by UID
fn get_ical_event(
  source_id: String,
  cfg: ICalConfig,
  uid: String,
) -> Result(ContentItem, ContentSourceError) {
  case fetch_ical(cfg.url) {
    Ok(ical_content) -> {
      let events = parse_ical_events(source_id, ical_content)

      case list.find(events, fn(e) { e.id == uid }) {
        Ok(event) -> Ok(event)
        Error(_) -> Error(types.NotFound("Event not found: " <> uid))
      }
    }
    Error(err) -> Error(err)
  }
}

/// Search events by text
fn search_ical_events(
  source_id: String,
  cfg: ICalConfig,
  query: SearchQuery,
) -> Result(List(ContentItem), ContentSourceError) {
  case fetch_ical(cfg.url) {
    Ok(ical_content) -> {
      let events = parse_ical_events(source_id, ical_content)
      let search_term = string.lowercase(query.query)

      let matching =
        events
        |> filter_events_by_date(query.start_date, query.end_date)
        |> list.filter(fn(e) {
          string.contains(string.lowercase(e.title), search_term)
          || string.contains(string.lowercase(e.content), search_term)
        })
        |> list.take(query.limit)

      Ok(matching)
    }
    Error(err) -> Error(err)
  }
}

/// Fetch iCal content from URL
fn fetch_ical(url: String) -> Result(String, ContentSourceError) {
  case request.to(url) {
    Ok(req) -> {
      let final_req =
        req
        |> request.set_header("User-Agent", "Zone/1.0")
        |> request.set_header("Accept", "text/calendar")

      case httpc.send(final_req) {
        Ok(resp) -> {
          case resp.status >= 200 && resp.status < 300 {
            True -> Ok(resp.body)
            False ->
              Error(types.NetworkError(
                "Failed to fetch iCal: HTTP " <> int_to_string(resp.status),
              ))
          }
        }
        Error(_) ->
          Error(types.NetworkError("Failed to fetch iCal URL: " <> url))
      }
    }
    Error(_) -> Error(types.InvalidSource("Invalid iCal URL: " <> url))
  }
}

/// Parse iCal content into ContentItems
fn parse_ical_events(source_id: String, content: String) -> List(ContentItem) {
  // Split into VEVENT blocks
  content
  |> string.split("BEGIN:VEVENT")
  |> list.drop(1)
  |> list.filter_map(fn(block) { parse_vevent(source_id, block) })
}

/// Parse a single VEVENT block
fn parse_vevent(source_id: String, block: String) -> Result(ContentItem, Nil) {
  // Extract the event content (before END:VEVENT)
  let event_content =
    block
    |> string.split("END:VEVENT")
    |> list.first()
    |> result.unwrap("")

  // Extract fields
  let uid = extract_ical_field(event_content, "UID")
  let summary = extract_ical_field(event_content, "SUMMARY")
  let description = extract_ical_field(event_content, "DESCRIPTION")
  let dtstart = extract_ical_field(event_content, "DTSTART")
  let dtend = extract_ical_field(event_content, "DTEND")
  let location = extract_ical_field(event_content, "LOCATION")
  let rrule = extract_ical_field(event_content, "RRULE")

  case uid, summary, dtstart {
    Some(id), Some(title), Some(start) -> {
      let end = option.unwrap(dtend, start)
      let all_day = string.length(start) == 8
      // YYYYMMDD format

      Ok(ContentItem(
        id: id,
        source_id: source_id,
        category: CalendarCategory,
        title: unescape_ical(title),
        content: option.unwrap(description, "") |> unescape_ical(),
        content_type: "text/calendar",
        timestamp: Some(parse_ical_datetime(start)),
        url: None,
        metadata: CalendarMetadata(
          start_time: parse_ical_datetime(start),
          end_time: parse_ical_datetime(end),
          location: option.map(location, unescape_ical),
          attendees: extract_attendees(event_content),
          recurrence: rrule,
          all_day: all_day,
        ),
      ))
    }
    _, _, _ -> Error(Nil)
  }
}

/// Extract a field value from iCal content
fn extract_ical_field(content: String, field: String) -> Option(String) {
  // Handle fields with parameters (e.g., DTSTART;VALUE=DATE:20240101)
  let patterns = [field <> ":", field <> ";"]

  patterns
  |> list.find_map(fn(pattern) {
    case string.contains(content, pattern) {
      True -> {
        content
        |> string.split("\n")
        |> list.find(fn(line) {
          string.starts_with(line, pattern)
          || string.contains(line, "\n" <> pattern)
        })
        |> result.map(fn(line) {
          line
          |> string.split(":")
          |> list.drop(1)
          |> string.join(":")
          |> string.trim()
          |> unfold_ical_line()
        })
      }
      False -> Error(Nil)
    }
  })
  |> option.from_result()
}

/// Extract attendees from iCal content
fn extract_attendees(content: String) -> List(String) {
  content
  |> string.split("\n")
  |> list.filter(fn(line) { string.starts_with(line, "ATTENDEE") })
  |> list.filter_map(fn(line) {
    // Extract mailto: value
    case string.contains(line, "mailto:") {
      True -> {
        line
        |> string.split("mailto:")
        |> list.drop(1)
        |> list.first()
        |> result.map(fn(email) {
          email
          |> string.split("\"")
          |> list.first()
          |> result.unwrap(email)
          |> string.trim()
        })
      }
      False -> Error(Nil)
    }
  })
}

/// Parse iCal datetime to ISO8601
fn parse_ical_datetime(dt: String) -> String {
  // Handle DATE format: YYYYMMDD
  case string.length(dt) {
    8 -> {
      let year = string.slice(dt, 0, 4)
      let month = string.slice(dt, 4, 2)
      let day = string.slice(dt, 6, 2)
      year <> "-" <> month <> "-" <> day <> "T00:00:00Z"
    }
    // Handle DATETIME format: YYYYMMDDTHHMMSS or YYYYMMDDTHHMMSSZ
    _ -> {
      let clean = string.replace(dt, "Z", "")
      case string.length(clean) >= 15 {
        True -> {
          let year = string.slice(clean, 0, 4)
          let month = string.slice(clean, 4, 2)
          let day = string.slice(clean, 6, 2)
          let hour = string.slice(clean, 9, 2)
          let minute = string.slice(clean, 11, 2)
          let second = string.slice(clean, 13, 2)
          year
          <> "-"
          <> month
          <> "-"
          <> day
          <> "T"
          <> hour
          <> ":"
          <> minute
          <> ":"
          <> second
          <> "Z"
        }
        False -> dt
      }
    }
  }
}

/// Unfold continuation lines in iCal
fn unfold_ical_line(line: String) -> String {
  line
  |> string.replace("\r\n ", "")
  |> string.replace("\n ", "")
  |> string.replace("\r", "")
}

/// Unescape iCal special characters
fn unescape_ical(text: String) -> String {
  text
  |> string.replace("\\n", "\n")
  |> string.replace("\\,", ",")
  |> string.replace("\\;", ";")
  |> string.replace("\\\\", "\\")
}

/// Filter events by date range
fn filter_events_by_date(
  events: List(ContentItem),
  start_date: Option(String),
  end_date: Option(String),
) -> List(ContentItem) {
  events
  |> list.filter(fn(event) {
    case event.metadata {
      CalendarMetadata(start_time, end_time, _, _, _, _) -> {
        let after_start = case start_date {
          Some(s) -> string.compare(start_time, s) != order.Lt
          None -> True
        }
        let before_end = case end_date {
          Some(e) -> string.compare(end_time, e) != order.Gt
          None -> True
        }
        after_start && before_end
      }
      _ -> True
    }
  })
}

@external(erlang, "erlang", "integer_to_list")
fn int_to_string(n: Int) -> String

/// Get the handler for calendar sources
pub fn handler() -> SourceHandler {
  SourceHandler(
    list_content: list_content,
    get_content: get_content,
    search_content: search_content,
    write_content: write_content,
  )
}
