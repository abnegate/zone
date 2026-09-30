//! AI Settings integration tests

mod common;

use axum::http::StatusCode;
use serde_json::json;
use uuid::Uuid;
use zone_core::OptionalSecretExt;
use zone_server::db::ai_settings;

use common::{TestClient, test_email, test_password};

const AGENT_PROVIDERS: [&str; 2] = ["claude_code", "codex"];
const MODEL_FIELDS: [&str; 3] = ["model_fast", "model_reasoning", "model_embedding"];
const COMFYUI_MODELS: [(&str, &str); 3] = [
    ("model_image", "organization-image.safetensors"),
    ("model_video", "organization-video.safetensors"),
    ("model_audio", "organization-audio.safetensors"),
];
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
    assert!(
        body["model_reasoning"].is_null(),
        "an anthropic workspace must not inherit the openai organization's reasoning model, got {}",
        body["model_reasoning"]
    );
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
                "provider": "codex",
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

fn organization_models(provider: &str, models: [&str; 3]) -> serde_json::Value {
    let mut body = json!({ "provider": provider });
    for (field, model) in MODEL_FIELDS.into_iter().zip(models) {
        body[field] = json!(model);
    }
    for (field, model) in COMFYUI_MODELS {
        body[field] = json!(model);
    }
    body
}

fn blank_models(provider: &str) -> serde_json::Value {
    let mut body = json!({ "provider": provider });
    for field in MODEL_FIELDS {
        body[field] = json!("");
    }
    body
}

#[tokio::test]
async fn test_a_workspace_on_another_provider_inherits_none_of_its_organizations_models() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;
    let workspace_id = uuid::Uuid::parse_str(&ws_id).unwrap();
    let workspace = format!("/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai");
    let effective = format!("{workspace}/effective");

    client
        .put_json_auth(
            &format!("/api/organizations/{org_id}/settings/ai"),
            &organization_models("openai", ["gpt-4o", "o1-preview", "text-embedding-3-small"]),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);

    for provider in [
        "codex",
        "claude_code",
        "anthropic",
        "bedrock",
        "self_hosted",
    ] {
        client
            .put_json_auth(&workspace, &blank_models(provider), &token)
            .await
            .assert_status(StatusCode::OK);

        let running = ai_settings::for_workspace(client.state().db(), workspace_id)
            .await
            .expect("the workspace's settings to be readable");
        assert_eq!(running.provider, provider);
        assert_eq!(
            [
                running.model_fast,
                running.model_reasoning,
                running.model_embedding
            ],
            [None, None, None],
            "a {provider} workspace must run without the openai organization's models"
        );

        let response = client.get_auth(&effective, &token).await;
        response.assert_status(StatusCode::OK);
        let body = response.json_value();
        assert_eq!(body["provider"], provider);
        for field in MODEL_FIELDS {
            assert!(
                body[field].is_null(),
                "a {provider} workspace must not inherit the openai organization's {field}, got {}",
                body[field]
            );
        }
        for (field, model) in COMFYUI_MODELS {
            assert_eq!(
                body[field], model,
                "a {provider} workspace still inherits the organization's {field}"
            );
        }
    }
}

#[tokio::test]
async fn test_a_workspace_on_its_organizations_provider_inherits_its_models() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;
    let workspace = format!("/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai");
    let models = ["gpt-5.5", "gpt-6-sol", "organization-embedding"];

    client
        .put_json_auth(
            &format!("/api/organizations/{org_id}/settings/ai"),
            &organization_models("codex", models),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);
    client
        .put_json_auth(&workspace, &blank_models("codex"), &token)
        .await
        .assert_status(StatusCode::OK);

    let response = client
        .get_auth(&format!("{workspace}/effective"), &token)
        .await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "codex");
    for (field, model) in MODEL_FIELDS.into_iter().zip(models) {
        assert_eq!(
            body[field], model,
            "a codex workspace in a codex organization inherits its {field}"
        );
    }
    for (field, model) in COMFYUI_MODELS {
        assert_eq!(body[field], model, "the workspace inherits the {field}");
    }
}

