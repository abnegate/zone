import gleam/regexp

/// Validate UUID v4 format
/// Returns True if the string is a valid UUID, False otherwise
pub fn is_valid_uuid(id: String) -> Bool {
  let uuid_pattern =
    "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"

  case regexp.from_string(uuid_pattern) {
    Ok(re) -> regexp.check(re, id)
    Error(_) -> False
  }
}

/// Validate UUID and return error if invalid
pub fn validate_uuid(id: String, field_name: String) -> Result(String, String) {
  case is_valid_uuid(id) {
    True -> Ok(id)
    False -> Error(field_name <> " must be a valid UUID")
  }
}
