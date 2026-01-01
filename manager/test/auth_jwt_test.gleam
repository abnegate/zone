import auth/jwt.{type JwtClaims, JwtClaims}
import birl
import gleam/list
import gleam/string
import gleeunit/should

const test_secret = "super-secret-key-for-testing-only-32chars!"

// --- Token Creation Tests ---

pub fn create_access_token_returns_valid_jwt_format_test() {
  let token =
    jwt.create_access_token(
      "user-123",
      "test@example.com",
      ["user"],
      ["chats:read", "chats:create"],
      test_secret,
      900,
    )

  // JWT should have 3 parts separated by dots
  let parts = string.split(token, ".")
  parts
  |> list.length
  |> should.equal(3)
}

pub fn create_access_token_includes_all_claims_test() {
  let token =
    jwt.create_access_token(
      "user-456",
      "admin@example.com",
      ["admin", "user"],
      ["models:read", "models:delete"],
      test_secret,
      900,
    )

  // Should be able to validate and extract claims
  let result = jwt.validate_token(token, test_secret)

  case result {
    Ok(claims) -> {
      claims.sub |> should.equal("user-456")
      claims.email |> should.equal("admin@example.com")
      claims.roles |> should.equal(["admin", "user"])
      claims.permissions |> should.equal(["models:read", "models:delete"])
    }
    Error(_) -> should.fail()
  }
}

pub fn create_access_token_sets_expiry_correctly_test() {
  let lifetime = 900
  // 15 minutes
  let before = birl.to_unix(birl.now())

  let token =
    jwt.create_access_token(
      "user-1",
      "test@test.com",
      [],
      [],
      test_secret,
      lifetime,
    )

  let after = birl.to_unix(birl.now())

  case jwt.validate_token(token, test_secret) {
    Ok(claims) -> {
      // This is a simplification - just check exp > iat
      { claims.exp > claims.iat }
      |> should.be_true
      { claims.exp - claims.iat }
      |> should.equal(lifetime)
    }
    Error(_) -> should.fail()
  }
}

pub fn create_access_token_generates_unique_jti_test() {
  let token1 =
    jwt.create_access_token("u1", "a@b.com", [], [], test_secret, 900)
  let token2 =
    jwt.create_access_token("u1", "a@b.com", [], [], test_secret, 900)

  case
    jwt.validate_token(token1, test_secret),
    jwt.validate_token(token2, test_secret)
  {
    Ok(claims1), Ok(claims2) -> {
      claims1.jti
      |> should.not_equal(claims2.jti)
    }
    _, _ -> should.fail()
  }
}

// --- Token Validation Tests ---

pub fn validate_token_valid_token_succeeds_test() {
  let token =
    jwt.create_access_token(
      "test-user",
      "user@test.com",
      ["user"],
      ["projects:read"],
      test_secret,
      900,
    )

  case jwt.validate_token(token, test_secret) {
    Ok(_) -> should.be_true(True)
    Error(_) -> should.fail()
  }
}

pub fn validate_token_wrong_secret_fails_test() {
  let token =
    jwt.create_access_token("user", "e@e.com", [], [], test_secret, 900)

  case jwt.validate_token(token, "wrong-secret") {
    Ok(_) -> should.fail()
    Error(err) -> {
      err
      |> string.contains("signature")
      |> should.be_true
    }
  }
}

pub fn validate_token_expired_token_fails_test() {
  // Create token that expired in the past (negative lifetime won't work,
  // but we can test the validation logic)
  let token =
    jwt.create_access_token(
      "user",
      "e@e.com",
      [],
      [],
      test_secret,
      -100,
      // Already expired
    )

  case jwt.validate_token(token, test_secret) {
    Ok(_) -> should.fail()
    Error(err) -> {
      err
      |> string.contains("expired")
      |> should.be_true
    }
  }
}

pub fn validate_token_malformed_token_fails_test() {
  case jwt.validate_token("not.a.valid.token.format", test_secret) {
    Ok(_) -> should.fail()
    Error(_) -> should.be_true(True)
  }

  case jwt.validate_token("", test_secret) {
    Ok(_) -> should.fail()
    Error(_) -> should.be_true(True)
  }

  case jwt.validate_token("noperiods", test_secret) {
    Ok(_) -> should.fail()
    Error(_) -> should.be_true(True)
  }
}

pub fn validate_token_tampered_payload_fails_test() {
  let token =
    jwt.create_access_token("user", "e@e.com", [], [], test_secret, 900)

  // Tamper with the payload (middle part)
  let parts = string.split(token, ".")
  case parts {
    [header, _payload, sig] -> {
      let tampered = header <> ".dGFtcGVyZWQ." <> sig
      case jwt.validate_token(tampered, test_secret) {
        Ok(_) -> should.fail()
        Error(_) -> should.be_true(True)
      }
    }
    _ -> should.fail()
  }
}

pub fn validate_token_tampered_signature_fails_test() {
  let token =
    jwt.create_access_token("user", "e@e.com", [], [], test_secret, 900)

  // Tamper with signature (last part)
  let parts = string.split(token, ".")
  case parts {
    [header, payload, _sig] -> {
      let tampered = header <> "." <> payload <> ".invalidsig"
      case jwt.validate_token(tampered, test_secret) {
        Ok(_) -> should.fail()
        Error(_) -> should.be_true(True)
      }
    }
    _ -> should.fail()
  }
}

