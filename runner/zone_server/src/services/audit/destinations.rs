//! Audit destinations: where an organization's, or the server's, audit events are streamed.
//! Settings are checked against the endpoint rule both when saved and when opened for sending,
//! certificates are parsed before they are saved, secrets are sealed at rest and only their
//! names leave this module, and every change is audited. Testing a destination is not a change,
//! so it is never audited.

mod draft;
mod error;
mod overview;
mod revision;
mod view;

pub use draft::Draft;
pub use error::Error;
pub use overview::Overview;
pub use revision::Revision;
pub use view::View;

use std::collections::HashMap;
use std::sync::Arc;

use abnegate_http::metadata;
use abnegate_secret::{MasterKey, SecretValue};
use chrono::{DateTime, Utc};
use futures::future::try_join_all;
use reqwest::Url;
use serde_json::{Map, Value, json};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;
use zone_enterprise::audit::delivery::STALL_AGE;
use zone_enterprise::audit::destination::{self, SecretName, Secrets, Settings};
use zone_enterprise::audit::{Event, FailureCode, Kind, UrlReason, VERSION};

use super::Actor;
use super::delivery::Open;
use super::transport::open::Transports;
use super::transport::{Batch, Failure, syslog};
use crate::db::audit::deliveries::{self, Delivery};
use crate::db::audit::destinations::{
    self as store, DestinationChanges, DestinationRecord, NewDestination,
};
use crate::db::audit::scope::Scope;
use crate::db::audit::{actions, events, resources};
use crate::routes::common::{AuditEvent, audit};
use crate::services::endpoint::{UrlError, validate_url};
use crate::services::hosts::Hosts;
use crate::state::AppState;

pub const HOST_REFUSED: &str =
    "The destination's host isn't allowed by ZONE_ENDPOINT_HOSTS or is a metadata address";
pub const SETTINGS_UNREADABLE: &str = "The destination's stored settings can't be read";
pub const SECRETS_UNOPENABLE: &str =
    "The destination's secrets can't be opened with this server's encryption key";

const KIND: &str = "kind";
const NAME: &str = "name";
const ENABLED: &str = "enabled";
const SETTINGS: &str = "settings";
const SECRETS_CHANGED: &str = "secrets_changed";
const PORT: &str = "port";
const SYSLOG_SCHEME: &str = "http";

/// Every destination of `scope` with its delivery health. Reads on the pool: a transaction that
/// has written would hold the snapshot horizon back and misreport the backlog.
pub async fn overview(state: &AppState, scope: Scope) -> Result<Overview, Error> {
    let pool = state.db();
    let records = store::list(pool, scope).await?;
    let mut deliveries: HashMap<Uuid, Delivery> = deliveries::list(pool, scope)
        .await?
        .into_iter()
        .map(|delivery| (delivery.destination_id, delivery))
        .collect();
    let paired = records.into_iter().filter_map(|record| {
        let delivery = deliveries.remove(&record.id)?;
        Some(present(pool, state.master_key(), scope, record, delivery))
    });
    let destinations = try_join_all(paired).await?;
    let stalled_since = stalled(pool, scope, Utc::now()).await?;
    Ok(Overview {
        stalled_since,
        destinations,
    })
}

pub async fn view(state: &AppState, scope: Scope, id: Uuid) -> Result<View, Error> {
    let pool = state.db();
    let record = store::get(pool, scope, id)
        .await?
        .ok_or(zone_enterprise::audit::Error::DestinationNotFound)?;
    let delivery = delivery(pool, id).await?;
    present(pool, state.master_key(), scope, record, delivery).await
}