#[tokio::test]
async fn test_a_self_hosted_workspace_under_a_self_hosted_organization_is_unchanged() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;
    let workspace = format!("/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai");
    let effective = format!("{workspace}/effective");
    let models = ["llama3.1:8b", "deepseek-r1:7b", "nomic-embed-text"];
    let mut organization = organization_models("self_hosted", models);
    organization["litellm_host"] = json!("http://litellm:4000");
    organization["litellm_key"] = json!("sk-organization-litellm");

    client
        .put_json_auth(
            &format!("/api/organizations/{org_id}/settings/ai"),
            &organization,
            &token,
        )
        .await
        .assert_status(StatusCode::OK);
    let inherited = client.get_auth(&effective, &token).await;
    inherited.assert_status(StatusCode::OK);

    client
        .put_json_auth(&workspace, &blank_models("self_hosted"), &token)
        .await
        .assert_status(StatusCode::OK);
    let overridden = client.get_auth(&effective, &token).await;
    overridden.assert_status(StatusCode::OK);

    assert_eq!(
        overridden.json_value(),
        inherited.json_value(),
        "a self_hosted workspace with blank models runs exactly as its self_hosted organization"
    );
    let body = overridden.json_value();
    assert_eq!(body["provider"], "self_hosted");
    assert_eq!(body["litellm_host"], "http://litellm:4000");
    assert_eq!(body["has_litellm_key"], true);
    for (field, model) in MODEL_FIELDS.into_iter().zip(models) {
        assert_eq!(
            body[field], model,
            "the workspace inherits the organization's {field}"
        );
    }
    for (field, model) in COMFYUI_MODELS {
        assert_eq!(body[field], model, "the workspace inherits the {field}");
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

const ENDPOINT_PAIRS: [(&str, &str, &str); 3] = [
    ("litellm_host", "litellm_key", "has_litellm_key"),
    ("openai_base_url", "openai_api_key", "has_openai_api_key"),
    (
        "anthropic_base_url",
        "anthropic_api_key",
        "has_anthropic_api_key",
    ),
];

fn organization_endpoints() -> serde_json::Value {
    let mut body = json!({ "provider": "self_hosted" });
    for (url, key, _) in ENDPOINT_PAIRS {
        body[url] = json!(format!("http://organization-{url}.example:4000"));
        body[key] = json!(format!("sk-organization-{key}"));
    }
    body
}

#[tokio::test]
async fn test_ai_settings_refuse_a_base_url_that_is_not_http() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;
    let paths = [
        format!("/api/organizations/{org_id}/settings/ai"),
        format!("/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai"),
    ];

    for path in &paths {
        for (field, _, _) in ENDPOINT_PAIRS {
            for url in [
                "file:///etc/passwd",
                "http://user:secret@litellm:4000",
                "http://litellm:4000/?key=x",
            ] {
                let response = client
                    .put_json_auth(path, &json!({ field: url }), &token)
                    .await;
                assert_eq!(
                    response.status,
                    StatusCode::BAD_REQUEST,
                    "{path} saved {field} = {url}: {}",
                    response.text()
                );
                assert!(
                    !response.text().contains("secret"),
                    "the refusal echoed the URL's credential: {}",
                    response.text()
                );
            }
        }
    }

    for path in &paths {
        let saved = client.get_auth(path, &token).await;
        saved.assert_status(StatusCode::OK);
        for (field, _, _) in ENDPOINT_PAIRS {
            assert!(
                saved.json_value()[field].is_null(),
                "{path} stored a refused {field}"
            );
        }
    }
}

#[tokio::test]
async fn test_ai_settings_accept_private_and_loopback_hosts() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;
    let paths = [
        format!("/api/organizations/{org_id}/settings/ai"),
        format!("/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai"),
    ];

    for path in &paths {
        for (field, _, _) in ENDPOINT_PAIRS {
            for url in [
                "http://127.0.0.1:4000",
                "http://192.168.1.10:4000",
                "http://litellm:4000",
                "",
            ] {
                let response = client
                    .put_json_auth(path, &json!({ field: url }), &token)
                    .await;
                assert_eq!(
                    response.status,
                    StatusCode::OK,
                    "{path} refused {field} = {url:?}: {}",
                    response.text()
                );
                let saved = if url.is_empty() { json!(null) } else { json!(url) };
                assert_eq!(response.json_value()[field], saved, "{path} {field}");
            }
        }
    }
}

#[tokio::test]
async fn test_a_workspace_base_url_does_not_inherit_the_organization_key() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;
    let workspace = format!("/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai");

    client
        .put_json_auth(
            &format!("/api/organizations/{org_id}/settings/ai"),
            &organization_endpoints(),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);
    let mut repointed = json!({});
    for (url, _, _) in ENDPOINT_PAIRS {
        repointed[url] = json!(format!("http://workspace-{url}.example:4000"));
    }
    client
        .put_json_auth(&workspace, &repointed, &token)
        .await
        .assert_status(StatusCode::OK);

    let effective = client
        .get_auth(&format!("{workspace}/effective"), &token)
        .await;
    effective.assert_status(StatusCode::OK);
    let body = effective.json_value();
    for (url, _, has_key) in ENDPOINT_PAIRS {
        assert_eq!(
            body[url],
            format!("http://workspace-{url}.example:4000"),
            "the workspace's {url} wins"
        );
        assert_eq!(
            body[has_key], false,
            "the organization's key followed the workspace's {url}"
        );
    }
}

