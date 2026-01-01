/// File Source Types
/// Shared types and utilities for file source providers
import gleam/bit_array
import gleam/string

/// Error type for file operations
pub type FileSourceError {
  NotFound(path: String)
  AccessDenied(message: String)
  NetworkError(message: String)
  ParseError(message: String)
  WriteError(message: String)
  InvalidSource(message: String)
  UnsupportedOperation(message: String)
}

/// Convert error to string
pub fn error_to_string(err: FileSourceError) -> String {
  case err {
    NotFound(path) -> "File not found: " <> path
    AccessDenied(msg) -> "Access denied: " <> msg
    NetworkError(msg) -> "Network error: " <> msg
    ParseError(msg) -> "Parse error: " <> msg
    WriteError(msg) -> "Write error: " <> msg
    InvalidSource(msg) -> "Invalid source: " <> msg
    UnsupportedOperation(msg) -> "Unsupported: " <> msg
  }
}

// =============================================================================
// Utility Functions
// =============================================================================

/// Simple URI encoding
pub fn uri_encode(s: String) -> String {
  s
  |> string.replace(" ", "%20")
  |> string.replace("/", "%2F")
  |> string.replace(":", "%3A")
  |> string.replace("@", "%40")
  |> string.replace("#", "%23")
}

/// Base64 encode (using Erlang)
@external(erlang, "base64", "encode")
fn base64_encode_raw(data: BitArray) -> BitArray

pub fn base64_encode(s: String) -> String {
  let bits = <<s:utf8>>
  let encoded = base64_encode_raw(bits)
  case bit_array.to_string(encoded) {
    Ok(str) -> str
    Error(_) -> ""
  }
}

/// Base64 decode (using Erlang)
@external(erlang, "base64", "decode")
fn base64_decode_raw(data: BitArray) -> BitArray

pub fn base64_decode(s: String) -> String {
  let bits = <<s:utf8>>
  let decoded = base64_decode_raw(bits)
  case bit_array.to_string(decoded) {
    Ok(str) -> str
    Error(_) -> s
  }
}