// --- Permission Checking Tests ---

pub fn has_permission_returns_true_when_present_test() {
  let claims =
    JwtClaims(
      sub: "user-1",
      email: "test@test.com",
      roles: ["user"],
      permissions: ["chats:read", "chats:create", "projects:read"],
      iat: 0,
      exp: 999_999_999,
      jti: "test-jti",
    )

  jwt.has_permission(claims, "chats:read")
  |> should.be_true

  jwt.has_permission(claims, "chats:create")
  |> should.be_true

  jwt.has_permission(claims, "projects:read")
  |> should.be_true
}

pub fn has_permission_returns_false_when_missing_test() {
  let claims =
    JwtClaims(
      sub: "user-1",
      email: "test@test.com",
      roles: ["user"],
      permissions: ["chats:read"],
      iat: 0,
      exp: 999_999_999,
      jti: "test-jti",
    )

  jwt.has_permission(claims, "chats:delete")
  |> should.be_false

  jwt.has_permission(claims, "models:read")
  |> should.be_false

  jwt.has_permission(claims, "")
  |> should.be_false
}

pub fn has_permission_empty_permissions_returns_false_test() {
  let claims =
    JwtClaims(
      sub: "user-1",
      email: "test@test.com",
      roles: [],
      permissions: [],
      iat: 0,
      exp: 999_999_999,
      jti: "test-jti",
    )

  jwt.has_permission(claims, "anything")
  |> should.be_false
}

pub fn has_any_permission_returns_true_when_one_matches_test() {
  let claims =
    JwtClaims(
      sub: "user-1",
      email: "test@test.com",
      roles: [],
      permissions: ["chats:read", "projects:create"],
      iat: 0,
      exp: 999_999_999,
      jti: "test-jti",
    )

  jwt.has_any_permission(claims, ["chats:read", "models:delete"])
  |> should.be_true

  jwt.has_any_permission(claims, ["unknown", "projects:create"])
  |> should.be_true
}

pub fn has_any_permission_returns_false_when_none_match_test() {
  let claims =
    JwtClaims(
      sub: "user-1",
      email: "test@test.com",
      roles: [],
      permissions: ["chats:read"],
      iat: 0,
      exp: 999_999_999,
      jti: "test-jti",
    )

  jwt.has_any_permission(claims, ["models:read", "projects:delete"])
  |> should.be_false

  jwt.has_any_permission(claims, [])
  |> should.be_false
}

pub fn has_role_returns_true_when_present_test() {
  let claims =
    JwtClaims(
      sub: "user-1",
      email: "test@test.com",
      roles: ["admin", "user"],
      permissions: [],
      iat: 0,
      exp: 999_999_999,
      jti: "test-jti",
    )

  jwt.has_role(claims, "admin")
  |> should.be_true

  jwt.has_role(claims, "user")
  |> should.be_true
}

pub fn has_role_returns_false_when_missing_test() {
  let claims =
    JwtClaims(
      sub: "user-1",
      email: "test@test.com",
      roles: ["user"],
      permissions: [],
      iat: 0,
      exp: 999_999_999,
      jti: "test-jti",
    )

  jwt.has_role(claims, "admin")
  |> should.be_false

  jwt.has_role(claims, "superuser")
  |> should.be_false
}

// --- Refresh Token Tests ---

pub fn create_refresh_token_returns_token_and_expiry_test() {
  let lifetime = 604_800
  // 7 days
  let before = birl.to_unix(birl.now())

  let #(token, exp) = jwt.create_refresh_token(lifetime)

  let after = birl.to_unix(birl.now())

  // Token should not be empty
  token
  |> string.is_empty
  |> should.be_false

  // Token should be hex-encoded (64 chars for 32 bytes)
  token
  |> string.length
  |> should.equal(64)

  // Expiry should be within expected range
  { exp >= before + lifetime }
  |> should.be_true

  { exp <= after + lifetime }
  |> should.be_true
}

pub fn create_refresh_token_generates_unique_tokens_test() {
  let #(token1, _) = jwt.create_refresh_token(3600)
  let #(token2, _) = jwt.create_refresh_token(3600)

  token1
  |> should.not_equal(token2)
}

// --- Token Hashing Tests ---

pub fn hash_token_returns_consistent_hash_test() {
  let token = "my-refresh-token-123"
  let hash1 = jwt.hash_token(token)
  let hash2 = jwt.hash_token(token)

  hash1
  |> should.equal(hash2)
}

pub fn hash_token_different_tokens_produce_different_hashes_test() {
  let hash1 = jwt.hash_token("token-1")
  let hash2 = jwt.hash_token("token-2")

  hash1
  |> should.not_equal(hash2)
}

pub fn generate_secure_token_returns_64_char_hex_test() {
  let token = jwt.generate_secure_token()

  token
  |> string.length
  |> should.equal(64)

  // Should only contain hex characters
  token
  |> string.to_graphemes
  |> list.all(fn(c) { string.contains("0123456789abcdef", c) })
  |> should.be_true
}

pub fn generate_secure_token_generates_unique_tokens_test() {
  let token1 = jwt.generate_secure_token()
  let token2 = jwt.generate_secure_token()
  let token3 = jwt.generate_secure_token()

  token1 |> should.not_equal(token2)
  token2 |> should.not_equal(token3)
  token1 |> should.not_equal(token3)
}
