use abnegate_secret::{MasterKey, SecretValue};
use serde_json::{Map, Value, json};
use uuid::Uuid;
use zone_enterprise::sso::{DisplayName, Domains, Error, Fields, Protocol, Provider, saml};

use super::failure::Failure;
use super::imported::Imported;
use super::revision::Revision;
use crate::db::audit::{actions, resources};
use crate::db::sessions;
use crate::db::sso::{
    Connection, NewProvider, ProviderChanges, ProviderError, ProviderRecord, attempts, identities,
    providers, settings,
};
use crate::db::users::UserRow;
use crate::routes::common::{AuditEvent, audit};
use crate::state::AppState;

const CLIENT_SECRET_CHANGED: &str = "client_secret_changed";
const IDENTITIES_CLEARED: &str = "identities_cleared";

/// `metadata_url` and `metadata` are the two SAML metadata sources, of which a SAML provider
/// takes exactly one and an OIDC provider neither.
pub async fn create(
    state: &AppState,
    actor: &UserRow,
    fields: &Fields<'_>,
    metadata_url: Option<&str>,
    metadata: Option<&str>,
    enabled: bool,
) -> Result<ProviderRecord, Failure> {
    let new = match fields.protocol {
        Protocol::Oidc => {
            let saml = Revision {
                metadata_url,
                metadata,
                ..Revision::default()
            };
            if saml.mismatches(Protocol::Oidc) {
                return Err(Error::ProtocolMismatch.into());
            }
            let provider = Provider::parse(fields)?;
            state.oidc().discover(&provider.issuer).await?;
            NewProvider::new(&provider, enabled)
        }
        Protocol::Saml => {
            admissible(
                fields.display_name,
                fields.allowed_domains,
                fields.require_verified_email,
            )?;
            let imported = Imported::read(state.saml(), metadata_url, metadata)
                .await?
                .ok_or_else(Imported::missing)?;
            let provider = saml::Provider::from_metadata(
                fields.display_name,
                &imported.metadata,
                fields.allowed_domains,
                fields.require_verified_email,
            )?;
            NewProvider::saml(&provider, imported.metadata_url, enabled)
        }
    };
    let record = providers::create(state.db(), state.master_key(), &new).await?;
    let mut created = recorded(&record);
    if record.protocol == Protocol::Oidc {
        created.insert(CLIENT_SECRET_CHANGED.into(), Value::Bool(true));
    }
    record_change(
        state,
        actor,
        actions::SSO_PROVIDER_CREATED,
        record.id,
        None,
        Some(Value::Object(created)),
    )
    .await;
    Ok(record)
}

/// `entitled` says whether the license allows changes. Without one, the route lets through only
/// a revision that disables the provider or refreshes its metadata, and a refresh still may not
/// change the entity ID, because that unlinks every account.
pub async fn update(
    state: &AppState,
    actor: &UserRow,
    id: Uuid,
    revision: &Revision<'_>,
    entitled: bool,
) -> Result<ProviderRecord, Failure> {
    let key = state.master_key();
    let current = providers::get(state.db(), id)
        .await?
        .ok_or(Error::ProviderNotFound)?;
    if revision.mismatches(current.protocol) {
        return Err(Error::ProtocolMismatch.into());
    }
    let imported = match current.protocol {
        Protocol::Oidc => {
            let candidate = revised(&current, revision, key)?;
            if rediscovers(&current, &candidate) {
                state.oidc().discover(&candidate.issuer).await?;
            }
            None
        }
        Protocol::Saml => {
            let fields = revision.saml_fields(&current)?;
            admissible(
                fields.display_name,
                fields.allowed_domains,
                fields.require_verified_email,
            )?;
            Imported::read(state.saml(), revision.metadata_url, revision.metadata).await?
        }
    };

    let mut transaction = state.db().begin().await?;
    if revision.enabled == Some(false) {
        settings::lock(&mut transaction).await?;
    }
    let locked = providers::lock(&mut transaction, id)
        .await?
        .ok_or(Error::ProviderNotFound)?;
    let changes = changes(&locked, revision, imported.as_ref(), key)?;
    if changes.is_empty() {
        transaction.rollback().await?;
        return Ok(locked);
    }
    if !entitled && changes.renames_subjects() {
        return Err(Failure::Unentitled);
    }
    if changes.enabled == Some(false) && last_enabled(&mut transaction).await? {
        return Err(Error::ProviderRequired.into());
    }
    let updated = providers::update(&mut *transaction, key, id, &changes)
        .await?
        .ok_or(Error::ProviderNotFound)?;
    let cleared = if changes.renames_subjects() {
        Some(identities::clear(&mut *transaction, id).await?)
    } else {
        None
    };
    if changes.voids_attempts() {
        attempts::clear(&mut *transaction, id).await?;
    }
    if changes.enabled == Some(false) {
        sessions::revoke_provider_sessions(&mut *transaction, id).await?;
    }
    transaction.commit().await?;
    if changes.issuer.is_some() {
        forget(state, &locked);
    }

    let (before, mut after) = difference(recorded(&locked), recorded(&updated));
    if changes.client_secret.is_some() {
        after.insert(CLIENT_SECRET_CHANGED.into(), Value::Bool(true));
    }
    if let Some(cleared) = cleared {
        after.insert(IDENTITIES_CLEARED.into(), Value::from(cleared));
    }
    record_change(
        state,
        actor,
        actions::SSO_PROVIDER_UPDATED,
        id,
        Some(Value::Object(before)),
        Some(Value::Object(after)),
    )
    .await;
    Ok(updated)
}