/// `transaction` is the caller's, with access to `scope` already checked inside it.
pub async fn create(
    state: &AppState,
    mut transaction: Transaction<'_, Postgres>,
    scope: Scope,
    actor: &Actor<'_>,
    draft: Draft,
) -> Result<View, Error> {
    let name = destination::name(&draft.name)?;
    let settings = requested(draft.settings)?;
    let secrets = Secrets::parse(settings.kind(), draft.secrets)?;
    settings.admits(&secrets)?;
    certified(&secrets)?;
    check(&settings, &state.config().endpoint_hosts)?;

    let record = store::create(
        &mut transaction,
        state.master_key(),
        &NewDestination {
            scope,
            kind: settings.kind().as_str().to_owned(),
            name,
            enabled: draft.enabled,
            settings: settings.stored(),
            secrets: SecretValue::new(secrets.sealed_form()),
            created_by: actor.id,
        },
    )
    .await?;
    transaction.commit().await?;

    let mut created = recorded(&record);
    created.insert(SECRETS_CHANGED.into(), json!(secrets.names()));
    log(
        state,
        actor,
        scope,
        actions::AUDIT_DESTINATION_CREATED,
        record.id,
        None,
        Some(created),
    )
    .await;
    view(state, scope, record.id).await
}

/// `transaction` is the caller's, with access to `scope` already checked inside it. Re-enabling
/// the destination, or changing its settings or secrets, clears its pinned batch and retries now.
pub async fn update(
    state: &AppState,
    mut transaction: Transaction<'_, Postgres>,
    scope: Scope,
    id: Uuid,
    actor: &Actor<'_>,
    revision: Revision,
) -> Result<View, Error> {
    let key = state.master_key();
    let current = store::lock(&mut transaction, scope, id)
        .await?
        .ok_or(zone_enterprise::audit::Error::DestinationNotFound)?;
    let kind = kind(&current)?;
    let stored =
        Settings::from_stored(kind, current.settings.clone()).map_err(|_| Error::Unreadable(id))?;
    let opened = sealed(&current, kind, key).ok();

    let name = revision
        .name
        .as_deref()
        .map(destination::name)
        .transpose()?;
    let settings = revision
        .settings
        .map(|value| replacement(value, kind, &state.config().endpoint_hosts))
        .transpose()?;
    let secrets = match (revision.secrets, &opened) {
        (None, _) => None,
        (Some(patch), Some(opened)) => Some(opened.merged(patch)?),
        (Some(patch), None) => Some(Secrets::parse(kind, patch)?),
    };
    if let Some(effective) = secrets.as_ref().or(opened.as_ref()) {
        settings.as_ref().unwrap_or(&stored).admits(effective)?;
    }
    if let Some(secrets) = &secrets {
        certified(secrets)?;
    }

    let rotated = changed(opened.as_ref(), secrets.as_ref());
    let changes = DestinationChanges {
        name: name.filter(|name| *name != current.name),
        enabled: revision
            .enabled
            .filter(|enabled| *enabled != current.enabled),
        settings: settings
            .as_ref()
            .map(Settings::stored)
            .filter(|settings| *settings != current.settings),
        secrets: secrets
            .filter(|_| !rotated.is_empty())
            .map(|secrets| SecretValue::new(secrets.sealed_form())),
    };
    if changes.is_empty() {
        transaction.rollback().await?;
        return view(state, scope, id).await;
    }

    let updated = store::update(&mut *transaction, key, scope, id, &changes)
        .await?
        .ok_or(zone_enterprise::audit::Error::DestinationNotFound)?;
    if changes.enabled == Some(true) || changes.settings.is_some() || changes.secrets.is_some() {
        deliveries::reset(&mut *transaction, id).await?;
    }
    transaction.commit().await?;

    let (before, mut after) = difference(recorded(&current), recorded(&updated));
    if !rotated.is_empty() {
        after.insert(SECRETS_CHANGED.into(), json!(rotated));
    }
    log(
        state,
        actor,
        scope,
        actions::AUDIT_DESTINATION_UPDATED,
        id,
        Some(before),
        Some(after),
    )
    .await;
    view(state, scope, id).await
}

/// `transaction` is the caller's, with access to `scope` already checked inside it.
pub async fn delete(
    state: &AppState,
    mut transaction: Transaction<'_, Postgres>,
    scope: Scope,
    id: Uuid,
    actor: &Actor<'_>,
) -> Result<(), Error> {
    let current = store::lock(&mut transaction, scope, id)
        .await?
        .ok_or(zone_enterprise::audit::Error::DestinationNotFound)?;
    store::delete(&mut *transaction, scope, id).await?;
    transaction.commit().await?;
    log(
        state,
        actor,
        scope,
        actions::AUDIT_DESTINATION_DELETED,
        id,
        Some(recorded(&current)),
        None,
    )
    .await;
    Ok(())
}

