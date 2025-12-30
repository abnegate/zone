import gleeunit
import gleeunit/should
import manager

pub fn main() {
  gleeunit.main()
}

// =============================================================================
// parse_query_params tests
// =============================================================================

pub fn parse_query_params_empty_test() {
  manager.parse_query_params("")
  |> should.equal([])
}

pub fn parse_query_params_single_param_test() {
  manager.parse_query_params("key=value")
  |> should.equal([#("key", "value")])
}

pub fn parse_query_params_multiple_params_test() {
  manager.parse_query_params("key1=value1&key2=value2")
  |> should.equal([#("key1", "value1"), #("key2", "value2")])
}

pub fn parse_query_params_url_encoded_test() {
  manager.parse_query_params("model=llama3.1%3A8b")
  |> should.equal([#("model", "llama3.1:8b")])
}

pub fn parse_query_params_empty_value_test() {
  manager.parse_query_params("key=")
  |> should.equal([#("key", "")])
}

pub fn parse_query_params_no_value_test() {
  manager.parse_query_params("key")
  |> should.equal([#("key", "")])
}

pub fn parse_query_params_spaces_encoded_test() {
  manager.parse_query_params("q=hello%20world")
  |> should.equal([#("q", "hello world")])
}

pub fn parse_query_params_plus_sign_test() {
  // Plus signs should be kept as-is (not converted to spaces in percent_decode)
  manager.parse_query_params("q=hello+world")
  |> should.equal([#("q", "hello+world")])
}

pub fn parse_query_params_multiple_empty_params_test() {
  manager.parse_query_params("&&key=value&&")
  |> should.equal([#("key", "value")])
}

// =============================================================================
// validate_path_segments tests
// =============================================================================

pub fn validate_path_segments_valid_single_test() {
  manager.validate_path_segments(["style.css"])
  |> should.be_true()
}

pub fn validate_path_segments_valid_nested_test() {
  manager.validate_path_segments(["js", "app.js"])
  |> should.be_true()
}

pub fn validate_path_segments_dotdot_attack_test() {
  manager.validate_path_segments([".."])
  |> should.be_false()
}

pub fn validate_path_segments_dotdot_in_path_test() {
  manager.validate_path_segments(["foo", "..", "etc", "passwd"])
  |> should.be_false()
}

pub fn validate_path_segments_dotdot_embedded_test() {
  manager.validate_path_segments(["foo..bar"])
  |> should.be_false()
}

pub fn validate_path_segments_single_dot_test() {
  manager.validate_path_segments(["."])
  |> should.be_false()
}

pub fn validate_path_segments_empty_segment_test() {
  manager.validate_path_segments(["foo", "", "bar"])
  |> should.be_false()
}

pub fn validate_path_segments_leading_slash_test() {
  manager.validate_path_segments(["/etc/passwd"])
  |> should.be_false()
}

pub fn validate_path_segments_empty_list_test() {
  manager.validate_path_segments([])
  |> should.be_true()
}

pub fn validate_path_segments_complex_valid_test() {
  manager.validate_path_segments(["assets", "css", "main.bundle.css"])
  |> should.be_true()
}

// =============================================================================
// get_content_type tests
// =============================================================================

pub fn get_content_type_css_test() {
  manager.get_content_type("style.css")
  |> should.equal("text/css; charset=utf-8")
}

pub fn get_content_type_js_test() {
  manager.get_content_type("app.js")
  |> should.equal("application/javascript; charset=utf-8")
}

pub fn get_content_type_json_test() {
  manager.get_content_type("data.json")
  |> should.equal("application/json")
}

pub fn get_content_type_svg_test() {
  manager.get_content_type("icon.svg")
  |> should.equal("image/svg+xml")
}

pub fn get_content_type_unknown_test() {
  manager.get_content_type("file.txt")
  |> should.equal("text/plain")
}

pub fn get_content_type_no_extension_test() {
  manager.get_content_type("README")
  |> should.equal("text/plain")
}

pub fn get_content_type_nested_path_test() {
  manager.get_content_type("assets/js/bundle.js")
  |> should.equal("application/javascript; charset=utf-8")
}

// =============================================================================
// parse_ws_pull_request tests
// =============================================================================

pub fn parse_ws_pull_request_valid_test() {
  manager.parse_ws_pull_request("{\"model\": \"llama3.1:8b\"}")
  |> should.be_ok()
  |> should.equal("llama3.1:8b")
}

pub fn parse_ws_pull_request_trims_whitespace_test() {
  manager.parse_ws_pull_request("{\"model\": \"  llama3.1:8b  \"}")
  |> should.be_ok()
  |> should.equal("llama3.1:8b")
}

pub fn parse_ws_pull_request_empty_model_test() {
  manager.parse_ws_pull_request("{\"model\": \"\"}")
  |> should.be_error()
  |> should.equal("Model name cannot be empty")
}

pub fn parse_ws_pull_request_whitespace_only_model_test() {
  manager.parse_ws_pull_request("{\"model\": \"   \"}")
  |> should.be_error()
  |> should.equal("Model name cannot be empty")
}

pub fn parse_ws_pull_request_invalid_json_test() {
  manager.parse_ws_pull_request("not json")
  |> should.be_error()
  |> should.equal("Invalid request: expected {\"model\": \"model_name\"}")
}

pub fn parse_ws_pull_request_missing_model_field_test() {
  manager.parse_ws_pull_request("{\"name\": \"llama3\"}")
  |> should.be_error()
  |> should.equal("Invalid request: expected {\"model\": \"model_name\"}")
}

pub fn parse_ws_pull_request_wrong_type_test() {
  manager.parse_ws_pull_request("{\"model\": 123}")
  |> should.be_error()
  |> should.equal("Invalid request: expected {\"model\": \"model_name\"}")
}

pub fn parse_ws_pull_request_special_chars_test() {
  manager.parse_ws_pull_request("{\"model\": \"hf.co/user/model-name\"}")
  |> should.be_ok()
  |> should.equal("hf.co/user/model-name")
}