pub async fn delete(state: &AppState, actor: &UserRow, id: Uuid) -> Result<(), Failure> {
    let mut transaction = state.db().begin().await?;
    settings::lock(&mut transaction).await?;
    let locked = providers::lock(&mut transaction, id)
        .await?
        .ok_or(Error::ProviderNotFound)?;
    if locked.enabled && last_enabled(&mut transaction).await? {
        return Err(Error::ProviderRequired.into());
    }
    providers::delete(&mut *transaction, id).await?;
    transaction.commit().await?;
    forget(state, &locked);
    record_change(
        state,
        actor,
        actions::SSO_PROVIDER_DELETED,
        id,
        Some(Value::Object(recorded(&locked))),
        None,
    )
    .await;
    Ok(())
}

/// The SHA-256 fingerprints of a SAML provider's pinned certificates; an OIDC provider has none.
pub fn fingerprints(record: &ProviderRecord) -> Vec<String> {
    let Connection::Saml { certificates, .. } = &record.connection else {
        return Vec::new();
    };
    certificates
        .iter()
        .filter_map(|stored| match saml::Certificate::parse(stored) {
            Ok(certificate) => Some(certificate.fingerprint().to_owned()),
            Err(error) => {
                tracing::warn!(provider = %record.id, %error, "A stored SAML certificate didn't parse");
                None
            }
        })
        .collect()
}

/// The settings a SAML provider needs that don't come from its metadata, checked before the
/// metadata URL is fetched so that a request already bound to fail sends nothing out.
fn admissible(
    display_name: &str,
    allowed_domains: &[String],
    require_verified_email: bool,
) -> Result<(), Error> {
    DisplayName::parse(display_name)?;
    if Domains::parse(allowed_domains)?.is_empty() {
        return Err(Error::DomainRequired);
    }
    if require_verified_email {
        return Err(Error::VerifiedEmailUnsupported);
    }
    Ok(())
}

/// Only OIDC caches anything per issuer: its discovery document and signing keys.
fn forget(state: &AppState, record: &ProviderRecord) {
    if record.protocol == Protocol::Oidc {
        state.oidc().forget(&record.issuer);
    }
}

fn revised(
    record: &ProviderRecord,
    revision: &Revision<'_>,
    key: &MasterKey,
) -> Result<Provider, Failure> {
    let client_secret = revision.client_secret(record, key)?;
    Ok(Provider::parse(&revision.fields(record, &client_secret)?)?)
}

fn saml_revised(
    record: &ProviderRecord,
    revision: &Revision<'_>,
    imported: Option<&Imported>,
) -> Result<saml::Provider, Failure> {
    let fields = revision.saml_fields(record)?;
    let provider = match imported {
        Some(imported) => saml::Provider::from_metadata(
            fields.display_name,
            &imported.metadata,
            fields.allowed_domains,
            fields.require_verified_email,
        )?,
        None => saml::Provider::parse(&fields)?,
    };
    Ok(provider)
}

