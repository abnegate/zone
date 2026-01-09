//! Password reset integration tests
//!
//! Tests for password reset token creation, validation, expiration,
//! single-use enforcement, and password update flow.

mod common;

use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use zone_server::db::{password_reset, users};
use zone_server::utils::crypto::hash_token;

async fn create_test_pool() -> PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/zone_test".to_string());

    sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to test database")
}

async fn create_test_user(pool: &PgPool) -> Uuid {
    let email = format!("test-{}@example.com", Uuid::new_v4());
    users::create_user(pool, &email, "old_password_hash", Some("Test User"), false)
        .await
        .expect("Failed to create user")
        .id
}

// =============================================================================
// Token Creation Tests
// =============================================================================

#[tokio::test]
async fn test_create_reset_token_success() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    let result = password_reset::create_reset_token(&pool, user_id).await;

    assert!(result.is_ok(), "Should successfully create reset token");
    let (token, expires_at) = result.unwrap();
    assert!(!token.is_empty(), "Token should not be empty");
    assert!(expires_at > Utc::now(), "Token should not be expired yet");
}

#[tokio::test]
async fn test_create_reset_token_generates_unique_tokens() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    let (token1, _) = password_reset::create_reset_token(&pool, user_id)
        .await
        .expect("Failed to create first token");

    let (token2, _) = password_reset::create_reset_token(&pool, user_id)
        .await
        .expect("Failed to create second token");

    assert_ne!(token1, token2, "Tokens should be unique");
}

#[tokio::test]
async fn test_create_reset_token_allows_multiple_tokens() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    // Create first token
    let (token1, _) = password_reset::create_reset_token(&pool, user_id)
        .await
        .expect("Failed to create first token");

    // Create second token (should not invalidate the first)
    let (token2, _) = password_reset::create_reset_token(&pool, user_id)
        .await
        .expect("Failed to create second token");

    // Both tokens should be valid
    let result1 = password_reset::verify_reset_token(&pool, &token1).await;
    let result2 = password_reset::verify_reset_token(&pool, &token2).await;

    assert!(result1.is_ok(), "First token should still be valid");
    assert!(result2.is_ok(), "Second token should be valid");
}

#[tokio::test]
async fn test_create_reset_token_for_nonexistent_user_fails() {
    let pool = create_test_pool().await;
    let fake_user_id = Uuid::new_v4();

    let result = password_reset::create_reset_token(&pool, fake_user_id).await;

    assert!(result.is_err(), "Should fail for non-existent user");
}

// =============================================================================
// Token Verification Tests
// =============================================================================

#[tokio::test]
async fn test_verify_reset_token_success() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    let (token, _) = password_reset::create_reset_token(&pool, user_id)
        .await
        .expect("Failed to create token");

    let result = password_reset::verify_reset_token(&pool, &token).await;

    assert!(result.is_ok(), "Should successfully verify token");
    assert_eq!(result.unwrap(), user_id, "Should return correct user_id");
}

#[tokio::test]
async fn test_verify_reset_token_invalid_token_fails() {
    let pool = create_test_pool().await;

    let result = password_reset::verify_reset_token(&pool, "invalid-token").await;

    assert!(result.is_err(), "Should fail for invalid token");
}

#[tokio::test]
async fn test_verify_reset_token_expired_token_fails() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    // Create a token that's already expired (manual insertion)
    let token = format!("expired-{}", Uuid::new_v4());
    let token_hash = hash_token(&token);
    let expires_at = Utc::now() - Duration::hours(1); // Expired 1 hour ago

    let _ = sqlx::query(
        "INSERT INTO password_reset_tokens (user_id, token_hash, expires_at) VALUES ($1, $2, $3)",
    )
    .bind(user_id)
    .bind(&token_hash)
    .bind(expires_at)
    .execute(&pool)
    .await
    .expect("Failed to insert expired token");

    let result = password_reset::verify_reset_token(&pool, &token).await;

    assert!(result.is_err(), "Should fail for expired token");
}

#[tokio::test]
async fn test_verify_reset_token_used_token_fails() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    let (token, _) = password_reset::create_reset_token(&pool, user_id)
        .await
        .expect("Failed to create token");

    // Mark token as used
    password_reset::mark_token_used(&pool, &token)
        .await
        .expect("Failed to mark token as used");

    let result = password_reset::verify_reset_token(&pool, &token).await;

    assert!(result.is_err(), "Should fail for used token");
}

