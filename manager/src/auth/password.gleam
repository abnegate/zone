import gleam/bit_array
import gleam/crypto
import gleam/int
import gleam/list
import gleam/result
import gleam/string

/// Hash a password using PBKDF2-SHA256
/// Format: $pbkdf2-sha256$iterations$salt$hash
pub fn hash_password(password: String) -> String {
  let iterations = 100_000
  let salt = crypto.strong_random_bytes(16)
  let salt_b64 = bit_array.base64_encode(salt, True)

  let hash = pbkdf2_sha256(password, salt, iterations, 32)
  let hash_b64 = bit_array.base64_encode(hash, True)

  "$pbkdf2-sha256$"
  <> int.to_string(iterations)
  <> "$"
  <> salt_b64
  <> "$"
  <> hash_b64
}

/// Verify a password against a hash
pub fn verify_password(password: String, hash_string: String) -> Bool {
  case string.split(hash_string, "$") {
    ["", "pbkdf2-sha256", iterations_str, salt_b64, hash_b64] -> {
      case int.parse(iterations_str), bit_array.base64_decode(salt_b64) {
        Ok(iterations), Ok(salt) -> {
          let computed = pbkdf2_sha256(password, salt, iterations, 32)
          let computed_b64 = bit_array.base64_encode(computed, True)
          // Use constant-time comparison to prevent timing attacks
          secure_compare(computed_b64, hash_b64)
        }
        _, _ -> False
      }
    }
    _ -> False
  }
}

/// PBKDF2-SHA256 key derivation
/// This is a simplified implementation using HMAC-SHA256
fn pbkdf2_sha256(
  password: String,
  salt: BitArray,
  iterations: Int,
  key_length: Int,
) -> BitArray {
  let password_bytes = bit_array.from_string(password)

  // PBKDF2 block 1 (for 32 bytes, we only need 1 block)
  let block_num = <<1:32>>
  let initial = hmac_sha256(password_bytes, bit_array.concat([salt, block_num]))

  // Iterate
  let result = iterate_pbkdf2(password_bytes, initial, initial, iterations - 1)

  // Truncate to key_length
  case bit_array.slice(result, 0, key_length) {
    Ok(truncated) -> truncated
    Error(_) -> result
  }
}

fn iterate_pbkdf2(
  password: BitArray,
  acc: BitArray,
  prev: BitArray,
  remaining: Int,
) -> BitArray {
  case remaining {
    0 -> acc
    _ -> {
      let next = hmac_sha256(password, prev)
      let new_acc = xor_bytes(acc, next)
      iterate_pbkdf2(password, new_acc, next, remaining - 1)
    }
  }
}

/// HMAC-SHA256
fn hmac_sha256(key: BitArray, message: BitArray) -> BitArray {
  crypto.hmac(message, crypto.Sha256, key)
}

/// XOR two byte arrays of equal length
fn xor_bytes(a: BitArray, b: BitArray) -> BitArray {
  let a_bytes = bit_array_to_list(a)
  let b_bytes = bit_array_to_list(b)

  list.zip(a_bytes, b_bytes)
  |> list.map(fn(pair) {
    let #(x, y) = pair
    int.bitwise_exclusive_or(x, y)
  })
  |> list_to_bit_array
}

fn bit_array_to_list(bits: BitArray) -> List(Int) {
  do_bit_array_to_list(bits, [])
  |> list.reverse
}

fn do_bit_array_to_list(bits: BitArray, acc: List(Int)) -> List(Int) {
  case bits {
    <<byte:8, rest:bits>> -> do_bit_array_to_list(rest, [byte, ..acc])
    _ -> acc
  }
}

fn list_to_bit_array(bytes: List(Int)) -> BitArray {
  bytes
  |> list.fold(<<>>, fn(acc, byte) { <<acc:bits, byte:8>> })
}

/// Constant-time string comparison to prevent timing attacks
fn secure_compare(a: String, b: String) -> Bool {
  case string.length(a) == string.length(b) {
    False -> False
    True -> {
      let a_bytes = bit_array.from_string(a)
      let b_bytes = bit_array.from_string(b)
      do_secure_compare(a_bytes, b_bytes, 0)
    }
  }
}

fn do_secure_compare(a: BitArray, b: BitArray, diff: Int) -> Bool {
  case a, b {
    <<a_byte:8, a_rest:bits>>, <<b_byte:8, b_rest:bits>> -> {
      let new_diff =
        int.bitwise_or(diff, int.bitwise_exclusive_or(a_byte, b_byte))
      do_secure_compare(a_rest, b_rest, new_diff)
    }
    <<>>, <<>> -> diff == 0
    _, _ -> False
  }
}