async fn last_enabled(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<bool, Failure> {
    Ok(settings::required(&mut **transaction).await?
        && providers::count_enabled(&mut **transaction).await? <= 1)
}

fn rediscovers(record: &ProviderRecord, provider: &Provider) -> bool {
    record.issuer != provider.issuer.as_str()
        || record.client_id() != Some(provider.client_id.as_str())
}

fn changes(
    record: &ProviderRecord,
    revision: &Revision<'_>,
    imported: Option<&Imported>,
    key: &MasterKey,
) -> Result<ProviderChanges, Failure> {
    match record.protocol {
        Protocol::Oidc => Ok(oidc_changes(
            record,
            &revised(record, revision, key)?,
            revision.enabled(record),
            revision.replaces_secret(record, key),
        )),
        Protocol::Saml => saml_changes(
            record,
            &saml_revised(record, revision, imported)?,
            imported,
            revision.enabled(record),
        ),
    }
}

fn oidc_changes(
    record: &ProviderRecord,
    provider: &Provider,
    enabled: bool,
    replaces_secret: bool,
) -> ProviderChanges {
    let differs = |stored: &str, revised: &str| (stored != revised).then(|| revised.to_owned());
    ProviderChanges {
        display_name: differs(&record.display_name, provider.display_name.as_str()),
        issuer: differs(&record.issuer, provider.issuer.as_str()),
        client_id: (record.client_id() != Some(provider.client_id.as_str()))
            .then(|| provider.client_id.as_str().to_owned()),
        client_secret: replaces_secret.then(|| SecretValue::new(provider.client_secret.expose())),
        allowed_domains: (record.allowed_domains != provider.policy.domains.as_slice())
            .then(|| provider.policy.domains.as_slice().to_vec()),
        require_verified_email: (record.require_verified_email
            != provider.policy.require_verified_email)
            .then_some(provider.policy.require_verified_email),
        enabled: (record.enabled != enabled).then_some(enabled),
        ..ProviderChanges::default()
    }
}

/// The metadata URL changes only with new metadata: a URL replaces it, and a pasted document
/// clears it, because the stored values no longer come from that URL.
fn saml_changes(
    record: &ProviderRecord,
    provider: &saml::Provider,
    imported: Option<&Imported>,
    enabled: bool,
) -> Result<ProviderChanges, Failure> {
    let Connection::Saml {
        sso_url,
        certificates,
        metadata_url,
    } = &record.connection
    else {
        return Err(ProviderError::Protocol(record.id, record.protocol.to_string()).into());
    };
    let differs = |stored: &str, revised: &str| (stored != revised).then(|| revised.to_owned());
    let pinned: Vec<String> = provider
        .certificates
        .iter()
        .map(saml::Certificate::encoded)
        .collect();
    Ok(ProviderChanges {
        display_name: differs(&record.display_name, provider.display_name.as_str()),
        issuer: differs(&record.issuer, provider.entity_id.as_str()),
        sso_url: differs(sso_url, provider.sso_url.as_str()),
        certificates: (*certificates != pinned).then_some(pinned),
        metadata_url: imported
            .map(|imported| &imported.metadata_url)
            .filter(|revised| *revised != metadata_url)
            .cloned(),
        allowed_domains: (record.allowed_domains != provider.policy.domains.as_slice())
            .then(|| provider.policy.domains.as_slice().to_vec()),
        require_verified_email: (record.require_verified_email
            != provider.policy.require_verified_email)
            .then_some(provider.policy.require_verified_email),
        enabled: (record.enabled != enabled).then_some(enabled),
        ..ProviderChanges::default()
    })
}

pub fn recorded(record: &ProviderRecord) -> Map<String, Value> {
    let entry = match &record.connection {
        Connection::Oidc { client_id, .. } => json!({
            "protocol": record.protocol,
            "display_name": record.display_name,
            "issuer": record.issuer,
            "client_id": client_id,
            "allowed_domains": record.allowed_domains,
            "require_verified_email": record.require_verified_email,
            "enabled": record.enabled,
        }),
        Connection::Saml {
            sso_url,
            metadata_url,
            ..
        } => json!({
            "protocol": record.protocol,
            "display_name": record.display_name,
            "issuer": record.issuer,
            "sso_url": sso_url,
            "metadata_url": metadata_url,
            "certificate_fingerprints": fingerprints(record),
            "allowed_domains": record.allowed_domains,
            "require_verified_email": record.require_verified_email,
            "enabled": record.enabled,
        }),
    };
    let Value::Object(map) = entry else {
        unreachable!("a json! object literal is an object");
    };
    map
}

fn difference(
    before: Map<String, Value>,
    after: Map<String, Value>,
) -> (Map<String, Value>, Map<String, Value>) {
    before
        .into_iter()
        .filter_map(|(key, old)| {
            let new = after.get(&key)?;
            (old != *new).then(|| ((key.clone(), old), (key, new.clone())))
        })
        .unzip()
}

async fn record_change(
    state: &AppState,
    actor: &UserRow,
    action: &str,
    id: Uuid,
    old_values: Option<Value>,
    new_values: Option<Value>,
) {
    audit(
        state.db(),
        AuditEvent {
            organization_id: None,
            workspace_id: None,
            actor_id: actor.id,
            actor_email: &actor.email,
            action,
            resource_type: resources::SSO_PROVIDER,
            resource_id: Some(id),
            old_values,
            new_values,
        },
    )
    .await;
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use abnegate_secret::encrypt_value;
    use chrono::Utc;
    use zone_enterprise::sso::saml::testing::{
        CERTIFICATE, ENTITY_ID, OTHER_CERTIFICATE, SSO_URL, metadata,
    };

    use super::*;
    use crate::services::sso::imported::{AMBIGUOUS, MISSING};

    const SECRET: &str = "s3cret-client-value";
    const METADATA_URL: &str = "https://idp.example.com/app/metadata";
    const OTHER_ENTITY_ID: &str = "https://idp.example.com/other";

    static KEY: LazyLock<MasterKey> = LazyLock::new(|| MasterKey::generate().expect("a key"));

    fn record() -> ProviderRecord {
        ProviderRecord {
            id: Uuid::new_v4(),
            protocol: Protocol::Oidc,
            display_name: "Okta".into(),
            issuer: "https://acme.okta.com".into(),
            connection: Connection::Oidc {
                client_id: "0oa1".into(),
                sealed_secret: encrypt_value(&SecretValue::new(SECRET), &KEY)
                    .expect("a sealed secret"),
            },
            allowed_domains: vec!["acme.com".into()],
            require_verified_email: true,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn saml_record() -> ProviderRecord {
        ProviderRecord {
            id: Uuid::new_v4(),
            protocol: Protocol::Saml,
            display_name: "Okta".into(),
            issuer: ENTITY_ID.into(),
            connection: Connection::Saml {
                sso_url: SSO_URL.into(),
                certificates: vec![CERTIFICATE.into()],
                metadata_url: Some(METADATA_URL.into()),
            },
            allowed_domains: vec!["acme.com".into()],
            require_verified_email: false,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn imported(entity_id: &str, certificates: &[&str], metadata_url: Option<&str>) -> Imported {
        let document = metadata(entity_id, SSO_URL, certificates);
        Imported {
            metadata: saml::Metadata::parse(document.as_bytes()).expect("test metadata parses"),
            metadata_url: metadata_url.map(str::to_owned),
        }
    }

    fn fingerprint(certificate: &str) -> String {
        saml::Certificate::parse(certificate)
            .expect("a test certificate")
            .fingerprint()
            .to_owned()
    }

    fn provider(revision: &Revision<'_>, record: &ProviderRecord) -> Provider {
        revised(record, revision, &KEY).unwrap()
    }

    fn changed(revision: &Revision<'_>, record: &ProviderRecord) -> ProviderChanges {
        changes(record, revision, None, &KEY).unwrap()
    }

    fn saml_changed(
        revision: &Revision<'_>,
        record: &ProviderRecord,
        imported: Option<&Imported>,
    ) -> Result<ProviderChanges, Failure> {
        changes(record, revision, imported, &KEY)
    }

    #[test]
    fn the_record_never_carries_the_secret() {
        let recorded = Value::Object(recorded(&record()));
        assert_eq!(
            recorded,
            json!({
                "protocol": "oidc",
                "display_name": "Okta",
                "issuer": "https://acme.okta.com",
                "client_id": "0oa1",
                "allowed_domains": ["acme.com"],
                "require_verified_email": true,
                "enabled": true,
            })
        );
        assert!(!recorded.to_string().contains(SECRET));
    }

    #[test]
    fn a_saml_record_carries_its_connection_and_certificate_fingerprints() {
        let recorded = Value::Object(recorded(&saml_record()));
        assert_eq!(
            recorded,
            json!({
                "protocol": "saml",
                "display_name": "Okta",
                "issuer": ENTITY_ID,
                "sso_url": SSO_URL,
                "metadata_url": METADATA_URL,
                "certificate_fingerprints": [fingerprint(CERTIFICATE)],
                "allowed_domains": ["acme.com"],
                "require_verified_email": false,
                "enabled": true,
            })
        );
        assert!(!recorded.to_string().contains(CERTIFICATE));
    }

    #[test]
    fn a_saml_record_from_pasted_metadata_records_no_metadata_url() {
        let record = ProviderRecord {
            connection: Connection::Saml {
                sso_url: SSO_URL.into(),
                certificates: vec![CERTIFICATE.into()],
                metadata_url: None,
            },
            ..saml_record()
        };
        assert_eq!(recorded(&record)["metadata_url"], Value::Null);
    }

    #[test]
    fn fingerprints_are_those_of_the_pinned_certificates() {
        assert!(fingerprints(&record()).is_empty());
        let record = ProviderRecord {
            connection: Connection::Saml {
                sso_url: SSO_URL.into(),
                certificates: vec![
                    CERTIFICATE.into(),
                    "not a certificate".into(),
                    OTHER_CERTIFICATE.into(),
                ],
                metadata_url: None,
            },
            ..saml_record()
        };
        assert_eq!(
            fingerprints(&record),
            [fingerprint(CERTIFICATE), fingerprint(OTHER_CERTIFICATE)]
        );
    }

    #[test]
    fn values_equal_to_the_stored_ones_are_no_change() {
        let record = record();
        let domains = vec![" ACME.com ".to_owned()];
        let revision = Revision {
            display_name: Some(" Okta "),
            issuer: Some("https://acme.okta.com"),
            client_secret: Some(SECRET),
            allowed_domains: Some(&domains),
            enabled: Some(true),
            ..Revision::default()
        };
        let changes = changed(&revision, &record);
        assert!(changes.is_empty(), "{changes:?}");
        assert!(!rediscovers(&record, &provider(&revision, &record)));
    }

    #[test]
    fn only_revised_values_are_changed_and_normalized() {
        let record = record();
        let domains = vec!["Acme.IO".to_owned(), "acme.io".to_owned()];
        let revision = Revision {
            client_id: Some(" 0oa2 "),
            client_secret: Some("rotated"),
            allowed_domains: Some(&domains),
            enabled: Some(false),
            ..Revision::default()
        };
        let revised = provider(&revision, &record);
        let changes = changed(&revision, &record);
        assert_eq!(changes.display_name, None);
        assert_eq!(changes.issuer, None);
        assert_eq!(changes.client_id.as_deref(), Some("0oa2"));
        assert_eq!(
            changes.client_secret.as_ref().map(SecretValue::expose),
            Some("rotated")
        );
        assert_eq!(changes.allowed_domains, Some(vec!["acme.io".to_owned()]));
        assert_eq!(changes.require_verified_email, None);
        assert_eq!(changes.enabled, Some(false));
        assert!(rediscovers(&record, &revised));
    }

    #[test]
    fn the_difference_keeps_only_changed_fields() {
        let before = record();
        let after = ProviderRecord {
            display_name: "Okta SSO".into(),
            enabled: false,
            ..before.clone()
        };
        let (old, new) = difference(recorded(&before), recorded(&after));
        assert_eq!(
            Value::Object(old),
            json!({ "display_name": "Okta", "enabled": true })
        );
        assert_eq!(
            Value::Object(new),
            json!({ "display_name": "Okta SSO", "enabled": false })
        );
    }

    #[test]
    fn a_saml_revision_without_metadata_keeps_the_pinned_connection() {
        let record = saml_record();
        let domains = vec!["ACME.io".to_owned()];
        let revision = Revision {
            display_name: Some(" Okta SAML "),
            allowed_domains: Some(&domains),
            enabled: Some(false),
            ..Revision::default()
        };
        assert_eq!(
            saml_changed(&revision, &record, None).unwrap(),
            ProviderChanges {
                display_name: Some("Okta SAML".into()),
                allowed_domains: Some(vec!["acme.io".into()]),
                enabled: Some(false),
                ..ProviderChanges::default()
            }
        );
        assert!(
            saml_changed(&Revision::default(), &record, None)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn the_same_metadata_url_again_refreshes_only_the_certificates() {
        let record = saml_record();
        let revision = Revision {
            metadata_url: Some(METADATA_URL),
            ..Revision::default()
        };
        let rotated = imported(
            ENTITY_ID,
            &[CERTIFICATE, OTHER_CERTIFICATE],
            Some(METADATA_URL),
        );
        let changes = saml_changed(&revision, &record, Some(&rotated)).unwrap();
        assert_eq!(
            changes,
            ProviderChanges {
                certificates: Some(vec![CERTIFICATE.into(), OTHER_CERTIFICATE.into()]),
                ..ProviderChanges::default()
            }
        );
        assert!(!changes.renames_subjects());
        let unchanged = imported(ENTITY_ID, &[CERTIFICATE], Some(METADATA_URL));
        assert!(
            saml_changed(&revision, &record, Some(&unchanged))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn pasted_metadata_clears_the_metadata_url() {
        let record = saml_record();
        let pasted = imported(ENTITY_ID, &[CERTIFICATE], None);
        let revision = Revision {
            metadata: Some("<md:EntityDescriptor/>"),
            ..Revision::default()
        };
        assert_eq!(
            saml_changed(&revision, &record, Some(&pasted)).unwrap(),
            ProviderChanges {
                metadata_url: Some(None),
                ..ProviderChanges::default()
            }
        );
    }

    #[test]
    fn a_new_metadata_url_is_kept() {
        let record = saml_record();
        let moved = "https://idp.example.com/moved/metadata";
        let revision = Revision {
            metadata_url: Some(moved),
            ..Revision::default()
        };
        let refreshed = imported(ENTITY_ID, &[CERTIFICATE], Some(moved));
        assert_eq!(
            saml_changed(&revision, &record, Some(&refreshed))
                .unwrap()
                .metadata_url,
            Some(Some(moved.to_owned()))
        );
    }

    #[test]
    fn metadata_with_another_entity_id_renames_every_subject() {
        let record = saml_record();
        let revision = Revision {
            metadata_url: Some(METADATA_URL),
            ..Revision::default()
        };
        let replaced = imported(OTHER_ENTITY_ID, &[CERTIFICATE], Some(METADATA_URL));
        let changes = saml_changed(&revision, &record, Some(&replaced)).unwrap();
        assert_eq!(changes.issuer.as_deref(), Some(OTHER_ENTITY_ID));
        assert!(changes.renames_subjects());
    }

    #[test]
    fn a_saml_revision_keeps_the_saml_rules() {
        let record = saml_record();
        let refused = |revision: Revision<'_>| match saml_changed(&revision, &record, None) {
            Err(Failure::Refused(error)) => error,
            other => panic!("{revision:?} gave {other:?}"),
        };
        assert_eq!(
            refused(Revision {
                require_verified_email: Some(true),
                ..Revision::default()
            }),
            Error::VerifiedEmailUnsupported
        );
        assert_eq!(
            refused(Revision {
                allowed_domains: Some(&[][..]),
                ..Revision::default()
            }),
            Error::DomainRequired
        );
    }

    #[tokio::test]
    async fn nothing_to_import_is_no_metadata() {
        assert_eq!(
            Imported::read(&saml::Client::new(), None, None).await,
            Ok(None)
        );
    }

    #[tokio::test]
    async fn both_metadata_sources_are_refused_before_anything_is_fetched() {
        let refused = Imported::read(
            &saml::Client::new(),
            Some("https://idp.example.invalid/metadata"),
            Some("<md:EntityDescriptor/>"),
        )
        .await;
        assert_eq!(refused, Err(Error::MetadataInvalid(AMBIGUOUS.to_owned())));
    }

    #[tokio::test]
    async fn pasted_metadata_is_imported_without_a_metadata_url() {
        let document = metadata(ENTITY_ID, SSO_URL, &[CERTIFICATE]);
        let imported = Imported::read(&saml::Client::new(), None, Some(&document))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(imported.metadata.entity_id.as_str(), ENTITY_ID);
        assert_eq!(imported.metadata.sso_url.as_str(), SSO_URL);
        assert_eq!(imported.metadata_url, None);
    }

    #[tokio::test]
    async fn a_metadata_url_is_checked_before_it_is_fetched() {
        let refused = Imported::read(
            &saml::Client::new(),
            Some("http://idp.example.com/metadata"),
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(refused, Error::MetadataInvalid(_)), "{refused:?}");
    }

    #[test]
    fn a_missing_metadata_source_names_both_choices() {
        assert_eq!(
            Imported::missing(),
            Error::MetadataInvalid(MISSING.to_owned())
        );
    }
}
