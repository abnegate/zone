//! AI provider settings database queries

use chrono::NaiveDateTime;
use sqlx::{Executor, PgConnection, PgPool, Postgres};
use uuid::Uuid;
use zone_context::embeddings::providers::{
    PROVIDER_BEDROCK, PROVIDER_OPENAI, PROVIDER_SELF_HOSTED,
};
use zone_core::SecretValue;
use zone_core::llm::AgentKind;

use super::{
    DbResult,
    organization_members::{self, OrgRole},
    workspace_members::{self, WorkspaceRole},
};

const PROVIDER_ANTHROPIC: &str = "anthropic";
pub const PROVIDER_CLAUDE_CODE: &str = "claude_code";
pub const PROVIDER_CODEX: &str = "codex";

const PROVIDERS: [&str; 6] = [
    PROVIDER_SELF_HOSTED,
    PROVIDER_OPENAI,
    PROVIDER_ANTHROPIC,
    PROVIDER_BEDROCK,
    PROVIDER_CLAUDE_CODE,
    PROVIDER_CODEX,
];

pub fn agent(provider: &str) -> Option<AgentKind> {
    match provider {
        PROVIDER_CLAUDE_CODE => Some(AgentKind::Claude),
        PROVIDER_CODEX => Some(AgentKind::Codex),
        _ => None,
    }
}

pub fn provider(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => PROVIDER_CLAUDE_CODE,
        AgentKind::Codex => PROVIDER_CODEX,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AccessError {
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("{0}")]
    NotFound(&'static str),
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

type AccessResult<T> = Result<T, AccessError>;

#[derive(Default)]
pub struct Update<'a> {
    pub provider: Option<&'a str>,
    pub litellm_host: Option<&'a str>,
    pub litellm_key: Option<&'a str>,
    pub openai_api_key: Option<&'a str>,
    pub openai_base_url: Option<&'a str>,
    pub anthropic_api_key: Option<&'a str>,
    pub anthropic_base_url: Option<&'a str>,
    pub bedrock_region: Option<&'a str>,
    pub bedrock_access_key: Option<&'a str>,
    pub bedrock_secret_key: Option<&'a str>,
    pub bedrock_use_iam_role: Option<bool>,
    pub model_fast: Option<&'a str>,
    pub model_reasoning: Option<&'a str>,
    pub model_embedding: Option<&'a str>,
    pub model_image: Option<&'a str>,
    pub model_video: Option<&'a str>,
    pub model_audio: Option<&'a str>,
}

fn validate(update: &Update<'_>) -> AccessResult<()> {
    match update.provider {
        Some(provider) if !PROVIDERS.contains(&provider) => Err(AccessError::Invalid(format!(
            "Invalid provider. Must be one of: {}",
            PROVIDERS.join(", ")
        ))),
        _ => Ok(()),
    }
}

/// The organization's own settings carry its provider credentials, so reading
/// them takes an administrator, not just a seat; `minimum` says which.
pub(crate) async fn authorize_organization(
    connection: &mut PgConnection,
    organization_id: Uuid,
    user_id: Uuid,
    minimum: OrgRole,
) -> AccessResult<()> {
    let role = organization_members::lock_role(connection, organization_id, user_id).await?;

    match role {
        Some(role) if role >= minimum => Ok(()),
        Some(_) => Err(AccessError::Forbidden(
            "Only organization admins can manage AI settings",
        )),
        None => Err(AccessError::NotFound("Organization not found")),
    }
}

async fn authorize_workspace(
    connection: &mut PgConnection,
    organization_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    write: bool,
) -> AccessResult<()> {
    // Lock the owning relationship before either membership check so a nested
    // path can never combine authorization from two different tenants.
    let workspace: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM workspaces WHERE id = $1 AND organization_id = $2 FOR SHARE",
    )
    .bind(workspace_id)
    .bind(organization_id)
    .fetch_optional(&mut *connection)
    .await?;
    if workspace.is_none() {
        return Err(AccessError::NotFound("Workspace not found"));
    }

    // Membership proves the caller belongs to the tenant that owns the
    // workspace; the workspace role below decides whether they may write. The
    // organization admin rule guards organization-wide settings only.
    authorize_organization(&mut *connection, organization_id, user_id, OrgRole::Member).await?;

    let role = workspace_members::lock_role(connection, workspace_id, user_id).await?;

    // These settings override the organization's, and a base URL written here
    // is paired with whatever credential the organization set, so writing them
    // takes the workspace's own administrators -- the rank the organization
    // route demands for the settings this one overrides. Every member still
    // reads them, because their chats run under them.
    match role {
        Some(WorkspaceRole::Owner | WorkspaceRole::Admin) => Ok(()),
        Some(_) if !write => Ok(()),
        Some(_) => Err(AccessError::Forbidden(
            "Only workspace admins can change AI settings",
        )),
        None => Err(AccessError::NotFound("Workspace not found")),
    }
}

