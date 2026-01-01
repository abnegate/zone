import birl
import gleam/bit_array
import gleam/crypto
import gleam/dynamic/decode
import gleam/int
import gleam/json
import gleam/list
import gleam/result
import gleam/string

/// JWT Claims structure
pub type JwtClaims {
  JwtClaims(
    sub: String,
    email: String,
    roles: List(String),
    permissions: List(String),
    iat: Int,
    exp: Int,
    jti: String,
  )
}

/// Create an access token (short-lived)
pub fn create_access_token(
  user_id: String,
  email: String,
  roles: List(String),
  permissions: List(String),
  secret: String,
  lifetime_seconds: Int,
) -> String {
  let now = birl.to_unix(birl.now())
  let exp = now + lifetime_seconds
  let jti = generate_token_id()

  let claims =
    JwtClaims(
      sub: user_id,
      email: email,
      roles: roles,
      permissions: permissions,
      iat: now,
      exp: exp,
      jti: jti,
    )

  encode_jwt(claims, secret)
}

/// Create a refresh token (returns token and expiry timestamp)
pub fn create_refresh_token(lifetime_seconds: Int) -> #(String, Int) {
  let now = birl.to_unix(birl.now())
  let exp = now + lifetime_seconds
  let token = generate_secure_token()
  #(token, exp)
}

/// Validate and decode a JWT
pub fn validate_token(
  token: String,
  secret: String,
) -> Result(JwtClaims, String) {
  case string.split(token, ".") {
    [header_b64, payload_b64, signature_b64] -> {
      // Verify signature
      let message = header_b64 <> "." <> payload_b64
      let expected_sig = sign_message(message, secret)

      case signature_b64 == expected_sig {
        True -> {
          // Decode and validate claims
          case decode_claims(payload_b64) {
            Ok(claims) -> {
              let now = birl.to_unix(birl.now())
              case claims.exp > now {
                True -> Ok(claims)
                False -> Error("Token expired")
              }
            }
            Error(err) -> Error(err)
          }
        }
        False -> Error("Invalid signature")
      }
    }
    _ -> Error("Invalid token format")
  }
}

/// Check if claims have a specific permission
pub fn has_permission(claims: JwtClaims, permission: String) -> Bool {
  list.contains(claims.permissions, permission)
}

/// Check if claims have any of the given permissions
pub fn has_any_permission(claims: JwtClaims, permissions: List(String)) -> Bool {
  list.any(permissions, fn(p) { list.contains(claims.permissions, p) })
}

/// Check if claims have a specific role
pub fn has_role(claims: JwtClaims, role: String) -> Bool {
  list.contains(claims.roles, role)
}

// --- Internal functions ---

/// Encode JWT with HS256 signature
fn encode_jwt(claims: JwtClaims, secret: String) -> String {
  let header =
    json.object([#("alg", json.string("HS256")), #("typ", json.string("JWT"))])
    |> json.to_string

  let payload =
    json.object([
      #("sub", json.string(claims.sub)),
      #("email", json.string(claims.email)),
      #("roles", json.array(claims.roles, json.string)),
      #("permissions", json.array(claims.permissions, json.string)),
      #("iat", json.int(claims.iat)),
      #("exp", json.int(claims.exp)),
      #("jti", json.string(claims.jti)),
    ])
    |> json.to_string

  let header_b64 = base64_url_encode(header)
  let payload_b64 = base64_url_encode(payload)

  let message = header_b64 <> "." <> payload_b64
  let signature = sign_message(message, secret)

  message <> "." <> signature
}

/// Decode claims from base64url-encoded payload
fn decode_claims(payload_b64: String) -> Result(JwtClaims, String) {
  case base64_url_decode(payload_b64) {
    Ok(payload_str) -> {
      let decoder = {
        use sub <- decode.field("sub", decode.string)
        use email <- decode.field("email", decode.string)
        use roles <- decode.field("roles", decode.list(decode.string))
        use permissions <- decode.field(
          "permissions",
          decode.list(decode.string),
        )
        use iat <- decode.field("iat", decode.int)
        use exp <- decode.field("exp", decode.int)
        use jti <- decode.field("jti", decode.string)
        decode.success(JwtClaims(
          sub: sub,
          email: email,
          roles: roles,
          permissions: permissions,
          iat: iat,
          exp: exp,
          jti: jti,
        ))
      }

      case json.parse(payload_str, decoder) {
        Ok(claims) -> Ok(claims)
        Error(_) -> Error("Invalid claims format")
      }
    }
    Error(_) -> Error("Invalid base64 encoding")
  }
}

/// Sign a message using HMAC-SHA256 and return base64url-encoded signature
fn sign_message(message: String, secret: String) -> String {
  let message_bytes = bit_array.from_string(message)
  let secret_bytes = bit_array.from_string(secret)

  crypto.hmac(message_bytes, crypto.Sha256, secret_bytes)
  |> base64_url_encode_bytes
}

/// Generate a unique token ID
fn generate_token_id() -> String {
  crypto.strong_random_bytes(16)
  |> bit_array.base16_encode
  |> string.lowercase
}

/// Generate a secure refresh token
pub fn generate_secure_token() -> String {
  crypto.strong_random_bytes(32)
  |> bit_array.base16_encode
  |> string.lowercase
}

/// Hash a token for storage
pub fn hash_token(token: String) -> String {
  token
  |> bit_array.from_string
  |> crypto.hash(crypto.Sha256, _)
  |> bit_array.base64_encode(True)
}

/// Base64 URL-safe encode a string
fn base64_url_encode(input: String) -> String {
  input
  |> bit_array.from_string
  |> base64_url_encode_bytes
}

/// Base64 URL-safe encode bytes
fn base64_url_encode_bytes(input: BitArray) -> String {
  input
  |> bit_array.base64_encode(False)
  |> string.replace("+", "-")
  |> string.replace("/", "_")
  |> string.replace("=", "")
}

/// Base64 URL-safe decode a string
fn base64_url_decode(input: String) -> Result(String, Nil) {
  // Add padding if needed
  let padded = case string.length(input) % 4 {
    2 -> input <> "=="
    3 -> input <> "="
    _ -> input
  }

  // Replace URL-safe chars
  let standard =
    padded
    |> string.replace("-", "+")
    |> string.replace("_", "/")

  case bit_array.base64_decode(standard) {
    Ok(bytes) -> {
      case bit_array.to_string(bytes) {
        Ok(str) -> Ok(str)
        Error(_) -> Error(Nil)
      }
    }
    Error(_) -> Error(Nil)
  }
}
