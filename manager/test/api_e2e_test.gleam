//// End-to-end API tests for the manager backend
////
//// These tests verify the API endpoints work correctly.
//// To run full E2E tests, start the server first:
////   SECURITY_MANAGER_API_KEY=test-key gleam run
//// Then run the tests:
////   gleam test
////
//// Note: Tests that require external services (Ollama, LiteLLM)
//// are marked with _integration suffix and require those services running.

import gleam/dynamic/decode
import gleam/http
import gleam/http/request
import gleam/httpc
import gleam/json
import gleam/list
import gleam/string
import gleeunit/should

// Test configuration - these can be overridden via environment variables
const test_host = "http://localhost:8000"
const test_api_key = "test-key"

// =============================================================================
// Helper functions
// =============================================================================

fn make_api_request(
  method: http.Method,
  path: String,
  body: String,
  with_auth: Bool,
) -> Result(#(Int, String), String) {
  let url = test_host <> path

  case request.to(url) {
    Ok(req) -> {
      let req = request.set_method(req, method)
      let req = request.set_body(req, body)
      let req = case body != "" {
        True -> request.set_header(req, "content-type", "application/json")
        False -> req
      }
      let req = case with_auth {
        True -> request.set_header(req, "authorization", "Bearer " <> test_api_key)
        False -> req
      }

      case httpc.send(req) {
        Ok(response) -> Ok(#(response.status, response.body))
        Error(_) -> Error("HTTP request failed - is the server running?")
      }
    }
    Error(_) -> Error("Invalid URL")
  }
}

fn get_api(path: String, with_auth: Bool) -> Result(#(Int, String), String) {
  make_api_request(http.Get, path, "", with_auth)
}

// =============================================================================
// Server availability check
// =============================================================================

fn server_available() -> Bool {
  case get_api("/", False) {
    Ok(_) -> True
    Error(_) -> False
  }
}

// =============================================================================
// Authentication tests
// =============================================================================

pub fn api_requires_auth_models_test() {
  case server_available() {
    False -> Nil  // Skip if server not running
    True -> {
      case get_api("/api/models", False) {
        Ok(#(status, body)) -> {
          status |> should.equal(401)
          body |> string.contains("Unauthorized") |> should.be_true()
        }
        Error(_) -> Nil  // Server not configured correctly
      }
    }
  }
}

pub fn api_requires_auth_browse_test() {
  case server_available() {
    False -> Nil
    True -> {
      case get_api("/api/browse", False) {
        Ok(#(status, _)) -> {
          status |> should.equal(401)
        }
        Error(_) -> Nil
      }
    }
  }
}

pub fn api_accepts_bearer_token_test() {
  case server_available() {
    False -> Nil
    True -> {
      case get_api("/api/browse?source=ollama", True) {
        Ok(#(status, _)) -> {
          // Should be 200 if auth works (or 500 if external service issue)
          // 401 means auth failed
          { status != 401 } |> should.be_true()
        }
        Error(_) -> Nil
      }
    }
  }
}

pub fn api_accepts_x_api_key_header_test() {
  case server_available() {
    False -> Nil
    True -> {
      let url = test_host <> "/api/browse?source=ollama"

      case request.to(url) {
        Ok(req) -> {
          let req = request.set_header(req, "x-api-key", test_api_key)

          case httpc.send(req) {
            Ok(response) -> {
              // Should not be 401
              { response.status != 401 } |> should.be_true()
            }
            Error(_) -> Nil
          }
        }
        Error(_) -> Nil
      }
    }
  }
}

pub fn api_rejects_invalid_key_test() {
  case server_available() {
    False -> Nil
    True -> {
      let url = test_host <> "/api/browse"

      case request.to(url) {
        Ok(req) -> {
          let req = request.set_header(req, "authorization", "Bearer invalid-key")

          case httpc.send(req) {
            Ok(response) -> {
              response.status |> should.equal(401)
            }
            Error(_) -> Nil
          }
        }
        Error(_) -> Nil
      }
    }
  }
}

// =============================================================================
// Browse API tests (Ollama library - no external service needed)
// =============================================================================

pub fn browse_ollama_returns_models_test() {
  case server_available() {
    False -> Nil
    True -> {
      case get_api("/api/browse?source=ollama", True) {
        Ok(#(status, body)) -> {
          status |> should.equal(200)

          // Parse response
          let decoder = {
            use source <- decode.field("source", decode.string)
            use has_more <- decode.field("has_more", decode.bool)
            decode.success(#(source, has_more))
          }

          case json.parse(body, decoder) {
            Ok(#(source, _)) -> {
              source |> should.equal("ollama")
            }
            Error(_) -> {
              // Response should be valid JSON
              should.fail()
            }
          }
        }
        Error(_) -> Nil
      }
    }
  }
}

pub fn browse_ollama_with_search_test() {
  case server_available() {
    False -> Nil
    True -> {
      case get_api("/api/browse?source=ollama&q=llama", True) {
        Ok(#(status, body)) -> {
          status |> should.equal(200)

          // All results should contain "llama" in name or description
          body |> string.contains("llama") |> should.be_true()
        }
        Error(_) -> Nil
      }
    }
  }
}

pub fn browse_ollama_pagination_test() {
  case server_available() {
    False -> Nil
    True -> {
      case get_api("/api/browse?source=ollama&limit=5&offset=0", True) {
        Ok(#(status, body)) -> {
          status |> should.equal(200)

          // Parse to check has_more
          let decoder = {
            use has_more <- decode.field("has_more", decode.bool)
            use total <- decode.field("total", decode.int)
            decode.success(#(has_more, total))
          }

          case json.parse(body, decoder) {
            Ok(#(has_more, total)) -> {
              // With limit=5 and 200+ models, there should be more
              case total > 5 {
                True -> has_more |> should.be_true()
                False -> Nil
              }
            }
            Error(_) -> should.fail()
          }
        }
        Error(_) -> Nil
      }
    }
  }
}

pub fn browse_invalid_source_test() {
  case server_available() {
    False -> Nil
    True -> {
      case get_api("/api/browse?source=invalid", True) {
        Ok(#(status, body)) -> {
          status |> should.equal(400)
          body |> string.contains("Unknown source") |> should.be_true()
        }
        Error(_) -> Nil
      }
    }
  }
}

// =============================================================================
// Static file serving tests
// =============================================================================

pub fn serves_index_html_test() {
  case server_available() {
    False -> Nil
    True -> {
      case get_api("/", False) {
        Ok(#(status, body)) -> {
          status |> should.equal(200)
          // Index should contain HTML
          body |> string.contains("<!DOCTYPE html>") |> should.be_true()
        }
        Error(_) -> Nil
      }
    }
  }
}

pub fn serves_index_without_auth_test() {
  case server_available() {
    False -> Nil
    True -> {
      // Index page should be accessible without auth
      case get_api("/", False) {
        Ok(#(status, _)) -> {
          status |> should.equal(200)
        }
        Error(_) -> Nil
      }
    }
  }
}

// =============================================================================
// 404 handling tests
// =============================================================================

pub fn returns_404_for_unknown_path_test() {
  case server_available() {
    False -> Nil
    True -> {
      case get_api("/unknown/path", False) {
        Ok(#(status, _)) -> {
          status |> should.equal(404)
        }
        Error(_) -> Nil
      }
    }
  }
}

pub fn returns_404_for_unknown_api_test() {
  case server_available() {
    False -> Nil
    True -> {
      case get_api("/api/unknown", True) {
        Ok(#(status, _)) -> {
          status |> should.equal(404)
        }
        Error(_) -> Nil
      }
    }
  }
}

// =============================================================================
// Method validation tests
// =============================================================================

pub fn browse_only_accepts_get_test() {
  case server_available() {
    False -> Nil
    True -> {
      case make_api_request(http.Post, "/api/browse", "{}", True) {
        Ok(#(status, _)) -> {
          status |> should.equal(405)
        }
        Error(_) -> Nil
      }
    }
  }
}

pub fn models_only_accepts_get_test() {
  case server_available() {
    False -> Nil
    True -> {
      case make_api_request(http.Post, "/api/models", "{}", True) {
        Ok(#(status, _)) -> {
          status |> should.equal(405)
        }
        Error(_) -> Nil
      }
    }
  }
}

pub fn model_delete_only_accepts_delete_test() {
  case server_available() {
    False -> Nil
    True -> {
      case get_api("/api/models/test-model", True) {
        Ok(#(status, _)) -> {
          status |> should.equal(405)
        }
        Error(_) -> Nil
      }
    }
  }
}

// =============================================================================
// Response format tests
// =============================================================================

pub fn browse_returns_json_test() {
  case server_available() {
    False -> Nil
    True -> {
      let url = test_host <> "/api/browse?source=ollama"

      case request.to(url) {
        Ok(req) -> {
          let req = request.set_header(req, "authorization", "Bearer " <> test_api_key)

          case httpc.send(req) {
            Ok(response) -> {
              // Check content-type header
              let content_type = list.find(response.headers, fn(h) {
                string.lowercase(h.0) == "content-type"
              })

              case content_type {
                Ok(#(_, value)) -> {
                  value |> string.contains("application/json") |> should.be_true()
                }
                Error(_) -> should.fail()
              }
            }
            Error(_) -> Nil
          }
        }
        Error(_) -> Nil
      }
    }
  }
}

pub fn error_response_is_json_test() {
  case server_available() {
    False -> Nil
    True -> {
      case get_api("/api/browse?source=invalid", True) {
        Ok(#(_, body)) -> {
          // Error should be valid JSON
          let decoder = {
            use success <- decode.field("success", decode.bool)
            use error <- decode.field("error", decode.string)
            decode.success(#(success, error))
          }

          case json.parse(body, decoder) {
            Ok(#(success, _)) -> {
              success |> should.be_false()
            }
            Error(_) -> should.fail()
          }
        }
        Error(_) -> Nil
      }
    }
  }
}

// =============================================================================
// Browse response structure tests
// =============================================================================

pub fn browse_ollama_model_structure_test() {
  case server_available() {
    False -> Nil
    True -> {
      case get_api("/api/browse?source=ollama&limit=1", True) {
        Ok(#(status, body)) -> {
          status |> should.equal(200)

          // Parse and verify model structure
          let model_decoder = {
            use id <- decode.field("id", decode.string)
            use name <- decode.field("name", decode.string)
            use description <- decode.field("description", decode.string)
            use downloads <- decode.field("downloads", decode.int)
            use tags <- decode.field("tags", decode.list(decode.string))
            decode.success(#(id, name, description, downloads, tags))
          }

          let response_decoder = {
            use models <- decode.field("models", decode.list(model_decoder))
            decode.success(models)
          }

          case json.parse(body, response_decoder) {
            Ok(models) -> {
              case list.first(models) {
                Ok(#(id, name, desc, downloads, tags)) -> {
                  // Verify all fields are populated
                  string.length(id) |> fn(l) { l > 0 } |> should.be_true()
                  string.length(name) |> fn(l) { l > 0 } |> should.be_true()
                  string.length(desc) |> fn(l) { l > 0 } |> should.be_true()
                  downloads |> fn(d) { d > 0 } |> should.be_true()
                  list.length(tags) |> fn(l) { l > 0 } |> should.be_true()
                }
                Error(_) -> Nil  // No models returned
              }
            }
            Error(_) -> should.fail()
          }
        }
        Error(_) -> Nil
      }
    }
  }
}