/// Organization AI settings row from database
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OrgAiSettingsRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub provider: String,
    pub litellm_host: Option<String>,
    pub litellm_key: Option<SecretValue>,
    pub openai_api_key: Option<SecretValue>,
    pub openai_base_url: Option<String>,
    pub anthropic_api_key: Option<SecretValue>,
    pub anthropic_base_url: Option<String>,
    pub bedrock_region: Option<String>,
    pub bedrock_access_key: Option<SecretValue>,
    pub bedrock_secret_key: Option<SecretValue>,
    pub bedrock_use_iam_role: Option<bool>,
    pub model_fast: Option<String>,
    pub model_reasoning: Option<String>,
    pub model_embedding: Option<String>,
    pub model_image: Option<String>,
    pub model_video: Option<String>,
    pub model_audio: Option<String>,
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
    pub litellm_key: Option<SecretValue>,
    pub openai_api_key: Option<SecretValue>,
    pub openai_base_url: Option<String>,
    pub anthropic_api_key: Option<SecretValue>,
    pub anthropic_base_url: Option<String>,
    pub bedrock_region: Option<String>,
    pub bedrock_access_key: Option<SecretValue>,
    pub bedrock_secret_key: Option<SecretValue>,
    pub bedrock_use_iam_role: Option<bool>,
    pub model_fast: Option<String>,
    pub model_reasoning: Option<String>,
    pub model_embedding: Option<String>,
    pub model_image: Option<String>,
    pub model_video: Option<String>,
    pub model_audio: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Effective AI settings (merged from org and workspace)
#[derive(Debug, Clone)]
pub struct EffectiveAiSettings {
    pub provider: String,
    pub litellm_host: Option<String>,
    pub litellm_key: Option<SecretValue>,
    pub openai_api_key: Option<SecretValue>,
    pub openai_base_url: Option<String>,
    pub anthropic_api_key: Option<SecretValue>,
    pub anthropic_base_url: Option<String>,
    pub bedrock_region: Option<String>,
    pub bedrock_access_key: Option<SecretValue>,
    pub bedrock_secret_key: Option<SecretValue>,
    pub bedrock_use_iam_role: bool,
    pub model_fast: Option<String>,
    pub model_reasoning: Option<String>,
    pub model_embedding: Option<String>,
    pub model_image: Option<String>,
    pub model_video: Option<String>,
    pub model_audio: Option<String>,
}

impl EffectiveAiSettings {
    pub fn agent(&self) -> Option<AgentKind> {
        agent(&self.provider)
    }

    /// Overlay workspace/org image settings onto the process ComfyUI defaults.
    ///
    /// `model_fast` classifies image intent when rules are unsure, including
    /// informal edits of an attached photo. `model_image` is the
    /// ComfyUI checkpoint used for generation; an empty value keeps
    /// `COMFYUI_CHECKPOINT`. `model_video` is the Wan UNET filename and keeps
    /// `COMFYUI_VIDEO_UNET` when empty. `model_audio` is the ACE-Step
    /// checkpoint and keeps `COMFYUI_AUDIO_CHECKPOINT` when empty.
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
        if let Some(model) = nonempty(self.model_audio.as_deref()) {
            config.audio_checkpoint = model.to_string();
        }
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

async fn get_org<'e, E>(executor: E, organization_id: Uuid) -> DbResult<Option<OrgAiSettingsRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    let row: Option<OrgAiSettingsRow> = sqlx::query_as(
        r#"
        SELECT id, organization_id, provider, litellm_host, litellm_key,
               openai_api_key, openai_base_url, anthropic_api_key, anthropic_base_url,
               bedrock_region, bedrock_access_key, bedrock_secret_key, bedrock_use_iam_role,
               model_fast, model_reasoning, model_embedding, model_image, model_video, model_audio, created_at, updated_at
        FROM organization_ai_settings
        WHERE organization_id = $1
        "#,
    )
    .bind(organization_id)
    .fetch_optional(executor)
    .await?;

    Ok(row)
}