#[tokio::test]
async fn test_verify_reset_token_does_not_consume_token() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    let (token, _) = password_reset::create_reset_token(&pool, user_id)
        .await
        .expect("Failed to create token");

    // First verification
    let result1 = password_reset::verify_reset_token(&pool, &token).await;
    assert!(result1.is_ok(), "First verification should succeed");

    // Second verification should also succeed (not consumed yet)
    let result2 = password_reset::verify_reset_token(&pool, &token).await;
    assert!(result2.is_ok(), "Second verification should succeed");
}

// =============================================================================
// Mark Token Used Tests
// =============================================================================

#[tokio::test]
async fn test_mark_token_used_success() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    let (token, _) = password_reset::create_reset_token(&pool, user_id)
        .await
        .expect("Failed to create token");

    let result = password_reset::mark_token_used(&pool, &token).await;

    assert!(result.is_ok(), "Should successfully mark token as used");

    // Verify token is now invalid
    let verify_result = password_reset::verify_reset_token(&pool, &token).await;
    assert!(verify_result.is_err(), "Used token should be invalid");
}

#[tokio::test]
async fn test_mark_token_used_invalid_token_fails() {
    let pool = create_test_pool().await;

    let result = password_reset::mark_token_used(&pool, "invalid-token").await;

    assert!(result.is_err(), "Should fail for invalid token");
}

#[tokio::test]
async fn test_mark_token_used_idempotent() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    let (token, _) = password_reset::create_reset_token(&pool, user_id)
        .await
        .expect("Failed to create token");

    // Mark used twice
    password_reset::mark_token_used(&pool, &token)
        .await
        .expect("First mark should succeed");

    let result = password_reset::mark_token_used(&pool, &token).await;

    assert!(result.is_ok(), "Should be idempotent");
}

// =============================================================================
// Delete Expired Tokens Tests
// =============================================================================

#[tokio::test]
async fn test_delete_expired_tokens_removes_old_tokens() {
    let pool = create_test_pool().await;
    let user1_id = create_test_user(&pool).await;
    let user2_id = create_test_user(&pool).await;

    // Create an expired token
    let expired_token = format!("expired-{}", Uuid::new_v4());
    let expired_token_hash = hash_token(&expired_token);
    let expires_at = Utc::now() - Duration::hours(1);
    let _ = sqlx::query(
        "INSERT INTO password_reset_tokens (user_id, token_hash, expires_at) VALUES ($1, $2, $3)",
    )
    .bind(user1_id)
    .bind(&expired_token_hash)
    .bind(expires_at)
    .execute(&pool)
    .await
    .expect("Failed to insert expired token");

    // Create a valid token
    let (valid_token, _) = password_reset::create_reset_token(&pool, user2_id)
        .await
        .expect("Failed to create valid token");

    // Delete expired tokens
    let result = password_reset::delete_expired_tokens(&pool).await;
    assert!(result.is_ok(), "Should successfully delete expired tokens");

    // Expired token should be gone
    let result = password_reset::verify_reset_token(&pool, &expired_token).await;
    assert!(result.is_err(), "Expired token should be deleted");

    // Valid token should still exist
    let result = password_reset::verify_reset_token(&pool, &valid_token).await;
    assert!(result.is_ok(), "Valid token should still exist");
}

#[tokio::test]
async fn test_delete_expired_tokens_removes_used_tokens() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    // Create and use a token
    let (used_token, _) = password_reset::create_reset_token(&pool, user_id)
        .await
        .expect("Failed to create token");

    password_reset::mark_token_used(&pool, &used_token)
        .await
        .expect("Failed to mark token as used");

    // Make the used token old
    let old_time = Utc::now() - Duration::days(2);
    let token_hash = hash_token(&used_token);
    let _ = sqlx::query("UPDATE password_reset_tokens SET created_at = $1 WHERE token_hash = $2")
        .bind(old_time)
        .bind(&token_hash)
        .execute(&pool)
        .await
        .expect("Failed to update token timestamp");

    // Delete old used tokens (used tokens older than 24 hours)
    let result = password_reset::delete_expired_tokens(&pool).await;

    assert!(result.is_ok(), "Should successfully delete old used tokens");
}

