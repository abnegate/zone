use std::fmt;

use sqlx::PgConnection;
use sqlx::PgPool;
use uuid::Uuid;

use crate::sync::Provider;

use super::ai_key::AiKey;
use super::ai_scope::AiScope;
use super::error::SealingError;
use super::policy::Policy;

const AGENT_LOGINS: &str = "agent_logins";
const PROJECTS: &str = "projects";
const SOURCES: &str = "sources";
const SYNC_CONFIGS: &str = "sync_configs";

const GITHUB_APP_PRIVATE_KEY: &str = "config->'github_app'->>'private_key'";
const LINEAR_API_KEY: &str = "config->>'api_key'";
const SYNC_TOKEN: &str = "config->>'token'";

const REPLACE_SYNC_TOKEN: &str = "UPDATE sync_configs SET config = jsonb_set(config, '{token}', to_jsonb($2::text)) WHERE id = $1 AND provider = $4 AND config->>'token' = $3";

/// A stored value one row of a credential column holds.
pub type Row = (Uuid, String);

/// A column that holds a credential, sealed or not.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Column {
    /// `sources.credentials_encrypted`.
    SourceCredentials,
    /// `sync_configs.webhook_secret_encrypted`.
    WebhookSecret,
    /// `projects.github_access_token`.
    ProjectToken,
    /// `agent_logins.credential`, the sealed `Tokens` of a sign-in.
    AgentLoginCredential,
    /// The `token` in a GitHub `sync_configs.config`.
    GithubSyncToken,
    /// The GitHub App `private_key` in a `sources.config`.
    GithubAppPrivateKey,
    /// The `api_key` in a Linear `sync_configs.config`.
    LinearApiKey,
    /// A provider key in an AI settings table.
    AiSetting { scope: AiScope, key: AiKey },
}

impl Column {
    /// Every credential column, in report order.
    pub const ALL: &[Self] = &[
        Self::SourceCredentials,
        Self::WebhookSecret,
        Self::ProjectToken,
        Self::AgentLoginCredential,
        Self::GithubSyncToken,
        Self::GithubAppPrivateKey,
        Self::LinearApiKey,
        Self::ai(AiScope::Organization, AiKey::LitellmKey),
        Self::ai(AiScope::Organization, AiKey::OpenaiApiKey),
        Self::ai(AiScope::Organization, AiKey::AnthropicApiKey),
        Self::ai(AiScope::Organization, AiKey::RunpodApiKey),
        Self::ai(AiScope::Organization, AiKey::BedrockAccessKey),
        Self::ai(AiScope::Organization, AiKey::BedrockSecretKey),
        Self::ai(AiScope::Workspace, AiKey::LitellmKey),
        Self::ai(AiScope::Workspace, AiKey::OpenaiApiKey),
        Self::ai(AiScope::Workspace, AiKey::AnthropicApiKey),
        Self::ai(AiScope::Workspace, AiKey::RunpodApiKey),
        Self::ai(AiScope::Workspace, AiKey::BedrockAccessKey),
        Self::ai(AiScope::Workspace, AiKey::BedrockSecretKey),
    ];

