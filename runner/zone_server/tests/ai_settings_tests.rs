//! AI Settings integration tests

mod common;

use axum::http::StatusCode;
use serde_json::json;

use common::{TestClient, test_email, test_password};

const AGENT_PROVIDERS: [&str; 2] = ["claude_code", "codex"];
const MODEL_FIELDS: [&str; 3] = ["model_fast", "model_reasoning", "model_embedding"];
const INVALID_PROVIDER: &str =
    "Invalid provider. Must be one of: self_hosted, openai, anthropic, bedrock, claude_code, codex";

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
    response.json_value()["organization"]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

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
    response.json_value()["workspace"]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

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
    assert_eq!(response.json_value()["error"], INVALID_PROVIDER);
}

#[tokio::test]
async fn test_upsert_org_ai_settings_agent_providers() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let path = format!("/api/organizations/{org_id}/settings/ai");

    for provider in AGENT_PROVIDERS {
        let response = client
            .put_json_auth(&path, &json!({ "provider": provider }), &token)
            .await;
        response.assert_status(StatusCode::OK);
        assert_eq!(response.json_value()["provider"], provider);

        let stored = client.get_auth(&path, &token).await;
        stored.assert_status(StatusCode::OK);
        assert_eq!(
            stored.json_value()["provider"],
            provider,
            "the organization must keep {provider} once it is saved"
        );
    }
}

#[tokio::test]
async fn test_upsert_org_ai_settings_update_existing() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

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
    assert_eq!(body["provider"], "openai");
    assert_eq!(body["has_openai_api_key"], true);
    assert_eq!(body["model_fast"], "gpt-4o-mini");
}

#[tokio::test]
async fn test_delete_org_ai_settings() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

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

    let response = client
        .delete_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &token,
        )
        .await;
    response.assert_status(StatusCode::NO_CONTENT);

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

    let response = client
        .delete_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &token,
        )
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}

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
    assert_eq!(response.json_value()["error"], INVALID_PROVIDER);
}

#[tokio::test]
async fn test_upsert_workspace_ai_settings_agent_providers() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;
    let path = format!("/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai");

    for provider in AGENT_PROVIDERS {
        let response = client
            .put_json_auth(&path, &json!({ "provider": provider }), &token)
            .await;
        response.assert_status(StatusCode::OK);
        assert_eq!(response.json_value()["provider"], provider);

        let effective = client.get_auth(&format!("{path}/effective"), &token).await;
        effective.assert_status(StatusCode::OK);
        assert_eq!(
            effective.json_value()["provider"],
            provider,
            "a workspace override to {provider} must win over the organization's self_hosted"
        );
    }
}

#[tokio::test]
async fn test_delete_workspace_ai_settings() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;

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
    assert_eq!(body["provider"], "anthropic");
    assert_eq!(body["has_anthropic_api_key"], true);
    assert_eq!(body["model_fast"], "claude-3-haiku-20240307");
    assert_eq!(body["model_reasoning"], "gpt-4o");
}

#[tokio::test]
async fn test_get_effective_settings_partial_workspace_override() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;

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
    assert_eq!(body["model_embedding"], "text-embedding-3-small");
    assert_eq!(body["model_reasoning"], "o1-preview");
}

#[tokio::test]
async fn test_org_ai_settings_unauthorized() {
    let client = TestClient::with_db().await;
    let org_id = uuid::Uuid::new_v4();

    let response = client
        .get(&format!("/api/organizations/{}/settings/ai", org_id))
        .await;
    response.assert_status(StatusCode::UNAUTHORIZED);
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
                "model_embedding": "text-embedding-3-large",
                "model_image": "custom-image.safetensors",
                "model_video": "custom-video.safetensors",
                "model_audio": "custom-audio.safetensors"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["model_fast"], "gpt-4o-mini");
    assert_eq!(body["model_reasoning"], "o1-preview");
    assert_eq!(body["model_embedding"], "text-embedding-3-large");
    assert_eq!(body["model_image"], "custom-image.safetensors");
    assert_eq!(body["model_video"], "custom-video.safetensors");
    assert_eq!(body["model_audio"], "custom-audio.safetensors");
}

#[tokio::test]
async fn test_empty_model_video_clears_saved_override() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

    client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "self_hosted",
                "model_image": "custom-image.safetensors",
                "model_video": "custom-video.safetensors"
            }),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);

    let cleared = client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "self_hosted",
                "model_image": "",
                "model_video": ""
            }),
            &token,
        )
        .await;
    cleared.assert_status(StatusCode::OK);
    let body = cleared.json_value();
    assert!(body["model_image"].is_null());
    assert!(body["model_video"].is_null());

    client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "self_hosted",
                "model_video": "keep-video.safetensors"
            }),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);

    let omitted = client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({ "provider": "self_hosted" }),
            &token,
        )
        .await;
    omitted.assert_status(StatusCode::OK);
    assert_eq!(
        omitted.json_value()["model_video"],
        "keep-video.safetensors"
    );
}

