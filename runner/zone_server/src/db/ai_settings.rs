//! AI provider settings database queries

use chrono::NaiveDateTime;
use sqlx::{Executor, PgConnection, PgPool, Postgres};
use uuid::Uuid;
use zone_context::embeddings::providers::{
    PROVIDER_BEDROCK, PROVIDER_OPENAI, PROVIDER_SELF_HOSTED,
};
use zone_core::SecretValue;
use zone_core::llm::AgentKind;

use crate::services::endpoint;
use crate::services::hosts::Hosts;

use super::{
    DbResult,
    organization_members::{self, OrgRole},
    workspace_members::{self, WorkspaceRole},
};

pub const PROVIDER_ANTHROPIC: &str = "anthropic";
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

const RE_ENTER_KEY: &str = "Re-enter the key when changing the endpoint URL.";

/// An endpoint URL and the key saved beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pair {
    Litellm,
    OpenAI,
    Anthropic,
}

impl Pair {
    const ALL: [Self; 3] = [Self::Litellm, Self::OpenAI, Self::Anthropic];

    fn url_field(self) -> &'static str {
        match self {
            Self::Litellm => "litellm_host",
            Self::OpenAI => "openai_base_url",
            Self::Anthropic => "anthropic_base_url",
        }
    }

    fn key_field(self) -> &'static str {
        match self {
            Self::Litellm => "litellm_key",
            Self::OpenAI => "openai_api_key",
            Self::Anthropic => "anthropic_api_key",
        }
    }

    /// Where a key saved without a URL is sent. A LiteLLM key without a host
    /// is never sent: the instance runs on its own key.
    fn default_url(self) -> Option<&'static str> {
        match self {
            Self::Litellm => None,
            Self::OpenAI => Some(endpoint::OPENAI_URL),
            Self::Anthropic => Some(endpoint::ANTHROPIC_URL),
        }
    }

    fn url<'a>(self, update: &Update<'a>) -> Option<&'a str> {
        match self {
            Self::Litellm => update.litellm_host,
            Self::OpenAI => update.openai_base_url,
            Self::Anthropic => update.anthropic_base_url,
        }
    }

    fn key<'a>(self, update: &Update<'a>) -> Option<&'a str> {
        match self {
            Self::Litellm => update.litellm_key,
            Self::OpenAI => update.openai_api_key,
            Self::Anthropic => update.anthropic_api_key,
        }
    }
}

/// The endpoint URLs a row saves, and whether a key is saved beside each.
#[derive(Debug, Clone, Default, sqlx::FromRow)]
struct SavedEndpoints {
    litellm_host: Option<String>,
    litellm_key: bool,
    openai_base_url: Option<String>,
    openai_api_key: bool,
    anthropic_base_url: Option<String>,
    anthropic_api_key: bool,
    routed: bool,
}

impl SavedEndpoints {
    fn url(&self, pair: Pair) -> Option<&str> {
        nonempty(match pair {
            Pair::Litellm => self.litellm_host.as_deref(),
            Pair::OpenAI => self.openai_base_url.as_deref(),
            Pair::Anthropic => self.anthropic_base_url.as_deref(),
        })
    }

    fn keyed(&self, pair: Pair) -> bool {
        match pair {
            Pair::Litellm => self.litellm_key,
            Pair::OpenAI => self.openai_api_key,
            Pair::Anthropic => self.anthropic_api_key,
        }
    }

    /// The URL an organization row lends a workspace key saved without one.
    /// A row saved before completions were routed lends none.
    fn lent(&self, pair: Pair) -> Option<&str> {
        self.routed.then(|| self.url(pair)).flatten()
    }
}

/// The pairs whose lent URL an organization update moves: a workspace key
/// saved without a URL would follow it to a host its admin never chose. The
/// saved row is routed once the update is saved, so a URL it kept from before
/// routing is lent from then on.
fn moved_pairs(update: &Update<'_>, saved: &SavedEndpoints) -> [bool; 3] {
    Pair::ALL.map(|pair| {
        let next = match pair.url(update) {
            Some(url) => nonempty(Some(url)),
            None => saved.url(pair),
        };
        next != saved.lent(pair)
    })
}

const ORGANIZATION_ENDPOINTS: &str = r#"
    SELECT litellm_host, litellm_key IS NOT NULL AS litellm_key,
           openai_base_url, openai_api_key IS NOT NULL AS openai_api_key,
           anthropic_base_url, anthropic_api_key IS NOT NULL AS anthropic_api_key,
           completions_routed AS routed
    FROM organization_ai_settings
    WHERE organization_id = $1
    FOR UPDATE
