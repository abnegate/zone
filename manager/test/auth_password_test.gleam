import auth/password
import gleam/string
import gleeunit/should

// --- Password Hashing Tests ---

pub fn hash_password_returns_non_empty_string_test() {
  let hash = password.hash_password("my_secure_password")

  hash
  |> string.is_empty
  |> should.be_false
}

pub fn hash_password_includes_salt_test() {
  // Hash format should be: $pbkdf2-sha256$iterations$salt$hash
  let hash = password.hash_password("test_password")

  hash
  |> string.contains("$")
  |> should.be_true

  // Should have 5 parts separated by $ (empty first part due to leading $)
  let parts = string.split(hash, "$")
  parts
  |> list.length
  |> should.equal(5)
}

pub fn hash_password_different_salts_produce_different_hashes_test() {
  let password_str = "same_password"
  let hash1 = password.hash_password(password_str)
  let hash2 = password.hash_password(password_str)

  // Same password should produce different hashes due to random salt
  hash1
  |> should.not_equal(hash2)
}

pub fn hash_password_uses_pbkdf2_algorithm_test() {
  let hash = password.hash_password("test")

  hash
  |> string.starts_with("$pbkdf2-sha256$")
  |> should.be_true
}

// --- Password Verification Tests ---

pub fn verify_password_correct_password_returns_true_test() {
  let original = "my_secret_password_123"
  let hash = password.hash_password(original)

  password.verify_password(original, hash)
  |> should.be_true
}

pub fn verify_password_wrong_password_returns_false_test() {
  let original = "correct_password"
  let hash = password.hash_password(original)

  password.verify_password("wrong_password", hash)
  |> should.be_false
}

pub fn verify_password_empty_password_returns_false_test() {
  let hash = password.hash_password("some_password")

  password.verify_password("", hash)
  |> should.be_false
}

pub fn verify_password_case_sensitive_test() {
  let hash = password.hash_password("Password123")

  password.verify_password("password123", hash)
  |> should.be_false

  password.verify_password("PASSWORD123", hash)
  |> should.be_false

  password.verify_password("Password123", hash)
  |> should.be_true
}

pub fn verify_password_special_characters_test() {
  let special_pwd = "p@$$w0rd!#%^&*()_+-=[]{}|;':\",./<>?"
  let hash = password.hash_password(special_pwd)

  password.verify_password(special_pwd, hash)
  |> should.be_true
}

pub fn verify_password_unicode_test() {
  let unicode_pwd = "密码パスワード🔐"
  let hash = password.hash_password(unicode_pwd)

  password.verify_password(unicode_pwd, hash)
  |> should.be_true
}

pub fn verify_password_long_password_test() {
  let long_pwd = string.repeat("a", 1000)
  let hash = password.hash_password(long_pwd)

  password.verify_password(long_pwd, hash)
  |> should.be_true
}

pub fn verify_password_invalid_hash_format_returns_false_test() {
  password.verify_password("test", "invalid_hash")
  |> should.be_false

  password.verify_password("test", "")
  |> should.be_false

  password.verify_password("test", "a$b$c")
  |> should.be_false
}

pub fn verify_password_tampered_hash_returns_false_test() {
  let hash = password.hash_password("original")
  let tampered = string.replace(hash, "a", "b")

  password.verify_password("original", tampered)
  |> should.be_false
}

// --- Edge Cases ---

pub fn hash_and_verify_whitespace_password_test() {
  let whitespace_pwd = "   spaces   "
  let hash = password.hash_password(whitespace_pwd)

  password.verify_password(whitespace_pwd, hash)
  |> should.be_true

  // Trimmed version should not match
  password.verify_password("spaces", hash)
  |> should.be_false
}

pub fn hash_and_verify_newline_password_test() {
  let newline_pwd = "password\nwith\nnewlines"
  let hash = password.hash_password(newline_pwd)

  password.verify_password(newline_pwd, hash)
  |> should.be_true
}

import gleam/list
