/// Prometheus metrics middleware for HTTP request tracking
///
/// Provides automatic metrics collection for all HTTP requests:
/// - http_requests_total: Counter of total requests by method, path, status
/// - http_request_duration_seconds: Histogram of request latency
import birl
import birl/duration
import gleam/dict.{type Dict}
import gleam/http
import gleam/int
import gleam/list
import gleam/result
import gleam/set
import gleam/string
import themis
import themis/counter
import themis/gauge
import themis/histogram
import themis/number

// Re-export for type compatibility
pub type MetricsError {
  GaugeError(gauge.GaugeError)
  HistogramError(histogram.HistogramError)
  CounterError(counter.CounterError)
}

import wisp.{type Request, type Response}

// =============================================================================
// Metric Names
// =============================================================================

const http_requests_total = "http_requests_total"

const http_request_duration_seconds = "http_request_duration_seconds"

const http_requests_in_flight = "http_requests_in_flight"

// =============================================================================
// Initialization
// =============================================================================

/// Initialize all HTTP metrics
/// Call this once at application startup
pub fn init() -> Result(Nil, MetricsError) {
  // Start themis metric store
  themis.init()

  // Total requests counter
  use _ <- result.try(
    counter.new(http_requests_total, "Total number of HTTP requests")
    |> result.map_error(CounterError),
  )

  // Request duration histogram with standard buckets
  let duration_buckets =
    [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
    |> list.map(number.decimal)
    |> list.fold(set.new(), fn(s, n) { set.insert(s, n) })

  use _ <- result.try(
    histogram.new(
      http_request_duration_seconds,
      "HTTP request duration in seconds",
      duration_buckets,
    )
    |> result.map_error(HistogramError),
  )

  // Requests in flight gauge
  gauge.new(http_requests_in_flight, "Number of HTTP requests in flight")
  |> result.map_error(GaugeError)
}

// =============================================================================
// Middleware
// =============================================================================

/// Middleware to record HTTP request metrics
/// Tracks request count, duration, and in-flight requests
pub fn record_request(
  req: Request,
  handler: fn() -> Response,
) -> Response {
  let start_time = birl.now()
  let method = http_method_to_string(req.method)
  let path = normalize_path(wisp.path_segments(req))

  let in_flight_labels = make_labels([#("method", method)])

  // Get current in-flight count and increment
  // Note: gauge.observe sets the value, so we track in-flight differently
  // For simplicity, just observe 1 for now (proper tracking would need atomic counters)
  let _ = gauge.observe(http_requests_in_flight, in_flight_labels, number.integer(1))

  // Execute handler
  let response = handler()

  // Set in-flight back to 0 after request completes
  let _ = gauge.observe(http_requests_in_flight, in_flight_labels, number.integer(0))

  // Calculate duration
  let end_time = birl.now()
  let duration_us = birl.difference(end_time, start_time) |> duration.blur_to(duration.MicroSecond)
  let duration_seconds = int.to_float(duration_us) /. 1_000_000.0

  // Get status code
  let status = int.to_string(response.status)

  // Record counter
  let counter_labels = make_labels([
    #("method", method),
    #("path", path),
    #("status", status),
  ])
  let _ = counter.increment(http_requests_total, counter_labels)

  // Record histogram
  let histogram_labels = make_labels([
    #("method", method),
    #("path", path),
  ])
  let _ = histogram.observe(
    http_request_duration_seconds,
    histogram_labels,
    number.decimal(duration_seconds),
  )

  response
}

// =============================================================================
// Export
// =============================================================================

/// Export all metrics in Prometheus text format
pub fn export() -> String {
  case themis.print() {
    Ok(output) -> output
    Error(_) -> "# Error exporting metrics\n"
  }
}

// =============================================================================
// Helpers
// =============================================================================

fn make_labels(pairs: List(#(String, String))) -> Dict(String, String) {
  dict.from_list(pairs)
}

fn http_method_to_string(method: http.Method) -> String {
  case method {
    http.Get -> "GET"
    http.Post -> "POST"
    http.Put -> "PUT"
    http.Patch -> "PATCH"
    http.Delete -> "DELETE"
    http.Head -> "HEAD"
    http.Options -> "OPTIONS"
    http.Trace -> "TRACE"
    http.Connect -> "CONNECT"
    http.Other(m) -> string.uppercase(m)
  }
}

/// Normalize path to reduce cardinality
/// Replaces UUIDs and numeric IDs with placeholders
fn normalize_path(segments: List(String)) -> String {
  let normalized =
    segments
    |> list.map(fn(segment) {
      case is_uuid(segment), is_numeric(segment) {
        True, _ -> ":id"
        _, True -> ":id"
        _, _ -> segment
      }
    })

  "/" <> string.join(normalized, "/")
}

fn is_uuid(s: String) -> Bool {
  // Simple UUID check: 36 chars with dashes in right places
  case string.length(s) {
    36 ->
      case string.split(s, "-") {
        [a, b, c, d, e] ->
          string.length(a) == 8
          && string.length(b) == 4
          && string.length(c) == 4
          && string.length(d) == 4
          && string.length(e) == 12
        _ -> False
      }
    _ -> False
  }
}

fn is_numeric(s: String) -> Bool {
  case int.parse(s) {
    Ok(_) -> True
    Error(_) -> False
  }
}