"#;

const WORKSPACE_ENDPOINTS: &str = r#"
    SELECT litellm_host, litellm_key IS NOT NULL AS litellm_key,
           openai_base_url, openai_api_key IS NOT NULL AS openai_api_key,
           anthropic_base_url, anthropic_api_key IS NOT NULL AS anthropic_api_key,
           completions_routed AS routed
    FROM workspace_ai_settings
    WHERE workspace_id = $1
    FOR UPDATE
"#;

const ROUTED_ORGANIZATION_ENDPOINTS: &str = r#"
    SELECT litellm_host, litellm_key IS NOT NULL AS litellm_key,
           openai_base_url, openai_api_key IS NOT NULL AS openai_api_key,
           anthropic_base_url, anthropic_api_key IS NOT NULL AS anthropic_api_key,
           completions_routed AS routed
    FROM organization_ai_settings
    WHERE organization_id = $1 AND completions_routed
"#;

/// Stop routing the workspace rows of an organization that keep a key without
/// a URL for a pair whose lent URL moved, so those keys never reach the new
/// host. Their workspaces fall back as rows saved before routing until their
/// admins save again. Returns how many rows stopped.
const UNROUTE_KEYS_WITHOUT_URL: &str = r#"
    UPDATE workspace_ai_settings AS settings
    SET completions_routed = false, updated_at = NOW()
    FROM workspaces
    WHERE workspaces.id = settings.workspace_id
      AND workspaces.organization_id = $1
      AND settings.completions_routed
      AND (
        ($2 AND settings.litellm_key IS NOT NULL
            AND NULLIF(BTRIM(settings.litellm_host), '') IS NULL)
        OR ($3 AND settings.openai_api_key IS NOT NULL
            AND NULLIF(BTRIM(settings.openai_base_url), '') IS NULL)
        OR ($4 AND settings.anthropic_api_key IS NOT NULL
            AND NULLIF(BTRIM(settings.anthropic_base_url), '') IS NULL)
      )
"#;

async fn unroute_keys_without_url(
    connection: &mut PgConnection,
    organization_id: Uuid,
    [litellm, openai, anthropic]: [bool; 3],
) -> DbResult<u64> {
    if !(litellm || openai || anthropic) {
        return Ok(0);
    }
    let result = sqlx::query(UNROUTE_KEYS_WITHOUT_URL)
        .bind(organization_id)
        .bind(litellm)
        .bind(openai)
        .bind(anthropic)
        .execute(connection)
        .await?;
    Ok(result.rows_affected())
}

async fn saved_endpoints(
    connection: &mut PgConnection,
    query: &'static str,
    id: Uuid,
) -> DbResult<SavedEndpoints> {
    let saved: Option<SavedEndpoints> = sqlx::query_as(query)
        .bind(id)
        .fetch_optional(connection)
        .await?;
    Ok(saved.unwrap_or_default())
}

fn validate(update: &Update<'_>, hosts: &Hosts) -> AccessResult<()> {
    if let Some(provider) = update.provider
        && !PROVIDERS.contains(&provider)
    {
        return Err(AccessError::Invalid(format!(
            "Invalid provider. Must be one of: {}",
            PROVIDERS.join(", ")
        )));
    }
    for pair in Pair::ALL {
        if let Some(value) = nonempty(pair.url(update)) {
            endpoint::validate_url(value, hosts)
                .map_err(|error| AccessError::Invalid(format!("{}: {error}", pair.url_field())))?;
        }
    }
    Ok(())
}

/// Refuse an update that would send a saved key somewhere it was not saved
/// for: to a URL other than the one beside it, unless the update brings the
/// key again, or to a provider's default host the instance does not list.
/// `saved` is the row being updated and `inherited` the URLs it falls back
/// to, which a workspace takes from its organization.
fn validate_keys(
    update: &Update<'_>,
    saved: &SavedEndpoints,
    inherited: &SavedEndpoints,
    hosts: &Hosts,
) -> AccessResult<()> {
    for pair in Pair::ALL {
        let (url, key) = (pair.url(update), pair.key(update));
        if url.is_none() && key.is_none() {
            continue;
        }
        let saved_url = saved.url(pair);
        let next_url = match url {
            Some(url) => nonempty(Some(url)),
            None => saved_url,
        };
        if url.is_some() && next_url != saved_url && key.is_none() && saved.keyed(pair) {
            return Err(AccessError::Invalid(format!(
                "{}: {RE_ENTER_KEY}",
                pair.url_field()
            )));
        }
        let keyed = match key {
            Some(key) => nonempty(Some(key)).is_some(),
            None => saved.keyed(pair),
        };
        if keyed
            && next_url.is_none()
            && inherited.url(pair).is_none()
            && let Some(default) = pair.default_url()
            && endpoint::validate_url(default, hosts).is_err()
        {
            return Err(AccessError::Invalid(format!(
                "{}: a key saved without a base URL goes to {}, which is not a host this instance allows endpoints on. Save a base URL on a host it allows.",
                pair.key_field(),
                host(default)
            )));
        }
    }
    Ok(())
}

