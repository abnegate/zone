//! Email verification integration tests
//!
//! Tests for email verification token creation, validation, expiration,
//! and user verification flow.

mod common;

use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use zone_server::db::{email_verification, users};
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
    users::create_user(pool, &email, "password_hash", Some("Test User"), false)
        .await
        .expect("Failed to create user")
        .id
}

// =============================================================================
// Token Creation Tests
// =============================================================================

#[tokio::test]
async fn test_create_verification_token_success() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    let result = email_verification::create_verification_token(&pool, user_id).await;

    assert!(
        result.is_ok(),
        "Should successfully create verification token"
    );
    let (token, expires_at) = result.unwrap();
    assert!(!token.is_empty(), "Token should not be empty");
    assert!(expires_at > Utc::now(), "Token should not be expired yet");
}

#[tokio::test]
async fn test_create_verification_token_generates_unique_tokens() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    let (token1, _) = email_verification::create_verification_token(&pool, user_id)
        .await
        .expect("Failed to create first token");

    let (token2, _) = email_verification::create_verification_token(&pool, user_id)
        .await
        .expect("Failed to create second token");

    assert_ne!(token1, token2, "Tokens should be unique");
}

#[tokio::test]
async fn test_create_verification_token_invalidates_old_tokens() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    let (token1, _) = email_verification::create_verification_token(&pool, user_id)
        .await
        .expect("Failed to create first token");

    // Create a new token (should invalidate the old one)
    let (token2, _) = email_verification::create_verification_token(&pool, user_id)
        .await
        .expect("Failed to create second token");

    // Old token should no longer be valid
    let result = email_verification::verify_token(&pool, &token1).await;
    assert!(result.is_err(), "Old token should be invalid");

    // New token should be valid
    let result = email_verification::verify_token(&pool, &token2).await;
    assert!(result.is_ok(), "New token should be valid");
}

#[tokio::test]
async fn test_create_verification_token_for_nonexistent_user_fails() {
    let pool = create_test_pool().await;
    let fake_user_id = Uuid::new_v4();

    let result = email_verification::create_verification_token(&pool, fake_user_id).await;

    assert!(result.is_err(), "Should fail for non-existent user");
}

// =============================================================================
// Token Verification Tests
// =============================================================================

#[tokio::test]
async fn test_verify_token_success() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    let (token, _) = email_verification::create_verification_token(&pool, user_id)
        .await
        .expect("Failed to create token");

    let result = email_verification::verify_token(&pool, &token).await;

    assert!(result.is_ok(), "Should successfully verify token");
    assert_eq!(result.unwrap(), user_id, "Should return correct user_id");
}

#[tokio::test]
async fn test_verify_token_invalid_token_fails() {
    let pool = create_test_pool().await;

    let result = email_verification::verify_token(&pool, "invalid-token").await;

    assert!(result.is_err(), "Should fail for invalid token");
}

#[tokio::test]
async fn test_verify_token_expired_token_fails() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    // Create a token that's already expired (manual insertion)
    let token = format!("expired-{}", Uuid::new_v4());
    let token_hash = hash_token(&token);
    let expires_at = Utc::now() - Duration::hours(1); // Expired 1 hour ago

    let _ = sqlx::query(
        "INSERT INTO email_verification_tokens (user_id, token_hash, expires_at) VALUES ($1, $2, $3)",
    )
    .bind(user_id)
    .bind(&token_hash)
    .bind(expires_at)
    .execute(&pool)
    .await
    .expect("Failed to insert expired token");

    let result = email_verification::verify_token(&pool, &token).await;

    assert!(result.is_err(), "Should fail for expired token");
}

#[tokio::test]
async fn test_verify_token_deletes_token_after_verification() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    let (token, _) = email_verification::create_verification_token(&pool, user_id)
        .await
        .expect("Failed to create token");

    // First verification should succeed
    let result = email_verification::verify_token(&pool, &token).await;
    assert!(result.is_ok(), "First verification should succeed");

    // Second verification should fail (token deleted)
    let result = email_verification::verify_token(&pool, &token).await;
    assert!(result.is_err(), "Second verification should fail");
}

// =============================================================================
// Mark Email Verified Tests
// =============================================================================