/// Sends one synthetic event, which is never written to the audit log, to the destination now,
/// whether or not it is enabled and without touching its delivery cursor. The outer error is
/// the destination's own; the inner one is how the send failed.
pub async fn test(
    state: &AppState,
    scope: Scope,
    id: Uuid,
    actor: &Actor<'_>,
) -> Result<Result<(), Failure>, Error> {
    let record = store::get(state.db(), scope, id)
        .await?
        .ok_or(zone_enterprise::audit::Error::DestinationNotFound)?;
    let batch = Batch {
        destination_id: id,
        scope,
        events: vec![Event {
            id: Uuid::new_v4(),
            version: VERSION,
            created_at: Utc::now(),
            organization_id: scope.organization_id(),
            workspace_id: None,
            actor_id: Some(actor.id),
            actor_email: Some(actor.email.to_owned()),
            action: actions::AUDIT_DESTINATION_TESTED.to_owned(),
            resource_type: resources::AUDIT_DESTINATION.to_owned(),
            resource_id: Some(id),
            old_values: None,
            new_values: None,
            ip_address: None,
            user_agent: None,
        }],
    };
    let transports = Transports::new(
        Arc::clone(state.master_key()),
        state.config().endpoint_hosts.clone(),
    );
    Ok(sent(&transports, &record, &batch).await)
}

async fn sent(
    transports: &Transports,
    record: &DestinationRecord,
    batch: &Batch,
) -> Result<(), Failure> {
    transports.open(record).await?.deliver(batch).await
}

/// The settings and secrets to send with, re-checked against the endpoint rule: an operator may
/// have narrowed `ZONE_ENDPOINT_HOSTS` since the destination was saved.
pub fn open(
    record: &DestinationRecord,
    key: &MasterKey,
    hosts: &Hosts,
) -> Result<(Settings, Secrets), Failure> {
    let unreadable = || Failure::new(FailureCode::Configuration, SETTINGS_UNREADABLE);
    let kind = Kind::parse(&record.kind).ok_or_else(unreadable)?;
    let settings =
        Settings::from_stored(kind, record.settings.clone()).map_err(|_| unreadable())?;
    let secrets = sealed(record, kind, key)
        .map_err(|_| Failure::new(FailureCode::Unsealable, SECRETS_UNOPENABLE))?;
    check(&settings, hosts).map_err(|_| Failure::new(FailureCode::Configuration, HOST_REFUSED))?;
    Ok((settings, secrets))
}

/// The endpoint rule on where validated `settings` send: http or https to any host, LAN and
/// loopback included, never a metadata address, and only listed hosts when the operator set
/// `ZONE_ENDPOINT_HOSTS`. S3 without an endpoint sends to AWS, which must be listed too.
pub fn check(settings: &Settings, hosts: &Hosts) -> Result<(), zone_enterprise::audit::Error> {
    match settings {
        Settings::Webhook(webhook) => endpoint(&webhook.url, hosts),
        Settings::S3(s3) => endpoint(s3.endpoint_url().as_str(), hosts),
        Settings::Syslog(syslog) => {
            let refused = || zone_enterprise::audit::Error::HostInvalid(syslog.host.clone());
            if metadata::is_host(&syslog.host) {
                return Err(refused());
            }
            let url = Url::parse(&format!("{SYSLOG_SCHEME}://{}", syslog.authority()))
                .map_err(|_| refused())?;
            if !hosts.permits(&url) {
                return Err(refused());
            }
            Ok(())
        }
    }
}

/// TLS certificates and keys parse into the client the transport would build, so a bad one is
/// refused when saved rather than at every delivery.
fn certified(secrets: &Secrets) -> Result<(), zone_enterprise::audit::Error> {
    match secrets {
        Secrets::Syslog(secrets) => syslog::check(secrets),
        Secrets::Webhook(_) | Secrets::S3(_) => Ok(()),
    }
}

fn endpoint(raw: &str, hosts: &Hosts) -> Result<(), zone_enterprise::audit::Error> {
    validate_url(raw, hosts)
        .map(drop)
        .map_err(|error| zone_enterprise::audit::Error::UrlInvalid(reason(error)))
}