fn host(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| url.to_string())
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

    // These settings override the organization's for every chat in the
    // workspace, so writing them takes the workspace's own administrators --
    // the rank the organization route demands for the settings this one
    // overrides. A base URL written here never receives the organization's
    // credential (see `overlay_endpoint`). Every member still reads them,
    // because their chats run under them.
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
    /// Set by a save through the API. A row saved before completions were sent
    /// to saved endpoints lends none: its URLs and keys were never used.
    pub completions_routed: bool,
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
    /// Set by a save through the API. A row saved before completions were sent
    /// to saved endpoints lends none: its URLs and keys were never used.
    pub completions_routed: bool,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Which endpoint keys the owning organization saved. A workspace host never
/// receives them, so a workspace that names one without its own key sends none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, sqlx::FromRow)]
pub struct OrganizationKeys {
    pub litellm: bool,
    pub openai: bool,
    pub anthropic: bool,
}

/// An organization's saved settings, and how many of its workspaces stopped
/// routing because their key saved without a URL would have followed a moved
/// organization URL.
#[derive(Debug, Clone)]
pub struct OrganizationSave {
    pub settings: OrgAiSettingsRow,
    pub unrouted_workspaces: u64,
}

/// A workspace's own settings beside the keys its organization saved.
#[derive(Debug, Clone)]
pub struct WorkspaceSettings<T> {
    pub settings: T,
    pub organization_keys: OrganizationKeys,
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
               model_fast, model_reasoning, model_embedding, model_image, model_video, model_audio,
               completions_routed, created_at, updated_at
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
            model_fast, model_reasoning, model_embedding, model_image, model_video, model_audio,
            completions_routed
        ) VALUES (
            $1, COALESCE($2, 'self_hosted'),
            NULLIF(BTRIM($3), ''), NULLIF(BTRIM($4), ''), NULLIF(BTRIM($5), ''),
            NULLIF(BTRIM($6), ''), NULLIF(BTRIM($7), ''), NULLIF(BTRIM($8), ''),
            $9, $10, $11, $12,
            NULLIF(BTRIM($13), ''), NULLIF(BTRIM($14), ''), NULLIF(BTRIM($15), ''),
            NULLIF(BTRIM($16), ''), NULLIF(BTRIM($17), ''), NULLIF(BTRIM($18), ''),
            true
        )
        ON CONFLICT (organization_id) DO UPDATE SET
            provider = COALESCE($2, organization_ai_settings.provider),
            -- NULL keeps a saved endpoint value; an empty string clears it.
            litellm_host = CASE
                WHEN $3 IS NULL THEN organization_ai_settings.litellm_host
                WHEN BTRIM($3) = '' THEN NULL
                ELSE BTRIM($3)
            END,
            litellm_key = CASE
                WHEN $4 IS NULL THEN organization_ai_settings.litellm_key
                WHEN BTRIM($4) = '' THEN NULL
                ELSE BTRIM($4)
            END,
            openai_api_key = CASE
                WHEN $5 IS NULL THEN organization_ai_settings.openai_api_key
                WHEN BTRIM($5) = '' THEN NULL
                ELSE BTRIM($5)
            END,
            openai_base_url = CASE
                WHEN $6 IS NULL THEN organization_ai_settings.openai_base_url
                WHEN BTRIM($6) = '' THEN NULL
                ELSE BTRIM($6)
            END,
            anthropic_api_key = CASE
                WHEN $7 IS NULL THEN organization_ai_settings.anthropic_api_key
                WHEN BTRIM($7) = '' THEN NULL
                ELSE BTRIM($7)
            END,
            anthropic_base_url = CASE
                WHEN $8 IS NULL THEN organization_ai_settings.anthropic_base_url
                WHEN BTRIM($8) = '' THEN NULL
                ELSE BTRIM($8)
            END,
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
            completions_routed = true,
            updated_at = NOW()
        RETURNING id, organization_id, provider, litellm_host, litellm_key,
                  openai_api_key, openai_base_url, anthropic_api_key, anthropic_base_url,
                  bedrock_region, bedrock_access_key, bedrock_secret_key, bedrock_use_iam_role,
                  model_fast, model_reasoning, model_embedding, model_image, model_video, model_audio,
                  completions_routed, created_at, updated_at
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
    hosts: &Hosts,
    organization_id: Uuid,
    user_id: Uuid,
    update: Update<'_>,
) -> AccessResult<OrganizationSave> {
    let mut transaction = pool.begin().await?;
    authorize_organization(&mut transaction, organization_id, user_id, OrgRole::Admin).await?;
    validate(&update, hosts)?;
    let saved = saved_endpoints(&mut transaction, ORGANIZATION_ENDPOINTS, organization_id).await?;
    validate_keys(&update, &saved, &SavedEndpoints::default(), hosts)?;
    let settings = upsert_org(&mut *transaction, organization_id, &update).await?;
    let unrouted_workspaces = unroute_keys_without_url(
        &mut transaction,
        organization_id,
        moved_pairs(&update, &saved),
    )
    .await?;
    transaction.commit().await?;
    Ok(OrganizationSave {
        settings,
        unrouted_workspaces,
    })
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
               model_fast, model_reasoning, model_embedding, model_image, model_video, model_audio,
               completions_routed, created_at, updated_at
        FROM workspace_ai_settings
        WHERE workspace_id = $1
        "#,
    )
    .bind(workspace_id)
    .fetch_optional(executor)
    .await?;

    Ok(row)
}