#[tokio::test]
async fn test_empty_model_audio_clears_saved_override() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

    client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "self_hosted",
                "model_video": "custom-video.safetensors",
                "model_audio": "custom-audio.safetensors"
            }),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);

    let cleared = client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "self_hosted",
                "model_audio": ""
            }),
            &token,
        )
        .await;
    cleared.assert_status(StatusCode::OK);
    let body = cleared.json_value();
    assert!(body["model_audio"].is_null());
    assert_eq!(
        body["model_video"], "custom-video.safetensors",
        "clearing model_audio must not disturb the sibling video override"
    );

    client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "self_hosted",
                "model_audio": "keep-audio.safetensors"
            }),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);

    let omitted = client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({ "provider": "self_hosted" }),
            &token,
        )
        .await;
    omitted.assert_status(StatusCode::OK);
    assert_eq!(
        omitted.json_value()["model_audio"],
        "keep-audio.safetensors"
    );
}

#[tokio::test]
async fn test_workspace_model_audio_overrides_organization() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;

    client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "self_hosted",
                "model_audio": "org-audio.safetensors"
            }),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);

    let inherited = client
        .get_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai/effective",
                org_id, ws_id
            ),
            &token,
        )
        .await;
    inherited.assert_status(StatusCode::OK);
    assert_eq!(
        inherited.json_value()["model_audio"],
        "org-audio.safetensors"
    );

    let workspace_path = format!(
        "/api/organizations/{}/workspaces/{}/settings/ai",
        org_id, ws_id
    );

    client
        .put_json_auth(
            &workspace_path,
            &json!({
                "provider": "self_hosted",
                "model_audio": "ws-audio.safetensors"
            }),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);

    let overridden = client
        .get_auth(&format!("{}/effective", workspace_path), &token)
        .await;
    overridden.assert_status(StatusCode::OK);
    assert_eq!(
        overridden.json_value()["model_audio"],
        "ws-audio.safetensors"
    );

    let omitted = client
        .put_json_auth(
            &workspace_path,
            &json!({ "provider": "self_hosted" }),
            &token,
        )
        .await;
    omitted.assert_status(StatusCode::OK);
    assert_eq!(
        omitted.json_value()["model_audio"],
        "ws-audio.safetensors",
        "omitting model_audio must preserve the workspace override"
    );

    let cleared = client
        .put_json_auth(
            &workspace_path,
            &json!({
                "provider": "self_hosted",
                "model_audio": ""
            }),
            &token,
        )
        .await;
    cleared.assert_status(StatusCode::OK);
    assert!(cleared.json_value()["model_audio"].is_null());

    let reinherited = client
        .get_auth(&format!("{}/effective", workspace_path), &token)
        .await;
    reinherited.assert_status(StatusCode::OK);
    assert_eq!(
        reinherited.json_value()["model_audio"],
        "org-audio.safetensors",
        "clearing the workspace override must fall back to the organization"
    );
}

#[tokio::test]
async fn test_blank_models_clear_saved_organization_models() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let path = format!("/api/organizations/{org_id}/settings/ai");

    let first = client
        .put_json_auth(
            &path,
            &json!({
                "provider": "claude_code",
                "model_fast": "",
                "model_reasoning": "",
                "model_embedding": ""
            }),
            &token,
        )
        .await;
    first.assert_status(StatusCode::OK);
    for field in MODEL_FIELDS {
        assert!(
            first.json_value()[field].is_null(),
            "the first save must store a blank {field} as no model"
        );
    }

    for field in MODEL_FIELDS {
        client
            .put_json_auth(&path, &json!({ field: "saved-model" }), &token)
            .await
            .assert_status(StatusCode::OK);

        let omitted = client
            .put_json_auth(&path, &json!({ "provider": "claude_code" }), &token)
            .await;
        omitted.assert_status(StatusCode::OK);
        assert_eq!(
            omitted.json_value()[field],
            "saved-model",
            "leaving {field} out must keep the saved model"
        );

        let cleared = client
            .put_json_auth(
                &path,
                &json!({ "provider": "claude_code", field: "" }),
                &token,
            )
            .await;
        cleared.assert_status(StatusCode::OK);
        assert!(
            cleared.json_value()[field].is_null(),
            "a blank {field} must clear the saved model"
        );

        let stored = client.get_auth(&path, &token).await;
        stored.assert_status(StatusCode::OK);
        assert!(
            stored.json_value()[field].is_null(),
            "the cleared {field} must stay cleared"
        );
    }
}

