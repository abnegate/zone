use abnegate_secret::SecretValue;
use serde_json::{Map, Value, json};
use uuid::Uuid;
use zone_enterprise::sso::{Error, Fields, Provider};

use super::failure::Failure;
use super::revision::Revision;
use crate::db::audit::{actions, resources};
use crate::db::sso::{NewProvider, ProviderChanges, ProviderRecord, providers, settings};
use crate::db::users::UserRow;
use crate::routes::common::{AuditEvent, audit};
use crate::state::AppState;

const CLIENT_SECRET_CHANGED: &str = "client_secret_changed";

pub async fn create(
    state: &AppState,
    actor: &UserRow,
    fields: &Fields<'_>,
    enabled: bool,
) -> Result<ProviderRecord, Failure> {
    let provider = Provider::parse(fields)?;
    state.oidc().discover(&provider.issuer).await?;
    let record = providers::create(
        state.db(),
        state.master_key(),
        &NewProvider::new(&provider, enabled),
    )
    .await?;
    let mut created = recorded(&record);
    created.insert(CLIENT_SECRET_CHANGED.into(), Value::Bool(true));
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

pub async fn update(
    state: &AppState,
    actor: &UserRow,
    id: Uuid,
    revision: &Revision<'_>,
) -> Result<ProviderRecord, Failure> {
    let current = providers::get(state.db(), state.master_key(), id)
        .await?
        .ok_or(Error::ProviderNotFound)?;
    let candidate = Provider::parse(&revision.fields(&current))?;
    if rediscovers(&current, &candidate) {
        state.oidc().discover(&candidate.issuer).await?;
    }

    let mut transaction = state.db().begin().await?;
    if revision.enabled == Some(false) {
        settings::lock(&mut transaction).await?;
    }
    let locked = providers::lock(&mut transaction, state.master_key(), id)
        .await?
        .ok_or(Error::ProviderNotFound)?;
    let provider = Provider::parse(&revision.fields(&locked))?;
    let changes = changes(&locked, &provider, revision.enabled(&locked));
    if changes.is_empty() {
        transaction.rollback().await?;
        return Ok(locked);
    }
    if changes.enabled == Some(false) && last_enabled(&mut transaction).await? {
        return Err(Error::ProviderRequired.into());
    }
    let updated = providers::update(&mut *transaction, state.master_key(), id, &changes)
        .await?
        .ok_or(Error::ProviderNotFound)?;
    transaction.commit().await?;

    let (before, mut after) = difference(recorded(&locked), recorded(&updated));
    if changes.client_secret.is_some() {
        after.insert(CLIENT_SECRET_CHANGED.into(), Value::Bool(true));
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
    let locked = providers::lock(&mut transaction, state.master_key(), id)
        .await?
        .ok_or(Error::ProviderNotFound)?;
    if locked.enabled && last_enabled(&mut transaction).await? {
        return Err(Error::ProviderRequired.into());
    }
    providers::delete(&mut *transaction, id).await?;
    transaction.commit().await?;
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

/// Whether removing one enabled provider would leave enforcement on with none. The caller holds
/// the settings lock, so no other change can pass the same count.
async fn last_enabled(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<bool, Failure> {
    Ok(settings::required(&mut **transaction).await?
        && providers::count_enabled(&mut **transaction).await? <= 1)
}

fn rediscovers(record: &ProviderRecord, provider: &Provider) -> bool {
    record.issuer != provider.issuer.as_str() || record.client_id != provider.client_id.as_str()
}

fn changes(record: &ProviderRecord, provider: &Provider, enabled: bool) -> ProviderChanges {
    let differs = |stored: &str, revised: &str| (stored != revised).then(|| revised.to_owned());
    ProviderChanges {
        display_name: differs(&record.display_name, provider.display_name.as_str()),
        issuer: differs(&record.issuer, provider.issuer.as_str()),
        client_id: differs(&record.client_id, provider.client_id.as_str()),
        client_secret: (record.client_secret.expose() != provider.client_secret.expose())
            .then(|| SecretValue::new(provider.client_secret.expose())),
        allowed_domains: (record.allowed_domains != provider.policy.domains.as_slice())
            .then(|| provider.policy.domains.as_slice().to_vec()),
        require_verified_email: (record.require_verified_email
            != provider.policy.require_verified_email)
            .then_some(provider.policy.require_verified_email),
        enabled: (record.enabled != enabled).then_some(enabled),
    }
}

/// What the audit log keeps of a provider. Never the client secret: whoever can read the audit
/// log could otherwise sign in as Zone at the identity provider.
pub fn recorded(record: &ProviderRecord) -> Map<String, Value> {
    let Value::Object(map) = json!({
        "protocol": record.protocol,
        "display_name": record.display_name,
        "issuer": record.issuer,
        "client_id": record.client_id,
        "allowed_domains": record.allowed_domains,
        "require_verified_email": record.require_verified_email,
        "enabled": record.enabled,
    }) else {
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
    use chrono::Utc;
    use zone_enterprise::sso::Protocol;

    use super::*;

    const SECRET: &str = "s3cret-client-value";

    fn record() -> ProviderRecord {
        ProviderRecord {
            id: Uuid::new_v4(),
            protocol: Protocol::Oidc,
            display_name: "Okta".into(),
            issuer: "https://acme.okta.com".into(),
            client_id: "0oa1".into(),
            client_secret: SecretValue::new(SECRET),
            allowed_domains: vec!["acme.com".into()],
            require_verified_email: true,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn provider(revision: &Revision<'_>, record: &ProviderRecord) -> Provider {
        Provider::parse(&revision.fields(record)).unwrap()
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
        let changes = changes(
            &record,
            &provider(&revision, &record),
            revision.enabled(&record),
        );
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
        let changes = changes(&record, &revised, revision.enabled(&record));
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
}