async fn organization_keys<'e, E>(executor: E, organization_id: Uuid) -> DbResult<OrganizationKeys>
where
    E: Executor<'e, Database = Postgres>,
{
    let keys: Option<OrganizationKeys> = sqlx::query_as(
        r#"
        SELECT litellm_key IS NOT NULL AS litellm,
               openai_api_key IS NOT NULL AS openai,
               anthropic_api_key IS NOT NULL AS anthropic
        FROM organization_ai_settings
        WHERE organization_id = $1
        "#,
    )
    .bind(organization_id)
    .fetch_optional(executor)
    .await?;

    Ok(keys.unwrap_or_default())
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
) -> AccessResult<WorkspaceSettings<Option<WorkspaceAiSettingsRow>>> {
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
    let organization_keys = organization_keys(&mut *transaction, organization_id).await?;
    transaction.commit().await?;
    Ok(WorkspaceSettings {
        settings,
        organization_keys,
    })
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
            model_fast, model_reasoning, model_embedding, model_image, model_video, model_audio,
            completions_routed
        ) VALUES (
            $1, $2,
            NULLIF(BTRIM($3), ''), NULLIF(BTRIM($4), ''), NULLIF(BTRIM($5), ''),
            NULLIF(BTRIM($6), ''), NULLIF(BTRIM($7), ''), NULLIF(BTRIM($8), ''),
            $9, $10, $11, $12,
            NULLIF(BTRIM($13), ''), NULLIF(BTRIM($14), ''), NULLIF(BTRIM($15), ''),
            NULLIF(BTRIM($16), ''), NULLIF(BTRIM($17), ''), NULLIF(BTRIM($18), ''),
            true
        )
        ON CONFLICT (workspace_id) DO UPDATE SET
            provider = $2,
            -- NULL keeps a saved endpoint value; an empty string clears it.
            litellm_host = CASE
                WHEN $3 IS NULL THEN workspace_ai_settings.litellm_host
                WHEN BTRIM($3) = '' THEN NULL
                ELSE BTRIM($3)
            END,
            litellm_key = CASE
                WHEN $4 IS NULL THEN workspace_ai_settings.litellm_key
                WHEN BTRIM($4) = '' THEN NULL
                ELSE BTRIM($4)
            END,
            openai_api_key = CASE
                WHEN $5 IS NULL THEN workspace_ai_settings.openai_api_key
                WHEN BTRIM($5) = '' THEN NULL
                ELSE BTRIM($5)
            END,
            openai_base_url = CASE
                WHEN $6 IS NULL THEN workspace_ai_settings.openai_base_url
                WHEN BTRIM($6) = '' THEN NULL
                ELSE BTRIM($6)
            END,
            anthropic_api_key = CASE
                WHEN $7 IS NULL THEN workspace_ai_settings.anthropic_api_key
                WHEN BTRIM($7) = '' THEN NULL
                ELSE BTRIM($7)
            END,
            anthropic_base_url = CASE
                WHEN $8 IS NULL THEN workspace_ai_settings.anthropic_base_url
                WHEN BTRIM($8) = '' THEN NULL
                ELSE BTRIM($8)
            END,
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
            completions_routed = true,
            updated_at = NOW()
        RETURNING id, workspace_id, provider, litellm_host, litellm_key,
                  openai_api_key, openai_base_url, anthropic_api_key, anthropic_base_url,
                  bedrock_region, bedrock_access_key, bedrock_secret_key, bedrock_use_iam_role,
                  model_fast, model_reasoning, model_embedding, model_image, model_video, model_audio,
                  completions_routed, created_at, updated_at
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
    hosts: &Hosts,
    organization_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    update: Update<'_>,
) -> AccessResult<WorkspaceSettings<WorkspaceAiSettingsRow>> {
    let mut transaction = pool.begin().await?;
    authorize_workspace(
        &mut transaction,
        organization_id,
        workspace_id,
        user_id,
        true,
    )
    .await?;
    validate(&update, hosts)?;
    let saved = saved_endpoints(&mut transaction, WORKSPACE_ENDPOINTS, workspace_id).await?;
    let inherited = saved_endpoints(
        &mut transaction,
        ROUTED_ORGANIZATION_ENDPOINTS,
        organization_id,
    )
    .await?;
    validate_keys(&update, &saved, &inherited, hosts)?;
    let settings = upsert_workspace(&mut *transaction, workspace_id, &update).await?;
    let organization_keys = organization_keys(&mut *transaction, organization_id).await?;
    transaction.commit().await?;
    Ok(WorkspaceSettings {
        settings,
        organization_keys,
    })
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
        if org.completions_routed {
            effective.litellm_host = org.litellm_host;
            effective.litellm_key = org.litellm_key;
            effective.openai_api_key = org.openai_api_key;
            effective.openai_base_url = org.openai_base_url;
            effective.anthropic_api_key = org.anthropic_api_key;
            effective.anthropic_base_url = org.anthropic_base_url;
        }
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
        if ws.completions_routed {
            overlay_endpoint(
                &mut effective.litellm_host,
                &mut effective.litellm_key,
                ws.litellm_host,
                ws.litellm_key,
            );
            overlay_endpoint(
                &mut effective.openai_base_url,
                &mut effective.openai_api_key,
                ws.openai_base_url,
                ws.openai_api_key,
            );
            overlay_endpoint(
                &mut effective.anthropic_base_url,
                &mut effective.anthropic_api_key,
                ws.anthropic_base_url,
                ws.anthropic_api_key,
            );
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

/// A workspace that names its own endpoint brings its own key, even none: the
/// organization's key never leaves for a host the workspace chose. A key saved
/// without an endpoint keeps the organization's host, and a blank endpoint
/// inherits the organization's pair.
fn overlay_endpoint(
    url: &mut Option<String>,
    key: &mut Option<SecretValue>,
    workspace_url: Option<String>,
    workspace_key: Option<SecretValue>,
) {
    if nonempty(workspace_url.as_deref()).is_some() {
        *url = workspace_url;
        *key = workspace_key;
    } else if workspace_key.is_some() {
        *key = workspace_key;
    }
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
    use zone_core::OptionalSecretExt;

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
            assert!(
                validate(&update, &Hosts::default()).is_ok(),
                "{provider} must be accepted"
            );
        }
        assert!(validate(&Update::default(), &Hosts::default()).is_ok());

        let refused = validate(
            &Update {
                provider: Some("gemini"),
                ..Update::default()
            },
            &Hosts::default(),
        )
        .expect_err("gemini is not a provider");
        assert_eq!(
            refused.to_string(),
            "Invalid provider. Must be one of: self_hosted, openai, anthropic, bedrock, claude_code, codex"
        );
    }

    fn organization_row() -> OrgAiSettingsRow {
        OrgAiSettingsRow {
            id: Uuid::new_v4(),
            organization_id: Uuid::new_v4(),
            provider: PROVIDER_SELF_HOSTED.to_string(),
            litellm_host: Some("http://organization-litellm:4000".to_string()),
            litellm_key: Some(SecretValue::new("organization-litellm-key")),
            openai_api_key: Some(SecretValue::new("organization-openai-key")),
            openai_base_url: Some("https://organization-openai.example/v1".to_string()),
            anthropic_api_key: Some(SecretValue::new("organization-anthropic-key")),
            anthropic_base_url: Some("https://organization-anthropic.example/v1".to_string()),
            bedrock_region: None,
            bedrock_access_key: None,
            bedrock_secret_key: None,
            bedrock_use_iam_role: None,
            model_fast: None,
            model_reasoning: None,
            model_embedding: None,
            model_image: None,
            model_video: None,
            model_audio: None,
            completions_routed: true,
            created_at: None,
            updated_at: None,
        }
    }

    fn workspace_row() -> WorkspaceAiSettingsRow {
        WorkspaceAiSettingsRow {
            id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            provider: None,
            litellm_host: None,
            litellm_key: None,
            openai_api_key: None,
            openai_base_url: None,
            anthropic_api_key: None,
            anthropic_base_url: None,
            bedrock_region: None,
            bedrock_access_key: None,
            bedrock_secret_key: None,
            bedrock_use_iam_role: None,
            model_fast: None,
            model_reasoning: None,
            model_embedding: None,
            model_image: None,
            model_video: None,
            model_audio: None,
            completions_routed: true,
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn a_workspace_endpoint_never_inherits_the_organization_key() {
        let workspace = WorkspaceAiSettingsRow {
            litellm_host: Some("http://workspace-litellm:4000".to_string()),
            openai_base_url: Some("https://workspace-openai.example/v1".to_string()),
            anthropic_base_url: Some("https://workspace-anthropic.example/v1".to_string()),
            ..workspace_row()
        };

        let settings = effective(Some(organization_row()), Some(workspace));

        assert_eq!(
            settings.litellm_host.as_deref(),
            Some("http://workspace-litellm:4000")
        );
        assert_eq!(settings.litellm_key.expose_as_deref(), None);
        assert_eq!(
            settings.openai_base_url.as_deref(),
            Some("https://workspace-openai.example/v1")
        );
        assert_eq!(settings.openai_api_key.expose_as_deref(), None);
        assert_eq!(
            settings.anthropic_base_url.as_deref(),
            Some("https://workspace-anthropic.example/v1")
        );
        assert_eq!(settings.anthropic_api_key.expose_as_deref(), None);
    }

    #[test]
    fn a_workspace_endpoint_uses_the_workspace_key() {
        let workspace = WorkspaceAiSettingsRow {
            litellm_host: Some("http://workspace-litellm:4000".to_string()),
            litellm_key: Some(SecretValue::new("workspace-litellm-key")),
            openai_base_url: Some("https://workspace-openai.example/v1".to_string()),
            openai_api_key: Some(SecretValue::new("workspace-openai-key")),
            anthropic_base_url: Some("https://workspace-anthropic.example/v1".to_string()),
            anthropic_api_key: Some(SecretValue::new("workspace-anthropic-key")),
            ..workspace_row()
        };

        let settings = effective(Some(organization_row()), Some(workspace));

        assert_eq!(
            settings.litellm_key.expose_as_deref(),
            Some("workspace-litellm-key")
        );
        assert_eq!(
            settings.openai_api_key.expose_as_deref(),
            Some("workspace-openai-key")
        );
        assert_eq!(
            settings.anthropic_api_key.expose_as_deref(),
            Some("workspace-anthropic-key")
        );
    }

    #[test]
    fn a_workspace_key_alone_keeps_the_organization_endpoint() {
        let workspace = WorkspaceAiSettingsRow {
            litellm_key: Some(SecretValue::new("workspace-litellm-key")),
            openai_api_key: Some(SecretValue::new("workspace-openai-key")),
            anthropic_base_url: Some("  ".to_string()),
            anthropic_api_key: Some(SecretValue::new("workspace-anthropic-key")),
            ..workspace_row()
        };

        let settings = effective(Some(organization_row()), Some(workspace));

        assert_eq!(
            settings.litellm_host.as_deref(),
            Some("http://organization-litellm:4000")
        );
        assert_eq!(
            settings.litellm_key.expose_as_deref(),
            Some("workspace-litellm-key")
        );
        assert_eq!(
            settings.openai_base_url.as_deref(),
            Some("https://organization-openai.example/v1")
        );
        assert_eq!(
            settings.openai_api_key.expose_as_deref(),
            Some("workspace-openai-key")
        );
        assert_eq!(
            settings.anthropic_base_url.as_deref(),
            Some("https://organization-anthropic.example/v1")
        );
        assert_eq!(
            settings.anthropic_api_key.expose_as_deref(),
            Some("workspace-anthropic-key")
        );
    }

    #[test]
    fn a_workspace_without_endpoints_or_keys_inherits_the_organization_pairs() {
        let settings = effective(Some(organization_row()), Some(workspace_row()));

        assert_eq!(
            settings.litellm_key.expose_as_deref(),
            Some("organization-litellm-key")
        );
        assert_eq!(
            settings.openai_api_key.expose_as_deref(),
            Some("organization-openai-key")
        );
        assert_eq!(
            settings.anthropic_api_key.expose_as_deref(),
            Some("organization-anthropic-key")
        );
    }

    #[test]
    fn an_organization_row_saved_before_routing_lends_no_endpoint() {
        let organization = OrgAiSettingsRow {
            provider: PROVIDER_OPENAI.to_string(),
            model_fast: Some("gpt-4o-mini".to_string()),
            completions_routed: false,
            ..organization_row()
        };

        let settings = effective(Some(organization), None);

        assert_eq!(settings.provider, PROVIDER_OPENAI);
        assert_eq!(settings.model_fast.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(settings.litellm_host, None);
        assert_eq!(settings.litellm_key.expose_as_deref(), None);
        assert_eq!(settings.openai_base_url, None);
        assert_eq!(settings.openai_api_key.expose_as_deref(), None);
        assert_eq!(settings.anthropic_base_url, None);
        assert_eq!(settings.anthropic_api_key.expose_as_deref(), None);
    }

    #[test]
    fn a_workspace_row_saved_before_routing_keeps_the_organization_pairs() {
        let workspace = WorkspaceAiSettingsRow {
            litellm_host: Some("http://localhost:11434".to_string()),
            openai_base_url: Some("https://workspace-openai.example/v1".to_string()),
            anthropic_api_key: Some(SecretValue::new("workspace-anthropic-key")),
            completions_routed: false,
            ..workspace_row()
        };

        let settings = effective(Some(organization_row()), Some(workspace));

        assert_eq!(
            settings.litellm_host.as_deref(),
            Some("http://organization-litellm:4000")
        );
        assert_eq!(
            settings.litellm_key.expose_as_deref(),
            Some("organization-litellm-key")
        );
        assert_eq!(
            settings.openai_base_url.as_deref(),
            Some("https://organization-openai.example/v1")
        );
        assert_eq!(
            settings.anthropic_api_key.expose_as_deref(),
            Some("organization-anthropic-key")
        );
    }

    #[test]
    fn validate_names_the_field_whose_url_it_refuses_and_allows_blanks() {
        let refused = validate(
            &Update {
                anthropic_base_url: Some("file:///etc/passwd"),
                ..Update::default()
            },
            &Hosts::default(),
        )
        .expect_err("a file URL is no endpoint");
        assert_eq!(
            refused.to_string(),
            "anthropic_base_url: The URL must use http or https."
        );

        let blank = Update {
            litellm_host: Some(""),
            openai_base_url: Some("  "),
            anthropic_base_url: Some(""),
            ..Update::default()
        };
        assert!(validate(&blank, &Hosts::default()).is_ok());
    }

    fn keyed_beside(url: Option<&str>) -> SavedEndpoints {
        SavedEndpoints {
            litellm_host: url.map(str::to_string),
            litellm_key: true,
            openai_base_url: url.map(str::to_string),
            openai_api_key: true,
            anthropic_base_url: url.map(str::to_string),
            anthropic_api_key: true,
            routed: true,
        }
    }

    fn url_update(pair: Pair, url: &str) -> Update<'_> {
        match pair {
            Pair::Litellm => Update {
                litellm_host: Some(url),
                ..Update::default()
            },
            Pair::OpenAI => Update {
                openai_base_url: Some(url),
                ..Update::default()
            },
            Pair::Anthropic => Update {
                anthropic_base_url: Some(url),
                ..Update::default()
            },
        }
    }

    #[test]
    fn only_an_organization_update_that_moves_a_lent_url_moves_its_pair() {
        let first = keyed_beside(Some("http://first.example"));
        for pair in Pair::ALL {
            let index = Pair::ALL.iter().position(|each| *each == pair).unwrap_or(0);
            assert!(moved_pairs(&url_update(pair, "http://second.example"), &first)[index]);
            assert!(moved_pairs(&url_update(pair, ""), &first)[index]);
            assert!(!moved_pairs(&url_update(pair, "http://first.example"), &first)[index]);
            assert!(
                moved_pairs(
                    &url_update(pair, "http://first.example"),
                    &SavedEndpoints {
                        routed: false,
                        ..keyed_beside(Some("http://first.example"))
                    }
                )[index],
                "a URL saved before routing starts being lent"
            );
        }
        assert_eq!(moved_pairs(&Update::default(), &first), [false; 3]);
    }

    #[test]
    fn a_saved_key_never_follows_a_new_url_it_was_not_saved_beside() {
        let nothing = SavedEndpoints::default();
        let open = Hosts::default();
        for pair in Pair::ALL {
            for (saved, url) in [
                (Some("http://first.example"), "http://second.example"),
                (Some("http://first.example"), ""),
                (None, "http://second.example"),
            ] {
                let refused = validate_keys(
                    &url_update(pair, url),
                    &keyed_beside(saved),
                    &nothing,
                    &open,
                )
                .expect_err("the key followed the URL");
                assert_eq!(
                    refused.to_string(),
                    format!("{}: {RE_ENTER_KEY}", pair.url_field()),
                    "{pair:?} {saved:?} -> {url:?}"
                );
            }

            let unchanged = url_update(pair, " http://first.example ");
            assert!(
                validate_keys(
                    &unchanged,
                    &keyed_beside(Some("http://first.example")),
                    &nothing,
                    &open
                )
                .is_ok()
            );
            let keyless = url_update(pair, "http://second.example");
            let without_key = SavedEndpoints {
                litellm_host: Some("http://first.example".to_string()),
                openai_base_url: Some("http://first.example".to_string()),
                anthropic_base_url: Some("http://first.example".to_string()),
                ..SavedEndpoints::default()
            };
            assert!(
                validate_keys(&keyless, &without_key, &nothing, &open).is_ok(),
                "{pair:?}"
            );
        }

        let re_entered = Update {
            openai_base_url: Some("http://second.example"),
            openai_api_key: Some("sk-again"),
            ..Update::default()
        };
        let cleared = Update {
            openai_base_url: Some("http://second.example"),
            openai_api_key: Some(""),
            ..Update::default()
        };
        for update in [re_entered, cleared] {
            assert!(
                validate_keys(
                    &update,
                    &keyed_beside(Some("http://first.example")),
                    &nothing,
                    &open
                )
                .is_ok()
            );
        }
    }

    #[test]
    fn a_key_without_a_url_is_refused_only_when_its_default_host_is_not_listed() {
        const KEY: &str = "sk-never-echoed";
        let nothing = SavedEndpoints::default();
        let restricted = Hosts::parse("llm.corp.example");
        for (update, field, host) in [
            (
                Update {
                    openai_api_key: Some(KEY),
                    ..Update::default()
                },
                "openai_api_key",
                "api.openai.com",
            ),
            (
                Update {
                    anthropic_api_key: Some(KEY),
                    ..Update::default()
                },
                "anthropic_api_key",
                "api.anthropic.com",
            ),
        ] {
            let refused = validate_keys(&update, &nothing, &nothing, &restricted)
                .expect_err("the key would go to an unlisted default host")
                .to_string();
            assert!(refused.starts_with(field), "{refused}");
            assert!(refused.contains(host), "{refused}");
            assert!(!refused.contains(KEY), "{refused}");

            assert!(validate_keys(&update, &nothing, &nothing, &Hosts::default()).is_ok());
            let organization = SavedEndpoints {
                openai_base_url: Some("https://llm.corp.example/v1".to_string()),
                anthropic_base_url: Some("https://llm.corp.example/v1".to_string()),
                ..SavedEndpoints::default()
            };
            assert!(
                validate_keys(&update, &nothing, &organization, &restricted).is_ok(),
                "a workspace key alone goes to its organization's URL"
            );
        }

        let litellm = Update {
            litellm_key: Some(KEY),
            ..Update::default()
        };
        assert!(validate_keys(&litellm, &nothing, &nothing, &restricted).is_ok());
    }

    #[test]
    fn validate_refuses_a_metadata_url_and_a_host_the_instance_does_not_list() {
        let metadata = validate(
            &Update {
                litellm_host: Some("http://169.254.169.254/latest"),
                ..Update::default()
            },
            &Hosts::default(),
        )
        .expect_err("the metadata service is no endpoint");
        assert_eq!(
            metadata.to_string(),
            "litellm_host: The URL must not point at a link-local or cloud metadata address."
        );

        let hosts = Hosts::parse(".corp.example");
        let unlisted = validate(
            &Update {
                openai_base_url: Some("https://gateway.example/v1"),
                ..Update::default()
            },
            &hosts,
        )
        .expect_err("the instance lists only its own domain");
        assert_eq!(
            unlisted.to_string(),
            "openai_base_url: The URL's host is not one this instance allows endpoints on."
        );
        assert!(
            validate(
                &Update {
                    openai_base_url: Some("https://llm.corp.example/v1"),
                    ..Update::default()
                },
                &hosts,
            )
            .is_ok()
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