#[tokio::test]
async fn test_blank_models_clear_saved_workspace_models() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;
    let organization = format!("/api/organizations/{org_id}/settings/ai");
    let workspace = format!("/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai");
    let effective = format!("{workspace}/effective");

    client
        .put_json_auth(
            &organization,
            &json!({
                "provider": "claude_code",
                "model_fast": "organization-model",
                "model_reasoning": "organization-model",
                "model_embedding": "organization-model"
            }),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);

    let first = client
        .put_json_auth(
            &workspace,
            &json!({
                "provider": "codex",
                "model_fast": "",
                "model_reasoning": "",
                "model_embedding": ""
            }),
            &token,
        )
        .await;
    first.assert_status(StatusCode::OK);
    let inherited = client.get_auth(&effective, &token).await;
    inherited.assert_status(StatusCode::OK);
    for field in MODEL_FIELDS {
        assert!(
            first.json_value()[field].is_null(),
            "the first save must store a blank {field} as no model"
        );
        assert_eq!(
            inherited.json_value()[field],
            "organization-model",
            "a blank {field} must leave the organization's model in effect"
        );
    }

    for field in MODEL_FIELDS {
        client
            .put_json_auth(
                &workspace,
                &json!({ "provider": "codex", field: "workspace-model" }),
                &token,
            )
            .await
            .assert_status(StatusCode::OK);

        let omitted = client
            .put_json_auth(&workspace, &json!({ "provider": "codex" }), &token)
            .await;
        omitted.assert_status(StatusCode::OK);
        assert_eq!(
            omitted.json_value()[field],
            "workspace-model",
            "leaving {field} out must keep the workspace's model"
        );

        let cleared = client
            .put_json_auth(
                &workspace,
                &json!({ "provider": "codex", field: "" }),
                &token,
            )
            .await;
        cleared.assert_status(StatusCode::OK);
        assert!(
            cleared.json_value()[field].is_null(),
            "a blank {field} must clear the workspace's model"
        );

        let reinherited = client.get_auth(&effective, &token).await;
        reinherited.assert_status(StatusCode::OK);
        assert_eq!(
            reinherited.json_value()[field],
            "organization-model",
            "clearing the workspace's {field} must fall back to the organization's"
        );
    }
}

#[tokio::test]
async fn test_workspace_settings_say_whether_the_workspace_overrides() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;
    let organization = format!("/api/organizations/{org_id}/settings/ai");
    let workspace = format!("/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai");

    client
        .put_json_auth(&organization, &json!({ "provider": "claude_code" }), &token)
        .await
        .assert_status(StatusCode::OK);

    let inherited = client.get_auth(&workspace, &token).await;
    inherited.assert_status(StatusCode::OK);
    assert_eq!(inherited.json_value()["provider"], "self_hosted");
    assert_eq!(inherited.json_value()["overrides"], false);

    let saved = client
        .put_json_auth(&workspace, &json!({ "provider": "self_hosted" }), &token)
        .await;
    saved.assert_status(StatusCode::OK);
    assert_eq!(saved.json_value()["overrides"], true);

    let stored = client.get_auth(&workspace, &token).await;
    stored.assert_status(StatusCode::OK);
    assert_eq!(stored.json_value()["provider"], "self_hosted");
    assert_eq!(
        stored.json_value()["overrides"],
        true,
        "a workspace that saved self_hosted overrides an organization on claude_code"
    );

    let effective = client
        .get_auth(&format!("{workspace}/effective"), &token)
        .await;
    effective.assert_status(StatusCode::OK);
    assert_eq!(effective.json_value()["provider"], "self_hosted");
    assert!(effective.json_value().get("overrides").is_none());
    let own = client.get_auth(&organization, &token).await;
    own.assert_status(StatusCode::OK);
    assert!(own.json_value().get("overrides").is_none());

    client
        .delete_auth(&workspace, &token)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let reset = client.get_auth(&workspace, &token).await;
    reset.assert_status(StatusCode::OK);
    assert_eq!(reset.json_value()["overrides"], false);
}

#[tokio::test]
async fn test_credentials_not_exposed_in_response() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;

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

    let response = client
        .get_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();

    assert_eq!(body["has_openai_api_key"], true);
    assert_eq!(body["has_litellm_key"], true);

    assert!(body.get("openai_api_key").is_none());
    assert!(body.get("litellm_key").is_none());
}
