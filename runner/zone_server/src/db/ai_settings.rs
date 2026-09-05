//! AI provider settings database queries

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;
use zone_context::embeddings::providers::PROVIDER_SELF_HOSTED;

use super::DbResult;

/// Organization AI settings row from database
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OrgAiSettingsRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub provider: String,
    pub litellm_host: Option<String>,
    pub litellm_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub openai_base_url: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub anthropic_base_url: Option<String>,
    pub bedrock_region: Option<String>,
    pub bedrock_access_key: Option<String>,
    pub bedrock_secret_key: Option<String>,
    pub bedrock_use_iam_role: Option<bool>,
    pub model_fast: Option<String>,
    pub model_reasoning: Option<String>,
    pub model_embedding: Option<String>,
    pub model_image: Option<String>,
    pub model_video: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Workspace AI settings row from database
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WorkspaceAiSettingsRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub provider: Option<String>,
    pub litellm_host: Option<String>,
    pub litellm_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub openai_base_url: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub anthropic_base_url: Option<String>,
    pub bedrock_region: Option<String>,
    pub bedrock_access_key: Option<String>,
    pub bedrock_secret_key: Option<String>,
    pub bedrock_use_iam_role: Option<bool>,
    pub model_fast: Option<String>,
    pub model_reasoning: Option<String>,
    pub model_embedding: Option<String>,
    pub model_image: Option<String>,
    pub model_video: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Effective AI settings (merged from org and workspace)
#[derive(Debug, Clone)]
pub struct EffectiveAiSettings {
    pub provider: String,
    pub litellm_host: Option<String>,
    pub litellm_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub openai_base_url: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub anthropic_base_url: Option<String>,
    pub bedrock_region: Option<String>,
    pub bedrock_access_key: Option<String>,
    pub bedrock_secret_key: Option<String>,
    pub bedrock_use_iam_role: bool,
    pub model_fast: Option<String>,
    pub model_reasoning: Option<String>,
    pub model_embedding: Option<String>,
    pub model_image: Option<String>,
    pub model_video: Option<String>,
}

