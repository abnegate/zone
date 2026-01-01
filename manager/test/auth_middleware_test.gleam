import auth/jwt.{type JwtClaims, JwtClaims}
import gleam/http
import gleam/http/request.{type Request}
import gleam/option.{None, Some}
import gleam/string
import gleeunit/should
import middleware/auth

const test_secret = "test-jwt-secret-for-middleware-tests!"

// Helper to create a test request with headers
fn create_request_with_header(
  header_name: String,
  header_value: String,
) -> Request(String) {
  request.new()
  |> request.set_method(http.Get)
  |> request.set_host("localhost")
  |> request.set_path("/api/test")
  |> request.set_header(header_name, header_value)
}

fn create_empty_request() -> Request(String) {
  request.new()
  |> request.set_method(http.Get)
  |> request.set_host("localhost")
  |> request.set_path("/api/test")
}

// --- JWT Token Creation for Tests ---

fn create_test_token(
  user_id: String,
  email: String,
  roles: List(String),
  permissions: List(String),
) -> String {
  jwt.create_access_token(user_id, email, roles, permissions, test_secret, 900)
}

// --- Basic Token Validation Tests ---

pub fn valid_bearer_token_can_be_validated_test() {
  let token =
    create_test_token("user-123", "test@example.com", ["user"], ["chats:read"])

  // Token should be a valid JWT format
  let parts = string.split(token, ".")
  should.equal(3, case parts {
    [_, _, _] -> 3
    _ -> 0
  })
}

pub fn token_can_be_decoded_test() {
  let token =
    create_test_token(
      "user-456",
      "admin@example.com",
      ["admin"],
      ["models:read"],
    )

  case jwt.validate_token(token, test_secret) {
    Ok(claims) -> {
      claims.sub
      |> should.equal("user-456")
      claims.email
      |> should.equal("admin@example.com")
    }
    Error(_) -> should.fail()
  }
}

pub fn invalid_token_fails_validation_test() {
  let result = jwt.validate_token("invalid-token", test_secret)
  case result {
    Error(_) -> should.be_ok(Ok(Nil))
    Ok(_) -> should.fail()
  }
}

pub fn wrong_secret_fails_validation_test() {
  let token =
    create_test_token("user-1", "test@test.com", ["user"], ["chats:read"])

  let result = jwt.validate_token(token, "wrong-secret")
  case result {
    Error(_) -> should.be_ok(Ok(Nil))
    Ok(_) -> should.fail()
  }
}

// --- Token Claims Tests ---

pub fn token_contains_user_id_test() {
  let token = create_test_token("my-user-id", "test@test.com", [], [])

  case jwt.validate_token(token, test_secret) {
    Ok(claims) -> claims.sub |> should.equal("my-user-id")
    Error(_) -> should.fail()
  }
}

pub fn token_contains_email_test() {
  let token = create_test_token("user-1", "specific@email.com", [], [])

  case jwt.validate_token(token, test_secret) {
    Ok(claims) -> claims.email |> should.equal("specific@email.com")
    Error(_) -> should.fail()
  }
}

pub fn token_contains_roles_test() {
  let token = create_test_token("user-1", "test@test.com", ["admin", "user"], [])

  case jwt.validate_token(token, test_secret) {
    Ok(claims) -> claims.roles |> should.equal(["admin", "user"])
    Error(_) -> should.fail()
  }
}

pub fn token_contains_permissions_test() {
  let token =
    create_test_token(
      "user-1",
      "test@test.com",
      [],
      ["chats:read", "chats:create"],
    )

  case jwt.validate_token(token, test_secret) {
    Ok(claims) ->
      claims.permissions |> should.equal(["chats:read", "chats:create"])
    Error(_) -> should.fail()
  }
}

pub fn token_has_expiry_greater_than_issued_at_test() {
  let token = create_test_token("user-1", "test@test.com", [], [])

  case jwt.validate_token(token, test_secret) {
    Ok(claims) -> { claims.exp > claims.iat } |> should.be_true()
    Error(_) -> should.fail()
  }
}

pub fn tokens_have_unique_jti_test() {
  let token1 = create_test_token("user-1", "test@test.com", [], [])
  let token2 = create_test_token("user-1", "test@test.com", [], [])

  case jwt.validate_token(token1, test_secret), jwt.validate_token(token2, test_secret) {
    Ok(claims1), Ok(claims2) -> claims1.jti |> should.not_equal(claims2.jti)
    _, _ -> should.fail()
  }
}