fn reason(error: UrlError) -> UrlReason {
    match error {
        UrlError::Unparseable => UrlReason::Unparseable,
        UrlError::Scheme => UrlReason::Scheme,
        UrlError::Host => UrlReason::Host,
        UrlError::Userinfo => UrlReason::Userinfo,
        UrlError::Query => UrlReason::Query,
        UrlError::Fragment => UrlReason::Fragment,
        UrlError::Metadata => UrlReason::Metadata,
        UrlError::Unlisted => UrlReason::Unlisted,
    }
}

/// Settings as a request sends them, tagged with their kind. A missing text field reads as empty
/// and a port that isn't a port is refused, so each gets its own code rather than the kind's.
fn requested(value: Value) -> Result<Settings, zone_enterprise::audit::Error> {
    let Value::Object(mut object) = value else {
        return Err(zone_enterprise::audit::Error::KindInvalid);
    };
    let kind = object
        .get(KIND)
        .and_then(Value::as_str)
        .and_then(Kind::parse)
        .ok_or(zone_enterprise::audit::Error::KindInvalid)?;
    for field in required(kind) {
        object
            .entry(*field)
            .or_insert_with(|| Value::String(String::new()));
    }
    if kind == Kind::Syslog
        && !object
            .get(PORT)
            .and_then(Value::as_u64)
            .is_some_and(|port| u16::try_from(port).is_ok())
    {
        return Err(zone_enterprise::audit::Error::PortInvalid);
    }
    let settings: Settings = serde_json::from_value(Value::Object(object))
        .map_err(|_| zone_enterprise::audit::Error::KindInvalid)?;
    settings.validated()
}

fn required(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Webhook => &["url"],
        Kind::S3 => &["bucket", "region"],
        Kind::Syslog => &["host"],
    }
}

fn replacement(
    value: Value,
    kind: Kind,
    hosts: &Hosts,
) -> Result<Settings, zone_enterprise::audit::Error> {
    let settings = requested(value)?;
    if settings.kind() != kind {
        return Err(zone_enterprise::audit::Error::KindInvalid);
    }
    check(&settings, hosts)?;
    Ok(settings)
}

fn kind(record: &DestinationRecord) -> Result<Kind, Error> {
    Kind::parse(&record.kind).ok_or(Error::Unreadable(record.id))
}

fn sealed(record: &DestinationRecord, kind: Kind, key: &MasterKey) -> Result<Secrets, Error> {
    let opened = record.secrets(key)?;
    Secrets::from_sealed_form(kind, opened.expose()).map_err(|_| Error::Unreadable(record.id))
}

/// The names whose values a change adds, replaces or clears. When the stored secrets can't be
/// opened, every name the new secrets hold counts as changed.
fn changed(before: Option<&Secrets>, after: Option<&Secrets>) -> Vec<SecretName> {
    let Some(after) = after else {
        return Vec::new();
    };
    let Some(before) = before else {
        return after.names();
    };
    let entries = |secrets: &Secrets| -> Map<String, Value> {
        serde_json::from_str(&secrets.sealed_form()).unwrap_or_default()
    };
    let (old, new) = (entries(before), entries(after));
    SecretName::of(after.kind())
        .filter(|name| old.get(name.as_str()) != new.get(name.as_str()))
        .collect()
}

async fn delivery(pool: &PgPool, id: Uuid) -> Result<Delivery, Error> {
    deliveries::get(pool, id)
        .await?
        .ok_or(Error::Unreadable(id))
}

