//! AI Settings integration tests

mod common;

use axum::http::StatusCode;
use serde_json::json;

use common::{TestClient, test_email, test_password};

/// Helper to get an auth token
async fn get_auth_token(client: &TestClient) -> String {
    let email = test_email();
    let password = test_password();

    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": &email,
                "password": &password,
            }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    response.json_value()["access_token"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Helper to create an organization
async fn create_org(client: &TestClient, token: &str) -> String {
    let slug = format!(
        "ai-test-org-{}",
        uuid::Uuid::new_v4().to_string().split('-').next().unwrap()
    );
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": "AI Test Org",
                "slug": &slug
            }),
            token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    response.json_value()["id"].as_str().unwrap().to_string()
}

/// Helper to create a workspace
async fn create_workspace(client: &TestClient, token: &str, org_id: &str) -> String {
    let slug = format!(
        "ai-test-ws-{}",
        uuid::Uuid::new_v4().to_string().split('-').next().unwrap()
    );
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({
                "name": "AI Test Workspace",
                "slug": &slug
            }),
            token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    response.json_value()["id"].as_str().unwrap().to_string()
}

// =============================================================================
// Organization AI Settings Tests
// =============================================================================

#[tokio::test]
async fn test_get_org_ai_settings_default() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

    let response = client
        .get_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "self_hosted");
    assert_eq!(body["has_litellm_key"], false);
    assert_eq!(body["has_openai_api_key"], false);
    assert_eq!(body["has_anthropic_api_key"], false);
    assert_eq!(body["has_bedrock_credentials"], false);
}

#[tokio::test]
async fn test_get_org_ai_settings_unauthorized() {
    let client = TestClient::with_db().await;
    let org_id = uuid::Uuid::new_v4();

    let response = client
        .get(&format!("/api/organizations/{}/settings/ai", org_id))
        .await;

    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_upsert_org_ai_settings_self_hosted() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

    let response = client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "self_hosted",
                "litellm_host": "http://localhost:4000",
                "litellm_key": "sk-test-key"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "self_hosted");
    assert_eq!(body["has_litellm_key"], true);
    assert_eq!(body["litellm_host"], "http://localhost:4000");
}

#[tokio::test]
async fn test_upsert_org_ai_settings_openai() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

    let response = client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "openai",
                "openai_api_key": "sk-openai-test",
                "openai_base_url": "https://api.openai.com/v1",
                "model_fast": "gpt-4o-mini",
                "model_reasoning": "gpt-4o"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "openai");
    assert_eq!(body["has_openai_api_key"], true);
    assert_eq!(body["openai_base_url"], "https://api.openai.com/v1");
    assert_eq!(body["model_fast"], "gpt-4o-mini");
    assert_eq!(body["model_reasoning"], "gpt-4o");
}

#[tokio::test]
async fn test_upsert_org_ai_settings_anthropic() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

    let response = client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "anthropic",
                "anthropic_api_key": "sk-ant-test",
                "anthropic_base_url": "https://api.anthropic.com",
                "model_fast": "claude-3-haiku-20240307",
                "model_reasoning": "claude-3-5-sonnet-20241022"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "anthropic");
    assert_eq!(body["has_anthropic_api_key"], true);
}

#[tokio::test]
async fn test_upsert_org_ai_settings_bedrock() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

    let response = client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "bedrock",
                "bedrock_region": "us-east-1",
                "bedrock_access_key": "AKIATEST",
                "bedrock_secret_key": "secret123",
                "bedrock_use_iam_role": false
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "bedrock");
    assert_eq!(body["bedrock_region"], "us-east-1");
    assert_eq!(body["has_bedrock_credentials"], true);
    assert_eq!(body["bedrock_use_iam_role"], false);
}

#[tokio::test]
async fn test_upsert_org_ai_settings_bedrock_iam_role() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

    let response = client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "bedrock",
                "bedrock_region": "us-west-2",
                "bedrock_use_iam_role": true
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "bedrock");
    assert_eq!(body["bedrock_use_iam_role"], true);
}