/// Get AI settings for an organization.
pub async fn get_org_ai_settings(
    pool: &PgPool,
    organization_id: Uuid,
) -> DbResult<Option<OrgAiSettingsRow>> {
    get_org(pool, organization_id).await
}

/// Read organization settings while holding the caller's membership row.
pub async fn get_org_authorized(
    pool: &PgPool,
    organization_id: Uuid,
    user_id: Uuid,
) -> AccessResult<Option<OrgAiSettingsRow>> {
    let mut transaction = pool.begin().await?;
    authorize_organization(&mut transaction, organization_id, user_id, OrgRole::Admin).await?;
    let settings = get_org(&mut *transaction, organization_id).await?;
    transaction.commit().await?;
    Ok(settings)
}

async fn upsert_org<'e, E>(
    executor: E,
    organization_id: Uuid,
    update: &Update<'_>,
) -> DbResult<OrgAiSettingsRow>
where
    E: Executor<'e, Database = Postgres>,
{
    let row: OrgAiSettingsRow = sqlx::query_as(
        r#"
        INSERT INTO organization_ai_settings (
            organization_id, provider, litellm_host, litellm_key,
            openai_api_key, openai_base_url, anthropic_api_key, anthropic_base_url,
            bedrock_region, bedrock_access_key, bedrock_secret_key, bedrock_use_iam_role,
            model_fast, model_reasoning, model_embedding, model_image, model_video, model_audio
        ) VALUES (
            $1, COALESCE($2, 'self_hosted'), $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
            NULLIF(BTRIM($13), ''), NULLIF(BTRIM($14), ''), NULLIF(BTRIM($15), ''),
            NULLIF(BTRIM($16), ''), NULLIF(BTRIM($17), ''), NULLIF(BTRIM($18), '')
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
            -- NULL keeps the saved model; an empty string clears it to NULL.
            model_fast = CASE
                WHEN $13 IS NULL THEN organization_ai_settings.model_fast
                WHEN BTRIM($13) = '' THEN NULL
                ELSE $13
            END,
            model_reasoning = CASE
                WHEN $14 IS NULL THEN organization_ai_settings.model_reasoning
                WHEN BTRIM($14) = '' THEN NULL
                ELSE $14
            END,
            model_embedding = CASE
                WHEN $15 IS NULL THEN organization_ai_settings.model_embedding
                WHEN BTRIM($15) = '' THEN NULL
                ELSE $15
            END,
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
            model_audio = CASE
                WHEN $18 IS NULL THEN organization_ai_settings.model_audio
                WHEN BTRIM($18) = '' THEN NULL
                ELSE $18
            END,
            updated_at = NOW()
        RETURNING id, organization_id, provider, litellm_host, litellm_key,
                  openai_api_key, openai_base_url, anthropic_api_key, anthropic_base_url,
                  bedrock_region, bedrock_access_key, bedrock_secret_key, bedrock_use_iam_role,
                  model_fast, model_reasoning, model_embedding, model_image, model_video, model_audio, created_at, updated_at
        "#
    )
    .bind(organization_id)
    .bind(update.provider)
    .bind(update.litellm_host)
    .bind(update.litellm_key)
    .bind(update.openai_api_key)
    .bind(update.openai_base_url)
    .bind(update.anthropic_api_key)
    .bind(update.anthropic_base_url)
    .bind(update.bedrock_region)
    .bind(update.bedrock_access_key)
    .bind(update.bedrock_secret_key)
    .bind(update.bedrock_use_iam_role)
    .bind(update.model_fast)
    .bind(update.model_reasoning)
    .bind(update.model_embedding)
    .bind(update.model_image)
    .bind(update.model_video)
    .bind(update.model_audio)
    .fetch_one(executor)
    .await?;

    Ok(row)
}

/// Upsert organization settings while holding the caller's admin membership row.
pub async fn upsert_org_authorized(
    pool: &PgPool,
    organization_id: Uuid,
    user_id: Uuid,
    update: Update<'_>,
) -> AccessResult<OrgAiSettingsRow> {
    let mut transaction = pool.begin().await?;
    authorize_organization(&mut transaction, organization_id, user_id, OrgRole::Admin).await?;
    validate(&update)?;
    let settings = upsert_org(&mut *transaction, organization_id, &update).await?;
    transaction.commit().await?;
    Ok(settings)
}

async fn delete_org<'e, E>(executor: E, organization_id: Uuid) -> DbResult<bool>
where
    E: Executor<'e, Database = Postgres>,
{
    let result = sqlx::query("DELETE FROM organization_ai_settings WHERE organization_id = $1")
        .bind(organization_id)
        .execute(executor)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Delete organization settings while holding the caller's admin membership row.
pub async fn delete_org_authorized(
    pool: &PgPool,
    organization_id: Uuid,
    user_id: Uuid,
) -> AccessResult<bool> {
    let mut transaction = pool.begin().await?;
    authorize_organization(&mut transaction, organization_id, user_id, OrgRole::Admin).await?;
    let deleted = delete_org(&mut *transaction, organization_id).await?;
    transaction.commit().await?;
    Ok(deleted)
}

async fn get_workspace<'e, E>(
    executor: E,
    workspace_id: Uuid,
) -> DbResult<Option<WorkspaceAiSettingsRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    let row: Option<WorkspaceAiSettingsRow> = sqlx::query_as(
        r#"
        SELECT id, workspace_id, provider, litellm_host, litellm_key,
               openai_api_key, openai_base_url, anthropic_api_key, anthropic_base_url,
               bedrock_region, bedrock_access_key, bedrock_secret_key, bedrock_use_iam_role,
               model_fast, model_reasoning, model_embedding, model_image, model_video, model_audio, created_at, updated_at
        FROM workspace_ai_settings
        WHERE workspace_id = $1
        "#,
    )
    .bind(workspace_id)
    .fetch_optional(executor)
    .await?;

    Ok(row)
}