impl EffectiveAiSettings {
    /// Overlay workspace/org image settings onto the process ComfyUI defaults.
    ///
    /// `model_fast` classifies image intent when rules are unsure, including
    /// informal edits of an attached photo. `model_image` is the
    /// ComfyUI checkpoint used for generation; an empty value keeps
    /// `COMFYUI_CHECKPOINT`. `model_video` is the Wan UNET filename and keeps
    /// `COMFYUI_VIDEO_UNET` when empty.
    pub fn apply_to_comfyui(&self, config: &mut crate::config::ComfyUiConfig) {
        if let Some(model) = nonempty(self.model_fast.as_deref()) {
            config.classifier_model = model.to_string();
        }
        if let Some(model) = nonempty(self.model_image.as_deref()) {
            config.checkpoint = model.to_string();
        }
        if let Some(model) = nonempty(self.model_video.as_deref()) {
            config.video_unet = model.to_string();
        }
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

// ============================================================================
// Organization AI Settings
// ============================================================================

/// Get AI settings for an organization
pub async fn get_org_ai_settings(
    pool: &PgPool,
    organization_id: Uuid,
) -> DbResult<Option<OrgAiSettingsRow>> {
    let row: Option<OrgAiSettingsRow> = sqlx::query_as(
        r#"
        SELECT id, organization_id, provider, litellm_host, litellm_key,
               openai_api_key, openai_base_url, anthropic_api_key, anthropic_base_url,
               bedrock_region, bedrock_access_key, bedrock_secret_key, bedrock_use_iam_role,
               model_fast, model_reasoning, model_embedding, model_image, model_video, created_at, updated_at
        FROM organization_ai_settings
        WHERE organization_id = $1
        "#,
    )
    .bind(organization_id)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

/// Upsert (create or update) AI settings for an organization
pub async fn upsert_org_ai_settings(
    pool: &PgPool,
    organization_id: Uuid,
    provider: Option<&str>,
    litellm_host: Option<&str>,
    litellm_key: Option<&str>,
    openai_api_key: Option<&str>,
    openai_base_url: Option<&str>,
    anthropic_api_key: Option<&str>,
    anthropic_base_url: Option<&str>,
    bedrock_region: Option<&str>,
    bedrock_access_key: Option<&str>,
    bedrock_secret_key: Option<&str>,
    bedrock_use_iam_role: Option<bool>,
    model_fast: Option<&str>,
    model_reasoning: Option<&str>,
    model_embedding: Option<&str>,
    model_image: Option<&str>,
    model_video: Option<&str>,
) -> DbResult<OrgAiSettingsRow> {
    let row: OrgAiSettingsRow = sqlx::query_as(
        r#"
        INSERT INTO organization_ai_settings (
            organization_id, provider, litellm_host, litellm_key,
            openai_api_key, openai_base_url, anthropic_api_key, anthropic_base_url,
            bedrock_region, bedrock_access_key, bedrock_secret_key, bedrock_use_iam_role,
            model_fast, model_reasoning, model_embedding, model_image, model_video
        ) VALUES (
            $1, COALESCE($2, 'self_hosted'), $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15,
            NULLIF(BTRIM($16), ''), NULLIF(BTRIM($17), '')
        )
        ON CONFLICT (organization_id) DO UPDATE SET
            provider = COALESCE($2, organization_ai_settings.provider),
            litellm_host = COALESCE($3, organization_ai_settings.litellm_host),
            litellm_key = COALESCE($4, organization_ai_settings.litellm_key),
            openai_api_key = COALESCE($5, organization_ai_settings.openai_api_key),
            openai_base_url = COALESCE($6, organization_ai_settings.openai_base_url),
            anthropic_api_key = COALESCE($7, organization_ai_settings.anthropic_api_key),
            anthropic_base_url = COALESCE($8, organization_ai_settings.anthropic_base_url),
            bedrock_region = COALESCE($9, organization_ai_settings.bedrock_region),
            bedrock_access_key = COALESCE($10, organization_ai_settings.bedrock_access_key),
            bedrock_secret_key = COALESCE($11, organization_ai_settings.bedrock_secret_key),
            bedrock_use_iam_role = COALESCE($12, organization_ai_settings.bedrock_use_iam_role),
            model_fast = COALESCE($13, organization_ai_settings.model_fast),
            model_reasoning = COALESCE($14, organization_ai_settings.model_reasoning),
            model_embedding = COALESCE($15, organization_ai_settings.model_embedding),
            -- NULL keeps the previous filename; empty string clears to NULL (server default).
            model_image = CASE
                WHEN $16 IS NULL THEN organization_ai_settings.model_image
                WHEN BTRIM($16) = '' THEN NULL
                ELSE $16
            END,
            model_video = CASE
                WHEN $17 IS NULL THEN organization_ai_settings.model_video
                WHEN BTRIM($17) = '' THEN NULL
                ELSE $17
            END,
            updated_at = NOW()
        RETURNING id, organization_id, provider, litellm_host, litellm_key,
                  openai_api_key, openai_base_url, anthropic_api_key, anthropic_base_url,
                  bedrock_region, bedrock_access_key, bedrock_secret_key, bedrock_use_iam_role,
                  model_fast, model_reasoning, model_embedding, model_image, model_video, created_at, updated_at
        "#
    )
    .bind(organization_id)
    .bind(provider)
    .bind(litellm_host)
    .bind(litellm_key)
    .bind(openai_api_key)
    .bind(openai_base_url)
    .bind(anthropic_api_key)
    .bind(anthropic_base_url)
    .bind(bedrock_region)
    .bind(bedrock_access_key)
    .bind(bedrock_secret_key)
    .bind(bedrock_use_iam_role)
    .bind(model_fast)
    .bind(model_reasoning)
    .bind(model_embedding)
    .bind(model_image)
    .bind(model_video)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Delete AI settings for an organization
pub async fn delete_org_ai_settings(pool: &PgPool, organization_id: Uuid) -> DbResult<bool> {
    let result = sqlx::query("DELETE FROM organization_ai_settings WHERE organization_id = $1")
        .bind(organization_id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

// ============================================================================
// Workspace AI Settings
// ============================================================================

/// Get AI settings for a workspace
pub async fn get_workspace_ai_settings(
    pool: &PgPool,
    workspace_id: Uuid,
) -> DbResult<Option<WorkspaceAiSettingsRow>> {
    let row: Option<WorkspaceAiSettingsRow> = sqlx::query_as(
        r#"
        SELECT id, workspace_id, provider, litellm_host, litellm_key,
               openai_api_key, openai_base_url, anthropic_api_key, anthropic_base_url,
               bedrock_region, bedrock_access_key, bedrock_secret_key, bedrock_use_iam_role,
               model_fast, model_reasoning, model_embedding, model_image, model_video, created_at, updated_at
        FROM workspace_ai_settings
        WHERE workspace_id = $1
        "#,
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

/// Upsert (create or update) AI settings for a workspace
pub async fn upsert_workspace_ai_settings(
    pool: &PgPool,
    workspace_id: Uuid,
    provider: Option<&str>,
    litellm_host: Option<&str>,
    litellm_key: Option<&str>,
    openai_api_key: Option<&str>,
    openai_base_url: Option<&str>,
    anthropic_api_key: Option<&str>,
    anthropic_base_url: Option<&str>,
    bedrock_region: Option<&str>,
    bedrock_access_key: Option<&str>,
    bedrock_secret_key: Option<&str>,
    bedrock_use_iam_role: Option<bool>,
    model_fast: Option<&str>,
    model_reasoning: Option<&str>,
    model_embedding: Option<&str>,
    model_image: Option<&str>,
    model_video: Option<&str>,
) -> DbResult<WorkspaceAiSettingsRow> {
    let row: WorkspaceAiSettingsRow = sqlx::query_as(
        r#"
        INSERT INTO workspace_ai_settings (
            workspace_id, provider, litellm_host, litellm_key,
            openai_api_key, openai_base_url, anthropic_api_key, anthropic_base_url,
            bedrock_region, bedrock_access_key, bedrock_secret_key, bedrock_use_iam_role,
            model_fast, model_reasoning, model_embedding, model_image, model_video
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15,
            NULLIF(BTRIM($16), ''), NULLIF(BTRIM($17), '')
        )
        ON CONFLICT (workspace_id) DO UPDATE SET
            provider = $2,
            litellm_host = COALESCE($3, workspace_ai_settings.litellm_host),
            litellm_key = COALESCE($4, workspace_ai_settings.litellm_key),
            openai_api_key = COALESCE($5, workspace_ai_settings.openai_api_key),
            openai_base_url = COALESCE($6, workspace_ai_settings.openai_base_url),
            anthropic_api_key = COALESCE($7, workspace_ai_settings.anthropic_api_key),
            anthropic_base_url = COALESCE($8, workspace_ai_settings.anthropic_base_url),
            bedrock_region = COALESCE($9, workspace_ai_settings.bedrock_region),
            bedrock_access_key = COALESCE($10, workspace_ai_settings.bedrock_access_key),
            bedrock_secret_key = COALESCE($11, workspace_ai_settings.bedrock_secret_key),
            bedrock_use_iam_role = COALESCE($12, workspace_ai_settings.bedrock_use_iam_role),
            model_fast = COALESCE($13, workspace_ai_settings.model_fast),
            model_reasoning = COALESCE($14, workspace_ai_settings.model_reasoning),
            model_embedding = COALESCE($15, workspace_ai_settings.model_embedding),
            -- NULL keeps the previous filename; empty string clears to NULL (inherit).
            model_image = CASE
                WHEN $16 IS NULL THEN workspace_ai_settings.model_image
                WHEN BTRIM($16) = '' THEN NULL
                ELSE $16
            END,
            model_video = CASE
                WHEN $17 IS NULL THEN workspace_ai_settings.model_video
                WHEN BTRIM($17) = '' THEN NULL
                ELSE $17
            END,
            updated_at = NOW()
        RETURNING id, workspace_id, provider, litellm_host, litellm_key,
                  openai_api_key, openai_base_url, anthropic_api_key, anthropic_base_url,
                  bedrock_region, bedrock_access_key, bedrock_secret_key, bedrock_use_iam_role,
                  model_fast, model_reasoning, model_embedding, model_image, model_video, created_at, updated_at
        "#,
    )
    .bind(workspace_id)
    .bind(provider)
    .bind(litellm_host)
    .bind(litellm_key)
    .bind(openai_api_key)
    .bind(openai_base_url)
    .bind(anthropic_api_key)
    .bind(anthropic_base_url)
    .bind(bedrock_region)
    .bind(bedrock_access_key)
    .bind(bedrock_secret_key)
    .bind(bedrock_use_iam_role)
    .bind(model_fast)
    .bind(model_reasoning)
    .bind(model_embedding)
    .bind(model_image)
    .bind(model_video)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Delete AI settings for a workspace
pub async fn delete_workspace_ai_settings(pool: &PgPool, workspace_id: Uuid) -> DbResult<bool> {
    let result = sqlx::query("DELETE FROM workspace_ai_settings WHERE workspace_id = $1")
        .bind(workspace_id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

// ============================================================================
// Effective Settings (Merged)
// ============================================================================

/// Get effective AI settings for a workspace (workspace overrides org)
pub async fn get_effective_ai_settings(
    pool: &PgPool,
    organization_id: Uuid,
    workspace_id: Uuid,
) -> DbResult<EffectiveAiSettings> {
    // Get org settings first (defaults)
    let org = get_org_ai_settings(pool, organization_id).await?;
    let ws = get_workspace_ai_settings(pool, workspace_id).await?;

    // Start with defaults
    let mut effective = EffectiveAiSettings {
        provider: PROVIDER_SELF_HOSTED.to_string(),
        litellm_host: None,
        litellm_key: None,
        openai_api_key: None,
        openai_base_url: None,
        anthropic_api_key: None,
        anthropic_base_url: None,
        bedrock_region: None,
        bedrock_access_key: None,
        bedrock_secret_key: None,
        bedrock_use_iam_role: false,
        model_fast: None,
        model_reasoning: None,
        model_embedding: None,
        model_image: None,
        model_video: None,
    };

    // Apply org settings
    if let Some(org) = org {
        effective.provider = org.provider;
        effective.litellm_host = org.litellm_host;
        effective.litellm_key = org.litellm_key;
        effective.openai_api_key = org.openai_api_key;
        effective.openai_base_url = org.openai_base_url;
        effective.anthropic_api_key = org.anthropic_api_key;
        effective.anthropic_base_url = org.anthropic_base_url;
        effective.bedrock_region = org.bedrock_region;
        effective.bedrock_access_key = org.bedrock_access_key;
        effective.bedrock_secret_key = org.bedrock_secret_key;
        effective.bedrock_use_iam_role = org.bedrock_use_iam_role.unwrap_or(false);
        effective.model_fast = org.model_fast;
        effective.model_reasoning = org.model_reasoning;
        effective.model_embedding = org.model_embedding;
        effective.model_image = org.model_image;
        effective.model_video = org.model_video;
    }

    // Override with workspace settings (only non-None values)
    if let Some(ws) = ws {
        if let Some(provider) = ws.provider {
            effective.provider = provider;
        }
        if ws.litellm_host.is_some() {
            effective.litellm_host = ws.litellm_host;
        }
        if ws.litellm_key.is_some() {
            effective.litellm_key = ws.litellm_key;
        }
        if ws.openai_api_key.is_some() {
            effective.openai_api_key = ws.openai_api_key;
        }
        if ws.openai_base_url.is_some() {
            effective.openai_base_url = ws.openai_base_url;
        }
        if ws.anthropic_api_key.is_some() {
            effective.anthropic_api_key = ws.anthropic_api_key;
        }
        if ws.anthropic_base_url.is_some() {
            effective.anthropic_base_url = ws.anthropic_base_url;
        }
        if ws.bedrock_region.is_some() {
            effective.bedrock_region = ws.bedrock_region;
        }
        if ws.bedrock_access_key.is_some() {
            effective.bedrock_access_key = ws.bedrock_access_key;
        }
        if ws.bedrock_secret_key.is_some() {
            effective.bedrock_secret_key = ws.bedrock_secret_key;
        }
        if let Some(use_iam) = ws.bedrock_use_iam_role {
            effective.bedrock_use_iam_role = use_iam;
        }
        if ws.model_fast.is_some() {
            effective.model_fast = ws.model_fast;
        }
        if ws.model_reasoning.is_some() {
            effective.model_reasoning = ws.model_reasoning;
        }
        if ws.model_embedding.is_some() {
            effective.model_embedding = ws.model_embedding;
        }
        if ws.model_image.is_some() {
            effective.model_image = ws.model_image;
        }
        if ws.model_video.is_some() {
            effective.model_video = ws.model_video;
        }
    }

    Ok(effective)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ComfyUiConfig;

    fn settings(fast: Option<&str>, image: Option<&str>) -> EffectiveAiSettings {
        EffectiveAiSettings {
            provider: PROVIDER_SELF_HOSTED.to_string(),
            litellm_host: None,
            litellm_key: None,
            openai_api_key: None,
            openai_base_url: None,
            anthropic_api_key: None,
            anthropic_base_url: None,
            bedrock_region: None,
            bedrock_access_key: None,
            bedrock_secret_key: None,
            bedrock_use_iam_role: false,
            model_fast: fast.map(str::to_string),
            model_reasoning: None,
            model_embedding: None,
            model_image: image.map(str::to_string),
            model_video: None,
        }
    }

    #[test]
    fn apply_to_comfyui_uses_configured_image_model() {
        let mut config = ComfyUiConfig::default();
        settings(Some("llama3.2:3b"), Some("custom-image.safetensors"))
            .apply_to_comfyui(&mut config);
        assert_eq!(config.classifier_model, "llama3.2:3b");
        assert_eq!(config.checkpoint, "custom-image.safetensors");
        assert_eq!(config.video_unet, "wan2.2_ti2v_5B_fp16.safetensors");
    }

    #[test]
    fn apply_to_comfyui_uses_configured_video_model() {
        let mut config = ComfyUiConfig::default();
        let mut settings = settings(None, None);
        settings.model_video = Some("custom-video.safetensors".to_string());
        settings.apply_to_comfyui(&mut config);
        assert_eq!(config.video_unet, "custom-video.safetensors");
    }

    #[test]
    fn apply_to_comfyui_keeps_env_checkpoint_when_image_model_unset() {
        let mut config = ComfyUiConfig {
            checkpoint: "flux1-schnell-fp8.safetensors".to_string(),
            classifier_model: "llama3.1:8b".to_string(),
            ..ComfyUiConfig::default()
        };
        settings(None, Some("   ")).apply_to_comfyui(&mut config);
        assert_eq!(config.checkpoint, "flux1-schnell-fp8.safetensors");
        assert_eq!(config.classifier_model, "llama3.1:8b");
    }
}