async fn present(
    pool: &PgPool,
    key: &MasterKey,
    scope: Scope,
    record: DestinationRecord,
    delivery: Delivery,
) -> Result<View, Error> {
    let kind = kind(&record)?;
    let settings = Settings::from_stored(kind, record.settings.clone())
        .map_err(|_| Error::Unreadable(record.id))?;
    let secrets = match sealed(&record, kind, key) {
        Ok(secrets) => secrets.names(),
        Err(error) => {
            tracing::warn!(%error, destination = %record.id, "Could not open an audit destination's secrets to list their names");
            Vec::new()
        }
    };
    let backlog = events::backlog(pool, scope, delivery.delivered).await?;
    Ok(View {
        id: record.id,
        name: record.name,
        kind,
        enabled: record.enabled,
        settings,
        secrets,
        delivery,
        backlog,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

async fn stalled(
    pool: &PgPool,
    scope: Scope,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    Ok(events::stalled(pool, scope)
        .await?
        .filter(|since| now - *since > STALL_AGE))
}

/// The response shape without secrets, with the settings tagged by kind.
fn recorded(record: &DestinationRecord) -> Map<String, Value> {
    let mut settings = record.settings.clone();
    if let Value::Object(object) = &mut settings {
        object.insert(KIND.into(), Value::String(record.kind.clone()));
    }
    Map::from_iter([
        (NAME.into(), Value::String(record.name.clone())),
        (KIND.into(), Value::String(record.kind.clone())),
        (ENABLED.into(), Value::Bool(record.enabled)),
        (SETTINGS.into(), settings),
    ])
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

async fn log(
    state: &AppState,
    actor: &Actor<'_>,
    scope: Scope,
    action: &str,
    id: Uuid,
    old_values: Option<Map<String, Value>>,
    new_values: Option<Map<String, Value>>,
) {
    audit(
        state.db(),
        AuditEvent {
            organization_id: scope.organization_id(),
            workspace_id: None,
            actor_id: actor.id,
            actor_email: actor.email,
            action,
            resource_type: resources::AUDIT_DESTINATION,
            resource_id: Some(id),
            old_values: old_values.map(Value::Object),
            new_values: new_values.map(Value::Object),
        },
    )
    .await;
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use abnegate_secret::encrypt_value;
    use zone_enterprise::audit::destination::{Protocol, SyslogSettings, WebhookSettings};

    use super::*;

    static KEY: LazyLock<MasterKey> = LazyLock::new(|| MasterKey::generate().expect("a key"));
    const CERTIFICATE: &str = include_str!("../../../tests/fixtures/audit/ca.pem");

    fn record(kind: Kind, settings: Value, secrets: &str) -> DestinationRecord {
        DestinationRecord {
            id: Uuid::new_v4(),
            organization_id: None,
            kind: kind.as_str().to_owned(),
            name: "SIEM".into(),
            enabled: true,
            settings,
            sealed_secrets: encrypt_value(&SecretValue::new(secrets), &KEY).expect("sealed"),
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn webhook(url: &str) -> DestinationRecord {
        record(
            Kind::Webhook,
            json!({"url": url}),
            r#"{"signing_secret":"whsec-plaintext"}"#,
        )
    }

    fn syslog(host: &str) -> Settings {
        Settings::Syslog(
            serde_json::from_value::<SyslogSettings>(
                json!({"host": host, "port": 6514, "protocol": "tls"}),
            )
            .unwrap(),
        )
    }

    fn url_invalid(reason: UrlReason) -> Result<(), zone_enterprise::audit::Error> {
        Err(zone_enterprise::audit::Error::UrlInvalid(reason))
    }

    #[test]
    fn every_endpoint_rule_reason_maps_one_to_one() {
        let pairs = [
            (UrlError::Unparseable, UrlReason::Unparseable),
            (UrlError::Scheme, UrlReason::Scheme),
            (UrlError::Host, UrlReason::Host),
            (UrlError::Userinfo, UrlReason::Userinfo),
            (UrlError::Query, UrlReason::Query),
            (UrlError::Fragment, UrlReason::Fragment),
            (UrlError::Metadata, UrlReason::Metadata),
            (UrlError::Unlisted, UrlReason::Unlisted),
        ];
        assert_eq!(pairs.len(), UrlReason::ALL.len());
        for (error, expected) in pairs {
            assert_eq!(reason(error), expected);
            assert_eq!(error.to_string(), expected.to_string());
        }
    }

    #[test]
    fn lan_and_loopback_endpoints_pass_and_metadata_never_does() {
        let open = Hosts::default();
        for url in [
            "http://10.0.0.5/hook",
            "http://splunk:8088/services/collector",
            "http://127.0.0.1:9000/hook",
            "https://siem.example.com/hook",
        ] {
            let settings = Settings::Webhook(WebhookSettings { url: url.into() });
            assert_eq!(check(&settings, &open), Ok(()), "{url}");
        }
        let metadata = Settings::Webhook(WebhookSettings {
            url: "http://169.254.169.254/latest".into(),
        });
        assert_eq!(check(&metadata, &open), url_invalid(UrlReason::Metadata));
        assert_eq!(check(&syslog("10.0.0.9"), &open), Ok(()));
        assert_eq!(
            check(&syslog("169.254.169.254"), &open),
            Err(zone_enterprise::audit::Error::HostInvalid(
                "169.254.169.254".into()
            ))
        );
    }

    #[test]
    fn listed_hosts_narrow_every_kind() {
        let hosts = Hosts::parse("siem.example.com");
        let allowed = Settings::Webhook(WebhookSettings {
            url: "https://siem.example.com/hook".into(),
        });
        let unlisted = Settings::Webhook(WebhookSettings {
            url: "http://10.0.0.5/hook".into(),
        });
        assert_eq!(check(&allowed, &hosts), Ok(()));
        assert_eq!(check(&unlisted, &hosts), url_invalid(UrlReason::Unlisted));
        assert_eq!(check(&syslog("siem.example.com"), &hosts), Ok(()));
        assert!(check(&syslog("10.0.0.9"), &hosts).is_err());
        let aws =
            requested(json!({"kind": "s3", "bucket": "audit", "region": "us-east-1"})).unwrap();
        assert_eq!(check(&aws, &hosts), url_invalid(UrlReason::Unlisted));
    }

    #[test]
    fn a_missing_or_malformed_field_gets_the_fields_own_code() {
        let cases = [
            (
                json!({"kind": "webhook"}),
                zone_enterprise::audit::URL_INVALID,
            ),
            (
                json!({"kind": "s3", "region": "us-east-1"}),
                zone_enterprise::audit::BUCKET_INVALID,
            ),
            (
                json!({"kind": "s3", "bucket": "audit"}),
                zone_enterprise::audit::REGION_INVALID,
            ),
            (
                json!({"kind": "syslog", "port": 514, "protocol": "udp"}),
                zone_enterprise::audit::HOST_INVALID,
            ),
            (
                json!({"kind": "syslog", "host": "siem", "protocol": "udp"}),
                zone_enterprise::audit::PORT_INVALID,
            ),
            (
                json!({"kind": "syslog", "host": "siem", "port": 70000, "protocol": "udp"}),
                zone_enterprise::audit::PORT_INVALID,
            ),
            (
                json!({"kind": "syslog", "host": "siem", "port": 0, "protocol": "udp"}),
                zone_enterprise::audit::PORT_INVALID,
            ),
            (
                json!({"kind": "syslog", "host": "siem", "port": 514, "protocol": "udp", "facility": 24}),
                zone_enterprise::audit::FACILITY_INVALID,
            ),
            (
                json!({"kind": "smtp"}),
                zone_enterprise::audit::KIND_INVALID,
            ),
            (json!("webhook"), zone_enterprise::audit::KIND_INVALID),
            (
                json!({"kind": "webhook", "url": "https://siem.example.com", "extra": 1}),
                zone_enterprise::audit::KIND_INVALID,
            ),
        ];
        for (value, code) in cases {
            let shown = value.to_string();
            assert_eq!(requested(value).unwrap_err().code(), code, "{shown}");
        }
    }

    #[test]
    fn requested_settings_are_validated_and_normalised() {
        let settings = requested(json!({
            "kind": "syslog", "host": " SIEM.example.com ", "port": 514, "protocol": "udp"
        }))
        .unwrap();
        let Settings::Syslog(syslog) = settings else {
            panic!("a syslog destination");
        };
        assert_eq!(syslog.host, "siem.example.com");
        assert_eq!(syslog.protocol, Protocol::Udp);
        assert_eq!(syslog.facility.value(), 13);
        assert_eq!(syslog.hostname, "zone");
    }

    #[test]
    fn a_replacement_keeps_the_kind() {
        assert_eq!(
            replacement(
                json!({"kind": "s3", "bucket": "audit", "region": "us-east-1"}),
                Kind::Webhook,
                &Hosts::default()
            ),
            Err(zone_enterprise::audit::Error::KindInvalid)
        );
    }

    #[test]
    fn only_the_secrets_whose_values_differ_count_as_changed() {
        let before = Secrets::parse(
            Kind::Syslog,
            json!({"ca_certificate": "-----BEGIN CERTIFICATE-----\nA\n-----END CERTIFICATE-----"}),
        )
        .unwrap();
        assert!(changed(Some(&before), Some(&before)).is_empty());
        assert!(changed(Some(&before), None).is_empty());
        let cleared = before.merged(json!({"ca_certificate": ""})).unwrap();
        assert_eq!(
            changed(Some(&before), Some(&cleared)),
            vec![SecretName::CaCertificate]
        );
        let rotated = Secrets::parse(Kind::Webhook, json!({"signing_secret": "new"})).unwrap();
        assert_eq!(
            changed(None, Some(&rotated)),
            vec![SecretName::SigningSecret]
        );
    }

    #[test]
    fn open_reads_what_was_stored() {
        let (settings, secrets) =
            open(&webhook("http://10.0.0.5/hook"), &KEY, &Hosts::default()).unwrap();
        assert_eq!(
            settings,
            Settings::Webhook(WebhookSettings {
                url: "http://10.0.0.5/hook".into()
            })
        );
        assert_eq!(secrets.names(), vec![SecretName::SigningSecret]);
    }

    #[test]
    fn open_refuses_a_host_the_operator_no_longer_allows() {
        let failure = open(
            &webhook("http://10.0.0.5/hook"),
            &KEY,
            &Hosts::parse("allowed.example"),
        )
        .unwrap_err();
        assert_eq!(failure.code, FailureCode::Configuration);
        assert_eq!(failure.message, HOST_REFUSED);
        let metadata = open(
            &webhook("http://169.254.169.254/hook"),
            &KEY,
            &Hosts::default(),
        )
        .unwrap_err();
        assert_eq!(metadata.code, FailureCode::Configuration);
    }

    #[test]
    fn open_reports_secrets_sealed_under_another_key_as_unsealable() {
        let other = MasterKey::generate().expect("a key");
        let failure = open(
            &webhook("https://siem.example.com/hook"),
            &other,
            &Hosts::default(),
        )
        .unwrap_err();
        assert_eq!(failure.code, FailureCode::Unsealable);
        assert!(!failure.message.contains("whsec-plaintext"));
    }

    #[test]
    fn open_reports_unreadable_settings_as_configuration() {
        let mut broken = webhook("https://siem.example.com/hook");
        broken.settings = json!({"bucket": "audit"});
        assert_eq!(
            open(&broken, &KEY, &Hosts::default()).unwrap_err().code,
            FailureCode::Configuration
        );
        broken.kind = "smtp".into();
        assert_eq!(
            open(&broken, &KEY, &Hosts::default()).unwrap_err().code,
            FailureCode::Configuration
        );
    }

    #[test]
    fn a_certificate_that_is_pem_but_not_a_certificate_is_refused() {
        let parsed = |ca: &str| {
            Secrets::parse(Kind::Syslog, json!({"ca_certificate": ca})).expect("PEM form")
        };
        assert_eq!(
            certified(&parsed(
                "-----BEGIN CERTIFICATE-----\nAAECAwQF\n-----END CERTIFICATE-----\n"
            )),
            Err(zone_enterprise::audit::Error::CertificateInvalid(
                SecretName::CaCertificate
            ))
        );
        assert_eq!(certified(&parsed(CERTIFICATE)), Ok(()));
        let webhook = Secrets::parse(Kind::Webhook, json!({"signing_secret": "whsec"})).unwrap();
        assert_eq!(certified(&webhook), Ok(()));
    }

    #[test]
    fn the_audit_record_carries_no_secret() {
        let recorded = Value::Object(recorded(&webhook("https://siem.example.com/hook")));
        assert_eq!(
            recorded,
            json!({
                "name": "SIEM",
                "kind": "webhook",
                "enabled": true,
                "settings": {"kind": "webhook", "url": "https://siem.example.com/hook"},
            })
        );
        assert!(!recorded.to_string().contains("whsec"));
    }
}