/// Get AI settings for a workspace.
pub async fn get_workspace_ai_settings(
    pool: &PgPool,
    workspace_id: Uuid,
) -> DbResult<Option<WorkspaceAiSettingsRow>> {
    get_workspace(pool, workspace_id).await
}

/// Read workspace settings while holding both membership rows and the owning workspace row.
pub async fn get_workspace_authorized(
    pool: &PgPool,
    organization_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
) -> AccessResult<Option<WorkspaceAiSettingsRow>> {
    let mut transaction = pool.begin().await?;
    authorize_workspace(
        &mut transaction,
        organization_id,
        workspace_id,
        user_id,
        false,
    )
    .await?;
    let settings = get_workspace(&mut *transaction, workspace_id).await?;
    transaction.commit().await?;
    Ok(settings)
}

async fn upsert_workspace<'e, E>(
    executor: E,
    workspace_id: Uuid,
    update: &Update<'_>,
) -> DbResult<WorkspaceAiSettingsRow>
where
    E: Executor<'e, Database = Postgres>,
{
    let row: WorkspaceAiSettingsRow = sqlx::query_as(
        r#"
        INSERT INTO workspace_ai_settings (
            workspace_id, provider, litellm_host, litellm_key,
            openai_api_key, openai_base_url, anthropic_api_key, anthropic_base_url,
            bedrock_region, bedrock_access_key, bedrock_secret_key, bedrock_use_iam_role,
            model_fast, model_reasoning, model_embedding, model_image, model_video, model_audio
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
            NULLIF(BTRIM($13), ''), NULLIF(BTRIM($14), ''), NULLIF(BTRIM($15), ''),
            NULLIF(BTRIM($16), ''), NULLIF(BTRIM($17), ''), NULLIF(BTRIM($18), '')
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
            -- NULL keeps the saved model; an empty string clears it to NULL.
            model_fast = CASE
                WHEN $13 IS NULL THEN workspace_ai_settings.model_fast
                WHEN BTRIM($13) = '' THEN NULL
                ELSE $13
            END,
            model_reasoning = CASE
                WHEN $14 IS NULL THEN workspace_ai_settings.model_reasoning
                WHEN BTRIM($14) = '' THEN NULL
                ELSE $14
            END,
            model_embedding = CASE
                WHEN $15 IS NULL THEN workspace_ai_settings.model_embedding
                WHEN BTRIM($15) = '' THEN NULL
                ELSE $15
            END,
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
            model_audio = CASE
                WHEN $18 IS NULL THEN workspace_ai_settings.model_audio
                WHEN BTRIM($18) = '' THEN NULL
                ELSE $18
            END,
            updated_at = NOW()
        RETURNING id, workspace_id, provider, litellm_host, litellm_key,
                  openai_api_key, openai_base_url, anthropic_api_key, anthropic_base_url,
                  bedrock_region, bedrock_access_key, bedrock_secret_key, bedrock_use_iam_role,
                  model_fast, model_reasoning, model_embedding, model_image, model_video, model_audio, created_at, updated_at
        "#,
    )
    .bind(workspace_id)
    .bind(update.provider)
    .bind(update.litellm_host)
    .bind(update.litellm_key)
    .bind(update.openai_api_key)
    .bind(update.openai_base_url)
    .bind(update.anthropic_api_key)
    .bind(update.anthropic_base_url)
    .bind(update.bedrock_region)
    .bind(update.bedrock_access_key)
    .bind(update.bedrock_secret_key)
    .bind(update.bedrock_use_iam_role)
    .bind(update.model_fast)
    .bind(update.model_reasoning)
    .bind(update.model_embedding)
    .bind(update.model_image)
    .bind(update.model_video)
    .bind(update.model_audio)
    .fetch_one(executor)
    .await?;

    Ok(row)
}

/// Upsert workspace settings while holding the organization and workspace membership rows.
pub async fn upsert_workspace_authorized(
    pool: &PgPool,
    organization_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    update: Update<'_>,
) -> AccessResult<WorkspaceAiSettingsRow> {
    let mut transaction = pool.begin().await?;
    authorize_workspace(
        &mut transaction,
        organization_id,
        workspace_id,
        user_id,
        true,
    )
    .await?;
    validate(&update)?;
    let settings = upsert_workspace(&mut *transaction, workspace_id, &update).await?;
    transaction.commit().await?;
    Ok(settings)
}

async fn delete_workspace<'e, E>(executor: E, workspace_id: Uuid) -> DbResult<bool>
where
    E: Executor<'e, Database = Postgres>,
{
    let result = sqlx::query("DELETE FROM workspace_ai_settings WHERE workspace_id = $1")
        .bind(workspace_id)
        .execute(executor)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Delete workspace settings while holding the organization and workspace membership rows.
pub async fn delete_workspace_authorized(
    pool: &PgPool,
    organization_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
) -> AccessResult<bool> {
    let mut transaction = pool.begin().await?;
    authorize_workspace(
        &mut transaction,
        organization_id,
        workspace_id,
        user_id,
        true,
    )
    .await?;
    let deleted = delete_workspace(&mut *transaction, workspace_id).await?;
    transaction.commit().await?;
    Ok(deleted)
}

fn effective(
    organization: Option<OrgAiSettingsRow>,
    workspace: Option<WorkspaceAiSettingsRow>,
) -> EffectiveAiSettings {
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
        model_audio: None,
    };

    if let Some(org) = organization {
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
        effective.model_audio = org.model_audio;
    }

    if let Some(ws) = workspace {
        if let Some(provider) = ws.provider {
            if provider != effective.provider {
                effective.model_fast = None;
                effective.model_reasoning = None;
                effective.model_embedding = None;
            }
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
        if ws.model_audio.is_some() {
            effective.model_audio = ws.model_audio;
        }
    }

    effective
}

/// Get effective AI settings for a workspace (workspace overrides organization).
pub async fn get_effective_ai_settings(
    pool: &PgPool,
    organization_id: Uuid,
    workspace_id: Uuid,
) -> DbResult<EffectiveAiSettings> {
    let organization = get_org_ai_settings(pool, organization_id).await?;
    let workspace = get_workspace_ai_settings(pool, workspace_id).await?;
    Ok(effective(organization, workspace))
}

/// Read effective settings while holding both membership rows and the owning workspace row.
pub async fn get_effective_authorized(
    pool: &PgPool,
    organization_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
) -> AccessResult<EffectiveAiSettings> {
    let mut transaction = pool.begin().await?;
    authorize_workspace(
        &mut transaction,
        organization_id,
        workspace_id,
        user_id,
        false,
    )
    .await?;
    let organization = get_org(&mut *transaction, organization_id).await?;
    let workspace = get_workspace(&mut *transaction, workspace_id).await?;
    transaction.commit().await?;
    Ok(effective(organization, workspace))
}

/// Effective settings for a workspace, resolving the owning organization for
/// the caller. Returns `None` when the workspace or its settings cannot be
/// read, so a settings outage degrades to process defaults instead of failing
/// the request.
pub async fn for_workspace(pool: &PgPool, workspace_id: Uuid) -> Option<EffectiveAiSettings> {
    let workspace = super::workspaces::get_workspace(pool, workspace_id)
        .await
        .ok()
        .flatten()?;
    get_effective_ai_settings(pool, workspace.organization_id, workspace_id)
        .await
        .ok()
}

/// The ComfyUI config a workspace should actually generate with: process
/// defaults overlaid with the models its organization and workspace pinned.
///
/// Every media lane must resolve through here. Reading `state.config().comfyui`
/// directly silently ignores a pinned `model_image`/`model_video`/`model_audio`.
pub async fn effective_comfyui(
    pool: &PgPool,
    workspace_id: Uuid,
    defaults: &crate::config::ComfyUiConfig,
) -> crate::config::ComfyUiConfig {
    let mut config = defaults.clone();
    if let Some(settings) = for_workspace(pool, workspace_id).await {
        settings.apply_to_comfyui(&mut config);
    }
    config
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
            model_audio: None,
        }
    }

    #[test]
    fn each_agent_provider_selects_its_agent_and_back() {
        assert_eq!(agent(PROVIDER_CLAUDE_CODE), Some(AgentKind::Claude));
        assert_eq!(agent(PROVIDER_CODEX), Some(AgentKind::Codex));
        for kind in AgentKind::ALL {
            assert_eq!(agent(provider(kind)), Some(kind));
        }
    }

    #[test]
    fn a_provider_that_is_no_agent_selects_none() {
        for name in [
            PROVIDER_SELF_HOSTED,
            PROVIDER_OPENAI,
            PROVIDER_ANTHROPIC,
            PROVIDER_BEDROCK,
            AgentKind::Claude.as_str(),
            "",
            "gemini",
        ] {
            assert_eq!(agent(name), None, "{name:?} must not select an agent");
        }
    }

    #[test]
    fn effective_settings_select_the_agent_their_provider_names() {
        let mut settings = settings(None, None);
        assert_eq!(settings.agent(), None);
        settings.provider = PROVIDER_CLAUDE_CODE.to_string();
        assert_eq!(settings.agent(), Some(AgentKind::Claude));
        settings.provider = PROVIDER_CODEX.to_string();
        assert_eq!(settings.agent(), Some(AgentKind::Codex));
    }

    #[test]
    fn validate_accepts_every_provider_and_lists_them_all_when_it_refuses() {
        for provider in PROVIDERS {
            let update = Update {
                provider: Some(provider),
                ..Update::default()
            };
            assert!(validate(&update).is_ok(), "{provider} must be accepted");
        }
        assert!(validate(&Update::default()).is_ok());

        let refused = validate(&Update {
            provider: Some("gemini"),
            ..Update::default()
        })
        .expect_err("gemini is not a provider");
        assert_eq!(
            refused.to_string(),
            "Invalid provider. Must be one of: self_hosted, openai, anthropic, bedrock, claude_code, codex"
        );
    }

    #[test]
    fn apply_to_comfyui_uses_configured_image_model() {
        let mut config = ComfyUiConfig::default();
        settings(Some("llama3.2:3b"), Some("custom-image.safetensors"))
            .apply_to_comfyui(&mut config);
        assert_eq!(config.classifier_model, "llama3.2:3b");
        assert_eq!(config.checkpoint, "custom-image.safetensors");
        assert_eq!(config.video_unet, "wan2.2_ti2v_5B_fp16.safetensors");
        assert_eq!(config.audio_checkpoint, "ace_step_v1_3.5b.safetensors");
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
    fn apply_to_comfyui_uses_configured_audio_model() {
        let mut config = ComfyUiConfig::default();
        let mut settings = settings(None, None);
        settings.model_audio = Some("custom-audio.safetensors".to_string());
        settings.apply_to_comfyui(&mut config);
        assert_eq!(config.audio_checkpoint, "custom-audio.safetensors");
    }

    #[test]
    fn apply_to_comfyui_keeps_default_audio_checkpoint() {
        let mut config = ComfyUiConfig::default();
        settings(None, None).apply_to_comfyui(&mut config);
        assert_eq!(config.audio_checkpoint, "ace_step_v1_3.5b.safetensors");

        let mut blank = settings(None, None);
        blank.model_audio = Some("   ".to_string());
        blank.apply_to_comfyui(&mut config);
        assert_eq!(config.audio_checkpoint, "ace_step_v1_3.5b.safetensors");
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