#[tokio::test]
async fn test_upsert_org_ai_settings_invalid_provider() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

    let response = client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "invalid_provider"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    let body = response.json_value();
    assert!(body["error"].as_str().unwrap().contains("Invalid provider"));
}

#[tokio::test]
async fn test_upsert_org_ai_settings_update_existing() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

    // Create initial settings
    let response = client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "openai",
                "openai_api_key": "initial-key"
            }),
            &token,
        )
        .await;
    response.assert_status(StatusCode::OK);

    // Update settings
    let response = client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "model_fast": "gpt-4o-mini"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    // Provider should be preserved
    assert_eq!(body["provider"], "openai");
    // Key should be preserved
    assert_eq!(body["has_openai_api_key"], true);
    // New field should be set
    assert_eq!(body["model_fast"], "gpt-4o-mini");
}

#[tokio::test]
async fn test_delete_org_ai_settings() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

    // Create settings first
    client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "openai",
                "openai_api_key": "test-key"
            }),
            &token,
        )
        .await;

    // Delete
    let response = client
        .delete_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &token,
        )
        .await;
    response.assert_status(StatusCode::NO_CONTENT);

    // Verify deleted - should get defaults
    let response = client
        .get_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &token,
        )
        .await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "self_hosted");
    assert_eq!(body["has_openai_api_key"], false);
}

#[tokio::test]
async fn test_delete_org_ai_settings_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

    // Delete without creating first
    let response = client
        .delete_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &token,
        )
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}

// =============================================================================
// Workspace AI Settings Tests
// =============================================================================

#[tokio::test]
async fn test_get_workspace_ai_settings_default() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;

    let response = client
        .get_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                org_id, ws_id
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "self_hosted");
}

#[tokio::test]
async fn test_upsert_workspace_ai_settings() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;

    let response = client
        .put_json_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                org_id, ws_id
            ),
            &json!({
                "provider": "anthropic",
                "anthropic_api_key": "workspace-key",
                "model_fast": "claude-3-haiku-20240307"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "anthropic");
    assert_eq!(body["has_anthropic_api_key"], true);
    assert_eq!(body["model_fast"], "claude-3-haiku-20240307");
}

#[tokio::test]
async fn test_upsert_workspace_ai_settings_invalid_provider() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;

    let response = client
        .put_json_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                org_id, ws_id
            ),
            &json!({
                "provider": "invalid"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_delete_workspace_ai_settings() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;

    // Create settings
    client
        .put_json_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                org_id, ws_id
            ),
            &json!({
                "provider": "openai",
                "openai_api_key": "ws-key"
            }),
            &token,
        )
        .await;

    // Delete
    let response = client
        .delete_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                org_id, ws_id
            ),
            &token,
        )
        .await;
    response.assert_status(StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_delete_workspace_ai_settings_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;

    let response = client
        .delete_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                org_id, ws_id
            ),
            &token,
        )
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}

// =============================================================================
// Effective Settings Tests
// =============================================================================

#[tokio::test]
async fn test_get_effective_settings_defaults() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;

    let response = client
        .get_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai/effective",
                org_id, ws_id
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "self_hosted");
}

#[tokio::test]
async fn test_get_effective_settings_org_only() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;

    // Set org settings
    client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "openai",
                "openai_api_key": "org-key",
                "model_fast": "gpt-4o-mini"
            }),
            &token,
        )
        .await;

    // Get effective settings
    let response = client
        .get_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai/effective",
                org_id, ws_id
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "openai");
    assert_eq!(body["has_openai_api_key"], true);
    assert_eq!(body["model_fast"], "gpt-4o-mini");
}