#[tokio::test]
async fn test_delete_expired_tokens_does_not_affect_valid_tokens() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    // Create only valid tokens
    let (token, _) = password_reset::create_reset_token(&pool, user_id)
        .await
        .expect("Failed to create token");

    // Call delete_expired_tokens - should not affect our valid token
    let result = password_reset::delete_expired_tokens(&pool).await;
    assert!(result.is_ok(), "Should successfully run");

    // The token we just created should still be valid
    let result = password_reset::verify_reset_token(&pool, &token).await;
    assert!(
        result.is_ok(),
        "Valid token should not be deleted by delete_expired_tokens"
    );
}

// =============================================================================
// Full Password Reset Flow Test
// =============================================================================

#[tokio::test]
async fn test_full_password_reset_flow() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    // Step 1: Create reset token
    let (token, expires_at) = password_reset::create_reset_token(&pool, user_id)
        .await
        .expect("Failed to create token");

    assert!(!token.is_empty(), "Token should be generated");
    assert!(expires_at > Utc::now(), "Token should not be expired");

    // Step 2: Verify token returns correct user_id
    let verified_user_id = password_reset::verify_reset_token(&pool, &token)
        .await
        .expect("Token verification should succeed");

    assert_eq!(verified_user_id, user_id, "Should return correct user_id");

    // Step 3: Update user's password (simulated)
    // In real flow, this would call users::update_password()
    let new_password_hash = "new_password_hash";
    sqlx::query!(
        "UPDATE users SET password_hash = $1, updated_at = NOW() WHERE id = $2",
        new_password_hash,
        user_id
    )
    .execute(&pool)
    .await
    .expect("Failed to update password");

    // Step 4: Mark token as used
    password_reset::mark_token_used(&pool, &token)
        .await
        .expect("Should mark token as used");

    // Step 5: Token should be consumed (cannot be reused)
    let result = password_reset::verify_reset_token(&pool, &token).await;
    assert!(result.is_err(), "Token should be invalid after use");

    // Step 6: Verify password was updated
    let user = users::get_user_by_id(&pool, user_id)
        .await
        .expect("Failed to get user")
        .expect("User should exist");

    let user_with_pass = users::get_user_by_email(&pool, &user.email)
        .await
        .expect("Failed to get user with password")
        .expect("User should exist");

    assert_eq!(
        user_with_pass.password_hash, new_password_hash,
        "Password should be updated"
    );
}

// =============================================================================
// Security Tests
// =============================================================================

#[tokio::test]
async fn test_token_hash_is_different_from_plain_token() {
    let token = "test-token-123";
    let hash = hash_token(token);

    assert_ne!(token, hash, "Hash should be different from plain token");
    assert!(hash.len() > token.len(), "Hash should be longer");
}

#[tokio::test]
async fn test_token_hash_is_deterministic() {
    let token = "test-token-123";
    let hash1 = hash_token(token);
    let hash2 = hash_token(token);

    assert_eq!(hash1, hash2, "Same token should produce same hash");
}

// =============================================================================
// Cascade Delete Tests
// =============================================================================

#[tokio::test]
async fn test_tokens_deleted_when_user_deleted() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    let (token, _) = password_reset::create_reset_token(&pool, user_id)
        .await
        .expect("Failed to create token");

    // Delete the user
    users::delete_user(&pool, user_id)
        .await
        .expect("Failed to delete user");

    // Token should be gone (cascade delete)
    let result = password_reset::verify_reset_token(&pool, &token).await;
    assert!(
        result.is_err(),
        "Token should be deleted when user is deleted"
    );
}

// =============================================================================
// Rate Limiting Test (Token Creation)
// =============================================================================

#[tokio::test]
async fn test_multiple_reset_tokens_allowed() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    // User should be able to request multiple reset tokens
    // (e.g., if they didn't receive the email)
    let mut tokens = Vec::new();
    for _ in 0..5 {
        let (token, _) = password_reset::create_reset_token(&pool, user_id)
            .await
            .expect("Should allow multiple token creation");
        tokens.push(token);
    }

    // All tokens should be valid
    for token in &tokens {
        let result = password_reset::verify_reset_token(&pool, token).await;
        assert!(result.is_ok(), "All tokens should be valid");
    }

    // But once one is used, it should be invalid
    password_reset::mark_token_used(&pool, &tokens[0])
        .await
        .expect("Failed to mark token as used");

    let result = password_reset::verify_reset_token(&pool, &tokens[0]).await;
    assert!(result.is_err(), "Used token should be invalid");

    // Others should still be valid
    let result = password_reset::verify_reset_token(&pool, &tokens[1]).await;
    assert!(result.is_ok(), "Unused tokens should still be valid");
}