    /// Every table sealing writes to, so every table the backup must hold the data of.
    pub fn sealed_tables() -> &'static [&'static str] {
        &[AGENT_LOGINS, PROJECTS, SOURCES, SYNC_CONFIGS]
    }

    const fn ai(scope: AiScope, key: AiKey) -> Self {
        Self::AiSetting { scope, key }
    }

    /// The table the column belongs to.
    pub fn table(&self) -> &'static str {
        match self {
            Self::SourceCredentials | Self::GithubAppPrivateKey => SOURCES,
            Self::WebhookSecret | Self::GithubSyncToken | Self::LinearApiKey => SYNC_CONFIGS,
            Self::ProjectToken => PROJECTS,
            Self::AgentLoginCredential => AGENT_LOGINS,
            Self::AiSetting { scope, .. } => scope.table(),
        }
    }

    /// The SQL expression that reads the value: a column name, or a path into JSON.
    pub fn name(&self) -> &'static str {
        match self {
            Self::SourceCredentials => "credentials_encrypted",
            Self::WebhookSecret => "webhook_secret_encrypted",
            Self::ProjectToken => "github_access_token",
            Self::AgentLoginCredential => "credential",
            Self::GithubSyncToken => SYNC_TOKEN,
            Self::GithubAppPrivateKey => GITHUB_APP_PRIVATE_KEY,
            Self::LinearApiKey => LINEAR_API_KEY,
            Self::AiSetting { key, .. } => key.column(),
        }
    }

    /// What the column's readers accept.
    pub fn policy(&self) -> Policy {
        match self {
            Self::SourceCredentials | Self::AgentLoginCredential => Policy::Strict,
            Self::WebhookSecret => Policy::Mixed,
            Self::ProjectToken | Self::GithubSyncToken => Policy::Lenient,
            Self::GithubAppPrivateKey | Self::LinearApiKey | Self::AiSetting { .. } => {
                Policy::Counted
            }
        }
    }

    /// The sync provider whose configs the column is read from, when it is one.
    pub fn provider(&self) -> Option<Provider> {
        match self {
            Self::GithubSyncToken => Some(Provider::GitHub),
            Self::LinearApiKey => Some(Provider::Linear),
            _ => None,
        }
    }

    /// Up to `limit` rows holding a value, ordered by id and starting after `after`.
    pub async fn page(
        &self,
        pool: &PgPool,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<Row>, SealingError> {
        let table = self.table();
        let name = self.name();
        let provided = if self.provider().is_some() {
            " AND provider = $3"
        } else {
            ""
        };
        let sql = format!(
            "SELECT id, {name} FROM {table} WHERE ($1::uuid IS NULL OR id > $1) AND {name} IS NOT NULL{provided} ORDER BY id LIMIT $2"
        );
        let query = sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(sql))
            .bind(after)
            .bind(limit as i64);
        let query = match self.provider() {
            Some(provider) => query.bind(provider.as_str()),
            None => query,
        };
        Ok(query.fetch_all(pool).await?)
    }

    /// Write `sealed` over row `id`'s value if the value is still `old`. False when the row
    /// changed since it was read, or is gone.
    pub async fn replace(
        &self,
        connection: &mut PgConnection,
        id: Uuid,
        old: &str,
        sealed: &str,
    ) -> Result<bool, SealingError> {
        let query = match self {
            Self::SourceCredentials
            | Self::WebhookSecret
            | Self::ProjectToken
            | Self::AgentLoginCredential => {
                let table = self.table();
                let name = self.name();
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "UPDATE {table} SET {name} = $2 WHERE id = $1 AND {name} = $3"
                )))
                .bind(id)
                .bind(sealed)
                .bind(old)
            }
            Self::GithubSyncToken => sqlx::query(REPLACE_SYNC_TOKEN)
                .bind(id)
                .bind(sealed)
                .bind(old)
                .bind(Provider::GitHub.as_str()),
            Self::GithubAppPrivateKey | Self::LinearApiKey | Self::AiSetting { .. } => {
                return Err(SealingError::Counted(*self));
            }
        };
        Ok(query.execute(connection).await?.rows_affected() == 1)
    }
}

impl fmt::Display for Column {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.table(), self.name())?;
        match self.provider() {
            Some(provider) => write!(formatter, " ({})", provider.as_str()),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::Column;
    use super::Policy;

    #[test]
    fn the_sealed_tables_are_exactly_the_tables_sealing_writes_to() {
        let written: BTreeSet<&str> = Column::ALL
            .iter()
            .filter(|column| column.policy().writes())
            .map(Column::table)
            .collect();
        let sealed: BTreeSet<&str> = Column::sealed_tables().iter().copied().collect();
        assert_eq!(written, sealed);
    }

    #[test]
    fn every_column_is_listed_once() {
        let distinct: BTreeSet<Column> = Column::ALL.iter().copied().collect();
        assert_eq!(distinct.len(), Column::ALL.len());
        assert_eq!(
            Column::ALL
                .iter()
                .filter(|column| matches!(column, Column::AiSetting { .. }))
                .count(),
            12
        );
    }

    #[test]
    fn plaintext_is_sealed_only_in_the_project_token_and_the_github_sync_token() {
        let lenient: Vec<Column> = Column::ALL
            .iter()
            .copied()
            .filter(|column| column.policy() == Policy::Lenient)
            .collect();
        assert_eq!(lenient, [Column::ProjectToken, Column::GithubSyncToken]);
    }
}