#[tokio::test]
async fn test_get_effective_settings_workspace_override() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;

    // Set org settings
    client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "openai",
                "openai_api_key": "org-key",
                "model_fast": "gpt-4o-mini",
                "model_reasoning": "gpt-4o"
            }),
            &token,
        )
        .await;

    // Set workspace settings to override provider and model_fast
    client
        .put_json_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                org_id, ws_id
            ),
            &json!({
                "provider": "anthropic",
                "anthropic_api_key": "ws-key",
                "model_fast": "claude-3-haiku-20240307"
            }),
            &token,
        )
        .await;

    // Get effective settings
    let response = client
        .get_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai/effective",
                org_id, ws_id
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    // Workspace overrides
    assert_eq!(body["provider"], "anthropic");
    assert_eq!(body["has_anthropic_api_key"], true);
    assert_eq!(body["model_fast"], "claude-3-haiku-20240307");
    // Org settings still preserved where workspace doesn't override
    // Note: model_reasoning from org should still be present unless workspace explicitly sets it
}

#[tokio::test]
async fn test_get_effective_settings_partial_workspace_override() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;

    // Set org settings with all models
    client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "openai",
                "openai_api_key": "org-key",
                "model_fast": "gpt-4o-mini",
                "model_reasoning": "gpt-4o",
                "model_embedding": "text-embedding-3-small"
            }),
            &token,
        )
        .await;

    // Set workspace to override only the reasoning model
    client
        .put_json_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                org_id, ws_id
            ),
            &json!({
                "model_reasoning": "o1-preview"
            }),
            &token,
        )
        .await;

    // Get effective settings
    let response = client
        .get_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai/effective",
                org_id, ws_id
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    // Org settings preserved
    assert_eq!(body["provider"], "openai");
    assert_eq!(body["has_openai_api_key"], true);
    assert_eq!(body["model_fast"], "gpt-4o-mini");
    assert_eq!(body["model_embedding"], "text-embedding-3-small");
    // Workspace override
    assert_eq!(body["model_reasoning"], "o1-preview");
}

// =============================================================================
// Authorization Tests
// =============================================================================

#[tokio::test]
async fn test_org_ai_settings_unauthorized() {
    let client = TestClient::with_db().await;
    let org_id = uuid::Uuid::new_v4();

    // GET
    let response = client
        .get(&format!("/api/organizations/{}/settings/ai", org_id))
        .await;
    response.assert_status(StatusCode::UNAUTHORIZED);

    // PUT - need to use raw request since put_json_auth requires a token
    // The unauthorized response will be caught by the middleware
}

#[tokio::test]
async fn test_workspace_ai_settings_unauthorized() {
    let client = TestClient::with_db().await;
    let org_id = uuid::Uuid::new_v4();
    let ws_id = uuid::Uuid::new_v4();

    let response = client
        .get(&format!(
            "/api/organizations/{}/workspaces/{}/settings/ai",
            org_id, ws_id
        ))
        .await;
    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_effective_settings_unauthorized() {
    let client = TestClient::with_db().await;
    let org_id = uuid::Uuid::new_v4();
    let ws_id = uuid::Uuid::new_v4();

    let response = client
        .get(&format!(
            "/api/organizations/{}/workspaces/{}/settings/ai/effective",
            org_id, ws_id
        ))
        .await;
    response.assert_status(StatusCode::UNAUTHORIZED);
}

// =============================================================================
// Model Configuration Tests
// =============================================================================

#[tokio::test]
async fn test_ai_settings_with_all_models() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

    let response = client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "openai",
                "openai_api_key": "test-key",
                "model_fast": "gpt-4o-mini",
                "model_reasoning": "o1-preview",
                "model_embedding": "text-embedding-3-large"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["model_fast"], "gpt-4o-mini");
    assert_eq!(body["model_reasoning"], "o1-preview");
    assert_eq!(body["model_embedding"], "text-embedding-3-large");
}

#[tokio::test]
async fn test_credentials_not_exposed_in_response() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

    // Set settings with sensitive credentials
    client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "openai",
                "openai_api_key": "sk-secret-key-12345",
                "litellm_key": "secret-litellm-key"
            }),
            &token,
        )
        .await;

    // Get settings - credentials should not be exposed
    let response = client
        .get_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();

    // Should show has_*_key flags but not actual keys
    assert_eq!(body["has_openai_api_key"], true);
    assert_eq!(body["has_litellm_key"], true);

    // Should NOT contain actual key values
    assert!(body.get("openai_api_key").is_none());
    assert!(body.get("litellm_key").is_none());
}