#[tokio::test]
async fn test_a_workspace_key_alone_keeps_the_organization_host() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;
    let workspace = format!("/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai");

    client
        .put_json_auth(
            &format!("/api/organizations/{org_id}/settings/ai"),
            &organization_endpoints(),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);
    let mut keys = json!({});
    for (_, key, _) in ENDPOINT_PAIRS {
        keys[key] = json!(format!("sk-workspace-{key}"));
    }
    client
        .put_json_auth(&workspace, &keys, &token)
        .await
        .assert_status(StatusCode::OK);

    let effective = client
        .get_auth(&format!("{workspace}/effective"), &token)
        .await;
    effective.assert_status(StatusCode::OK);
    let body = effective.json_value();
    for (url, _, has_key) in ENDPOINT_PAIRS {
        assert_eq!(
            body[url],
            format!("http://organization-{url}.example:4000"),
            "a workspace key alone keeps the organization's {url}"
        );
        assert_eq!(body[has_key], true);
    }

    let settings = ai_settings::get_effective_ai_settings(
        client.state().db(),
        org_id.parse().expect("organization id"),
        ws_id.parse().expect("workspace id"),
    )
    .await
    .expect("effective settings");
    assert_eq!(
        settings.litellm_key.expose_as_deref(),
        Some("sk-workspace-litellm_key")
    );
    assert_eq!(
        settings.openai_api_key.expose_as_deref(),
        Some("sk-workspace-openai_api_key")
    );
    assert_eq!(
        settings.anthropic_api_key.expose_as_deref(),
        Some("sk-workspace-anthropic_api_key")
    );
}

async fn save_unrouted(client: &TestClient, organization: Uuid, workspace: Uuid) {
    let pool = client.state().db();
    sqlx::query(
        "INSERT INTO organization_ai_settings (organization_id, provider, litellm_host, litellm_key) \
         VALUES ($1, 'self_hosted', 'http://localhost:11434', 'sk-organization-litellm')",
    )
    .bind(organization)
    .execute(pool)
    .await
    .expect("an organization row saved before completions were routed");
    sqlx::query(
        "INSERT INTO workspace_ai_settings (workspace_id, provider, openai_base_url) \
         VALUES ($1, 'openai', 'http://workspace-openai.example:4000')",
    )
    .bind(workspace)
    .execute(pool)
    .await
    .expect("a workspace row saved before completions were routed");
}

async fn effective_settings(
    client: &TestClient,
    organization: Uuid,
    workspace: Uuid,
) -> ai_settings::EffectiveAiSettings {
    ai_settings::get_effective_ai_settings(client.state().db(), organization, workspace)
        .await
        .expect("effective settings")
}

#[tokio::test]
async fn test_a_row_saved_before_completions_were_routed_lends_no_endpoint_until_saved() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;
    let organization: Uuid = org_id.parse().expect("organization id");
    let workspace: Uuid = ws_id.parse().expect("workspace id");
    let paths = [
        format!("/api/organizations/{org_id}/settings/ai"),
        format!("/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai"),
    ];
    save_unrouted(&client, organization, workspace).await;

    for path in &paths {
        let saved = client.get_auth(path, &token).await;
        saved.assert_status(StatusCode::OK);
        assert_eq!(
            saved.json_value()["completions_routed"],
            false,
            "{path} was saved before completions were routed"
        );
    }
    let before = effective_settings(&client, organization, workspace).await;
    assert_eq!(before.provider, "openai");
    assert_eq!(before.litellm_host, None);
    assert_eq!(before.litellm_key.expose_as_deref(), None);
    assert_eq!(before.openai_base_url, None);

    for path in &paths {
        let saved = client
            .put_json_auth(path, &json!({ "model_fast": "llama3.2:3b" }), &token)
            .await;
        saved.assert_status(StatusCode::OK);
        assert_eq!(
            saved.json_value()["completions_routed"],
            true,
            "saving {path} routes its completions"
        );
        let read = client.get_auth(path, &token).await;
        assert_eq!(read.json_value()["completions_routed"], true, "{path}");
    }
    let after = effective_settings(&client, organization, workspace).await;
    assert_eq!(
        after.litellm_host.as_deref(),
        Some("http://localhost:11434")
    );
    assert_eq!(
        after.litellm_key.expose_as_deref(),
        Some("sk-organization-litellm")
    );
    assert_eq!(
        after.openai_base_url.as_deref(),
        Some("http://workspace-openai.example:4000")
    );
    assert_eq!(after.openai_api_key.expose_as_deref(), None);
}

