//! A GitHub App's identity and signing key, sealed at rest and open in memory.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zone_core::secret::{MasterKey, SecretValue, encryption};

use super::error::GithubAppError;
use super::identifier::{ApplicationId, InstallationId};

/// The key a source's `config` column holds its GitHub App settings under.
pub const CONFIGURATION_KEY: &str = "github_app";

/// A GitHub App configuration as it sits in the database.
///
/// `private_key` holds an `ENC[v1:...]` envelope, never PEM text. [`open`]
/// refuses anything else, so a row written without encryption fails loudly
/// instead of signing with a key that was stored in the clear.
///
/// [`open`]: StoredConfiguration::open
#[derive(Clone, Serialize, Deserialize)]
pub struct StoredConfiguration {
    application_id: ApplicationId,
    installation_id: InstallationId,
    private_key: String,
}

impl StoredConfiguration {
    /// Seal a private key into an envelope this configuration can be stored as.
    pub fn seal(
        application_id: ApplicationId,
        installation_id: InstallationId,
        private_key: &SecretValue,
        master_key: &MasterKey,
    ) -> Result<Self, GithubAppError> {
        Ok(Self {
            application_id,
            installation_id,
            private_key: encryption::encrypt(private_key, master_key)
                .map_err(|_| GithubAppError::Sealing)?,
        })
    }

    /// Unseal the private key so a JWT can be signed with it.
    pub fn open(&self, master_key: &MasterKey) -> Result<AppConfiguration, GithubAppError> {
        if !encryption::is_encrypted(&self.private_key) {
            return Err(GithubAppError::Unsealing);
        }

        Ok(AppConfiguration {
            application_id: self.application_id,
            installation_id: self.installation_id,
            private_key: encryption::decrypt(&self.private_key, master_key)
                .map_err(|_| GithubAppError::Unsealing)?,
        })
    }

    /// The configuration held under [`CONFIGURATION_KEY`] in a source's config.
    ///
    /// A source with no GitHub App settings is not an error; it is a source
    /// that authenticates some other way.
    pub fn read(config: &Value) -> Result<Option<Self>, GithubAppError> {
        match config.get(CONFIGURATION_KEY) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => serde_json::from_value(value.clone())
                .map(Some)
                .map_err(|_| GithubAppError::Configuration),
        }
    }

    /// This configuration written into a source's config, ready to store.
    pub fn write(&self, config: &mut Value) -> Result<(), GithubAppError> {
        let object = config
            .as_object_mut()
            .ok_or(GithubAppError::Configuration)?;

        object.insert(
            CONFIGURATION_KEY.to_string(),
            serde_json::to_value(self).map_err(|_| GithubAppError::Configuration)?,
        );
        Ok(())
    }

    pub fn application_id(&self) -> ApplicationId {
        self.application_id
    }

    pub fn installation_id(&self) -> InstallationId {
        self.installation_id
    }
}

impl fmt::Debug for StoredConfiguration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoredConfiguration")
            .field("application_id", &self.application_id)
            .field("installation_id", &self.installation_id)
            .field("private_key", &zone_core::secret::REDACTED)
            .finish()
    }
}

/// A GitHub App configuration with its private key unsealed and ready to sign.
#[derive(Debug, Clone)]
pub struct AppConfiguration {
    application_id: ApplicationId,
    installation_id: InstallationId,
    private_key: SecretValue,
}

impl AppConfiguration {
    pub fn new(
        application_id: ApplicationId,
        installation_id: InstallationId,
        private_key: SecretValue,
    ) -> Self {
        Self {
            application_id,
            installation_id,
            private_key,
        }
    }

    pub fn application_id(&self) -> ApplicationId {
        self.application_id
    }

    pub fn installation_id(&self) -> InstallationId {
        self.installation_id
    }

    pub fn private_key(&self) -> &SecretValue {
        &self.private_key
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::github_app::testing::TEST_PRIVATE_KEY;

    fn sealed(master_key: &MasterKey) -> StoredConfiguration {
        StoredConfiguration::seal(
            ApplicationId::new(12345),
            InstallationId::new(67890),
            &SecretValue::new(TEST_PRIVATE_KEY),
            master_key,
        )
        .expect("seal")
    }

    #[test]
    fn sealing_and_opening_round_trips_the_private_key() {
        let master_key = MasterKey::generate();
        let opened = sealed(&master_key).open(&master_key).expect("open");

        assert_eq!(opened.private_key().expose(), TEST_PRIVATE_KEY);
        assert_eq!(opened.application_id(), ApplicationId::new(12345));
        assert_eq!(opened.installation_id(), InstallationId::new(67890));
    }

    #[test]
    fn the_stored_private_key_is_an_envelope_not_pem() {
        let stored = sealed(&MasterKey::generate());
        let encoded = serde_json::to_string(&stored).expect("serialise");

        assert!(encoded.contains("ENC[v1:"));
        assert!(!encoded.contains("BEGIN RSA PRIVATE KEY"));
        assert!(!encoded.contains(&TEST_PRIVATE_KEY[40..80]));
    }

    #[test]
    fn opening_with_the_wrong_key_fails() {
        let stored = sealed(&MasterKey::generate());
        assert!(matches!(
            stored.open(&MasterKey::generate()),
            Err(GithubAppError::Unsealing)
        ));
    }

    #[test]
    fn a_plaintext_private_key_is_refused_rather_than_used() {
        let stored: StoredConfiguration = serde_json::from_value(serde_json::json!({
            "application_id": 12345,
            "installation_id": 67890,
            "private_key": TEST_PRIVATE_KEY,
        }))
        .expect("deserialise");

        assert!(matches!(
            stored.open(&MasterKey::generate()),
            Err(GithubAppError::Unsealing)
        ));
    }

    #[test]
    fn debug_redacts_both_the_sealed_and_the_open_key() {
        let master_key = MasterKey::generate();
        let stored = sealed(&master_key);
        let rendered = format!("{stored:?}");

        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains("ENC[v1:"));
        assert!(!rendered.contains("BEGIN RSA PRIVATE KEY"));

        let opened = format!("{:?}", stored.open(&master_key).expect("open"));
        assert!(opened.contains("[REDACTED]"));
        assert!(!opened.contains("BEGIN RSA PRIVATE KEY"));
    }

    #[test]
    fn reading_a_source_without_app_settings_yields_nothing() {
        let config = serde_json::json!({ "owner": "zone-dev", "repo": "zone" });
        assert!(StoredConfiguration::read(&config).expect("read").is_none());

        let explicit_null = serde_json::json!({ "github_app": null });
        assert!(
            StoredConfiguration::read(&explicit_null)
                .expect("read")
                .is_none()
        );
    }

    #[test]
    fn writing_then_reading_preserves_the_rest_of_the_config() {
        let master_key = MasterKey::generate();
        let mut config = serde_json::json!({ "owner": "zone-dev", "repo": "zone" });
        sealed(&master_key).write(&mut config).expect("write");

        assert_eq!(config["owner"], "zone-dev");
        let read = StoredConfiguration::read(&config)
            .expect("read")
            .expect("present");
        assert_eq!(read.installation_id(), InstallationId::new(67890));
        assert_eq!(
            read.open(&master_key).expect("open").private_key().expose(),
            TEST_PRIVATE_KEY
        );
    }

    #[test]
    fn malformed_app_settings_are_an_error_not_an_absence() {
        let config = serde_json::json!({ "github_app": { "application_id": 12345 } });
        assert!(matches!(
            StoredConfiguration::read(&config),
            Err(GithubAppError::Configuration)
        ));
    }
}
