import gleeunit/should
import qcheck_gleeunit_utils/run
import test_db
import websocket/pull

pub fn main() {
  // Initialize the test database pool and schema before running tests
  // This ensures the pool is started in the main process which survives the entire test run
  test_db.setup()
  run.run_gleeunit()
}

// =============================================================================
// parse_query_params tests
// =============================================================================

pub fn parse_query_params_empty_test() {
  pull.parse_query_params("")
  |> should.equal([])
}

pub fn parse_query_params_single_param_test() {
  pull.parse_query_params("key=value")
  |> should.equal([#("key", "value")])
}

pub fn parse_query_params_multiple_params_test() {
  pull.parse_query_params("key1=value1&key2=value2")
  |> should.equal([#("key1", "value1"), #("key2", "value2")])
}

pub fn parse_query_params_url_encoded_test() {
  pull.parse_query_params("model=llama3.1%3A8b")
  |> should.equal([#("model", "llama3.1:8b")])
}

pub fn parse_query_params_empty_value_test() {
  pull.parse_query_params("key=")
  |> should.equal([#("key", "")])
}

pub fn parse_query_params_no_value_test() {
  pull.parse_query_params("key")
  |> should.equal([#("key", "")])
}

pub fn parse_query_params_spaces_encoded_test() {
  pull.parse_query_params("q=hello%20world")
  |> should.equal([#("q", "hello world")])
}

pub fn parse_query_params_plus_sign_test() {
  // Plus signs should be kept as-is (not converted to spaces in percent_decode)
  pull.parse_query_params("q=hello+world")
  |> should.equal([#("q", "hello+world")])
}

pub fn parse_query_params_multiple_empty_params_test() {
  pull.parse_query_params("&&key=value&&")
  |> should.equal([#("key", "value")])
}

// =============================================================================
// parse_ws_pull_request tests
// =============================================================================

pub fn parse_ws_pull_request_valid_test() {
  pull.parse_ws_pull_request("{\"model\": \"llama3.1:8b\"}")
  |> should.be_ok()
  |> should.equal("llama3.1:8b")
}

pub fn parse_ws_pull_request_trims_whitespace_test() {
  pull.parse_ws_pull_request("{\"model\": \"  llama3.1:8b  \"}")
  |> should.be_ok()
  |> should.equal("llama3.1:8b")
}

pub fn parse_ws_pull_request_empty_model_test() {
  pull.parse_ws_pull_request("{\"model\": \"\"}")
  |> should.be_error()
  |> should.equal("Model name cannot be empty")
}

pub fn parse_ws_pull_request_whitespace_only_model_test() {
  pull.parse_ws_pull_request("{\"model\": \"   \"}")
  |> should.be_error()
  |> should.equal("Model name cannot be empty")
}

pub fn parse_ws_pull_request_invalid_json_test() {
  pull.parse_ws_pull_request("not json")
  |> should.be_error()
  |> should.equal("Invalid request: expected {\"model\": \"model_name\"}")
}

pub fn parse_ws_pull_request_missing_model_field_test() {
  pull.parse_ws_pull_request("{\"name\": \"llama3\"}")
  |> should.be_error()
  |> should.equal("Invalid request: expected {\"model\": \"model_name\"}")
}

pub fn parse_ws_pull_request_wrong_type_test() {
  pull.parse_ws_pull_request("{\"model\": 123}")
  |> should.be_error()
  |> should.equal("Invalid request: expected {\"model\": \"model_name\"}")
}

pub fn parse_ws_pull_request_special_chars_test() {
  pull.parse_ws_pull_request("{\"model\": \"hf.co/user/model-name\"}")
  |> should.be_ok()
  |> should.equal("hf.co/user/model-name")
}