#[tokio::test]
async fn test_workspace_settings_say_which_keys_the_organization_saved() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;
    let workspace = format!("/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai");

    let none = client.get_auth(&workspace, &token).await;
    none.assert_status(StatusCode::OK);
    assert_eq!(
        none.json_value()["organization_keys"],
        json!({ "litellm": false, "openai": false, "anthropic": false })
    );

    client
        .put_json_auth(
            &format!("/api/organizations/{org_id}/settings/ai"),
            &json!({ "openai_api_key": "sk-organization-openai" }),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);

    let expected = json!({ "litellm": false, "openai": true, "anthropic": false });
    let read = client.get_auth(&workspace, &token).await;
    read.assert_status(StatusCode::OK);
    assert_eq!(read.json_value()["organization_keys"], expected);
    let saved = client
        .put_json_auth(
            &workspace,
            &json!({ "provider": "openai", "openai_base_url": "http://workspace.example:4000" }),
            &token,
        )
        .await;
    saved.assert_status(StatusCode::OK);
    assert_eq!(saved.json_value()["organization_keys"], expected);
}

#[tokio::test]
async fn test_an_empty_endpoint_value_clears_the_saved_one_and_an_absent_one_keeps_it() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;

    for path in [
        format!("/api/organizations/{org_id}/settings/ai"),
        format!("/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai"),
    ] {
        client
            .put_json_auth(&path, &organization_endpoints(), &token)
            .await
            .assert_status(StatusCode::OK);

        let kept = client
            .put_json_auth(&path, &json!({ "provider": "self_hosted" }), &token)
            .await;
        kept.assert_status(StatusCode::OK);
        let body = kept.json_value();
        for (url, _, has_key) in ENDPOINT_PAIRS {
            assert_eq!(
                body[url],
                format!("http://organization-{url}.example:4000"),
                "{path} dropped {url} it was not sent"
            );
            assert_eq!(body[has_key], true, "{path} dropped a key it was not sent");
        }

        let mut blanks = json!({ "provider": "self_hosted" });
        for (url, key, _) in ENDPOINT_PAIRS {
            blanks[url] = json!("");
            blanks[key] = json!("  ");
        }
        client
            .put_json_auth(&path, &blanks, &token)
            .await
            .assert_status(StatusCode::OK);

        let cleared = client.get_auth(&path, &token).await;
        cleared.assert_status(StatusCode::OK);
        let body = cleared.json_value();
        for (url, _, has_key) in ENDPOINT_PAIRS {
            assert_eq!(body[url], serde_json::Value::Null, "{path} kept {url}");
            assert_eq!(body[has_key], false, "{path} kept the key beside {url}");
        }
    }
}

#[tokio::test]
async fn test_a_legacy_url_carrying_credentials_is_returned_without_them() {
    const SECRET: &str = "legacy-secret";
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let org_id = create_org(&client, &token).await;
    let ws_id = create_workspace(&client, &token, &org_id).await;
    let organization: Uuid = org_id.parse().expect("organization id");
    let workspace: Uuid = ws_id.parse().expect("workspace id");
    let pool = client.state().db();
    sqlx::query(
        "INSERT INTO organization_ai_settings \
         (organization_id, provider, litellm_host, openai_base_url, anthropic_base_url, completions_routed) \
         VALUES ($1, 'self_hosted', $2, $3, $4, true)",
    )
    .bind(organization)
    .bind(format!("http://admin:{SECRET}@gateway.example:4000"))
    .bind(format!("https://proxy.example/v1?api_key={SECRET}"))
    .bind(format!("https://{SECRET}@proxy.example/v1#{SECRET}"))
    .execute(pool)
    .await
    .expect("an organization row saved before URLs were checked");
    sqlx::query("INSERT INTO workspace_ai_settings (workspace_id, litellm_host) VALUES ($1, $2)")
        .bind(workspace)
        .bind(format!("http://admin:{SECRET}@workspace.example:4000/"))
        .execute(pool)
        .await
        .expect("a workspace row saved before URLs were checked");

    let organization_path = format!("/api/organizations/{org_id}/settings/ai");
    let workspace_path = format!("/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai");
    let effective_path = format!("{workspace_path}/effective");
    for path in [&organization_path, &workspace_path, &effective_path] {
        let response = client.get_auth(path, &token).await;
        response.assert_status(StatusCode::OK);
        assert!(
            !response.text().contains(SECRET),
            "{path} returned a credential saved in a URL: {}",
            response.text()
        );
    }
    let organization_body = client
        .get_auth(&organization_path, &token)
        .await
        .json_value();
    assert_eq!(
        organization_body["litellm_host"],
        "http://gateway.example:4000"
    );
    assert_eq!(
        organization_body["openai_base_url"],
        "https://proxy.example/v1"
    );
    assert_eq!(
        organization_body["anthropic_base_url"],
        "https://proxy.example/v1"
    );
    assert_eq!(
        client.get_auth(&workspace_path, &token).await.json_value()["litellm_host"],
        "http://workspace.example:4000/"
    );
}