#[tokio::test]
async fn test_mark_email_verified_success() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    // Verify user is not initially verified
    let user = users::get_user_by_id(&pool, user_id)
        .await
        .expect("Failed to get user")
        .expect("User should exist");
    assert!(
        !user.email_verified,
        "User should not be verified initially"
    );

    let result = email_verification::mark_email_verified(&pool, user_id).await;

    assert!(result.is_ok(), "Should successfully mark email as verified");

    // Verify user is now verified
    let user = users::get_user_by_id(&pool, user_id)
        .await
        .expect("Failed to get user")
        .expect("User should exist");
    assert!(user.email_verified, "User should be verified now");
}

#[tokio::test]
async fn test_mark_email_verified_for_nonexistent_user_fails() {
    let pool = create_test_pool().await;
    let fake_user_id = Uuid::new_v4();

    let result = email_verification::mark_email_verified(&pool, fake_user_id).await;

    assert!(result.is_err(), "Should fail for non-existent user");
}

#[tokio::test]
async fn test_mark_email_verified_idempotent() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    // Mark verified twice
    email_verification::mark_email_verified(&pool, user_id)
        .await
        .expect("First verification should succeed");

    let result = email_verification::mark_email_verified(&pool, user_id).await;

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
        "INSERT INTO email_verification_tokens (user_id, token_hash, expires_at) VALUES ($1, $2, $3)",
    )
    .bind(user1_id)
    .bind(&expired_token_hash)
    .bind(expires_at)
    .execute(&pool)
    .await
    .expect("Failed to insert expired token");

    // Create a valid token
    let (valid_token, _) = email_verification::create_verification_token(&pool, user2_id)
        .await
        .expect("Failed to create valid token");

    // Delete expired tokens
    let result = email_verification::delete_expired_tokens(&pool).await;
    assert!(result.is_ok(), "Should successfully delete expired tokens");

    // Expired token should be gone (cannot be verified)
    let result = email_verification::verify_token(&pool, &expired_token).await;
    assert!(result.is_err(), "Expired token should be deleted");

    // Valid token should still exist
    let result = email_verification::verify_token(&pool, &valid_token).await;
    assert!(result.is_ok(), "Valid token should still exist");
}

#[tokio::test]
async fn test_delete_expired_tokens_does_not_affect_valid_tokens() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    // Create a valid token
    let (token, _) = email_verification::create_verification_token(&pool, user_id)
        .await
        .expect("Failed to create token");

    // Call delete_expired_tokens - should not affect our valid token
    let result = email_verification::delete_expired_tokens(&pool).await;
    assert!(result.is_ok(), "delete_expired_tokens should succeed");

    // The token we just created should still be valid
    let result = email_verification::verify_token(&pool, &token).await;
    assert!(
        result.is_ok(),
        "Valid token should not be deleted by delete_expired_tokens"
    );
}

// =============================================================================
// Full Verification Flow Test
// =============================================================================

#[tokio::test]
async fn test_full_verification_flow() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    // Step 1: Create verification token
    let (token, expires_at) = email_verification::create_verification_token(&pool, user_id)
        .await
        .expect("Failed to create token");

    assert!(!token.is_empty(), "Token should be generated");
    assert!(expires_at > Utc::now(), "Token should not be expired");

    // Step 2: Verify token returns correct user_id
    let verified_user_id = email_verification::verify_token(&pool, &token)
        .await
        .expect("Token verification should succeed");

    assert_eq!(verified_user_id, user_id, "Should return correct user_id");

    // Step 3: Mark email as verified
    email_verification::mark_email_verified(&pool, user_id)
        .await
        .expect("Should mark email as verified");

    // Step 4: Verify user.email_verified is true
    let user = users::get_user_by_id(&pool, user_id)
        .await
        .expect("Failed to get user")
        .expect("User should exist");

    assert!(user.email_verified, "User should be verified");

    // Step 5: Token should be consumed (deleted after verification)
    let result = email_verification::verify_token(&pool, &token).await;
    assert!(
        result.is_err(),
        "Token should be consumed after verification"
    );
}

// =============================================================================
// Cascade Delete Tests
// =============================================================================

#[tokio::test]
async fn test_tokens_deleted_when_user_deleted() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;

    let (token, _) = email_verification::create_verification_token(&pool, user_id)
        .await
        .expect("Failed to create token");

    // Delete the user
    users::delete_user(&pool, user_id)
        .await
        .expect("Failed to delete user");

    // Token should be gone (cascade delete)
    let result = email_verification::verify_token(&pool, &token).await;
    assert!(
        result.is_err(),
        "Token should be deleted when user is deleted"
    );
}
