use std::fmt;

use abnegate_secret::REDACTED;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zone_enterprise::Feature;
use zone_enterprise::sso::{Error, Fields, Protocol};

use super::refusal::{self, Stage};
use super::{failed, unreadable};
use crate::auth::{ServerAdmin, entitlement};
use crate::db::sso::{Connection, ProviderRecord};
use crate::services::sso::{Revision, providers};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ProviderResponse {
    pub id: Uuid,
    pub protocol: Protocol,
    pub display_name: String,
    pub issuer: String,
    pub client_id: Option<String>,
    pub sso_url: Option<String>,
    pub metadata_url: Option<String>,
    pub certificate_fingerprints: Vec<String>,
    pub allowed_domains: Vec<String>,
    pub require_verified_email: bool,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&ProviderRecord> for ProviderResponse {
    fn from(record: &ProviderRecord) -> Self {
        let (client_id, sso_url, metadata_url) = match &record.connection {
            Connection::Oidc { client_id, .. } => (Some(client_id.clone()), None, None),
            Connection::Saml {
                sso_url,
                metadata_url,
                ..
            } => (None, Some(sso_url.clone()), metadata_url.clone()),
        };
        Self {
            id: record.id,
            protocol: record.protocol,
            display_name: record.display_name.clone(),
            issuer: record.issuer.clone(),
            client_id,
            sso_url,
            metadata_url,
            certificate_fingerprints: providers::fingerprints(record),
            allowed_domains: record.allowed_domains.clone(),
            require_verified_email: record.require_verified_email,
            enabled: record.enabled,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

/// A missing text field reads as empty, so it is refused with that field's own code rather than
/// a generic body error. `require_verified_email` defaults by protocol: on for OIDC, and off for
/// SAML, which can't vouch for an email.
#[derive(Deserialize)]
pub struct CreateProviderRequest {
    #[serde(default)]
    pub protocol: Protocol,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub issuer: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    pub metadata_url: Option<String>,
    pub metadata: Option<String>,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    pub require_verified_email: Option<bool>,
    #[serde(default = "on")]
    pub enabled: bool,
}

impl CreateProviderRequest {
    fn fields(&self) -> Fields<'_> {
        Fields {
            protocol: self.protocol,
            display_name: &self.display_name,
            issuer: &self.issuer,
            client_id: &self.client_id,
            client_secret: &self.client_secret,
            allowed_domains: &self.allowed_domains,
            require_verified_email: self
                .require_verified_email
                .unwrap_or(self.protocol == Protocol::Oidc),
        }
    }

    /// Whether the request sets a field the chosen protocol doesn't have.
    fn mismatches(&self) -> bool {
        match self.protocol {
            Protocol::Oidc => self.metadata_url.is_some() || self.metadata.is_some(),
            Protocol::Saml => [&self.issuer, &self.client_id, &self.client_secret]
                .into_iter()
                .any(|field| !field.is_empty()),
        }
    }
}

impl fmt::Debug for CreateProviderRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateProviderRequest")
            .field("protocol", &self.protocol)
            .field("display_name", &self.display_name)
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("client_secret", &REDACTED)
            .field("metadata_url", &self.metadata_url)
            .field("metadata", &self.metadata.as_ref().map(String::len))
            .field("allowed_domains", &self.allowed_domains)
            .field("require_verified_email", &self.require_verified_email)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[derive(Deserialize)]
pub struct UpdateProviderRequest {
    pub display_name: Option<String>,
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub metadata_url: Option<String>,
    pub metadata: Option<String>,
    pub allowed_domains: Option<Vec<String>>,
    pub require_verified_email: Option<bool>,
    pub enabled: Option<bool>,
}

impl UpdateProviderRequest {
    fn revision(&self) -> Revision<'_> {
        Revision {
            display_name: self.display_name.as_deref(),
            issuer: self.issuer.as_deref(),
            client_id: self.client_id.as_deref(),
            client_secret: self.client_secret.as_deref(),
            metadata_url: self.metadata_url.as_deref(),
            metadata: self.metadata.as_deref(),
            allowed_domains: self.allowed_domains.as_deref(),
            require_verified_email: self.require_verified_email,
            enabled: self.enabled,
        }
    }
}

impl fmt::Debug for UpdateProviderRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpdateProviderRequest")
            .field("revision", &self.revision())
            .finish()
    }
}

fn on() -> bool {
    true
}

fn missing() -> Response {
    refusal::respond(&Error::ProviderNotFound, Stage::Configuration)
}

pub async fn create(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
    body: Result<Json<CreateProviderRequest>, JsonRejection>,
) -> Response {
    if let Err(unentitled) = entitlement::require(&state, Feature::Sso).await {
        return unentitled.into_response();
    }
    let Json(request) = match body {
        Ok(body) => body,
        Err(rejection) => return unreadable(&rejection),
    };
    if request.mismatches() {
        return refusal::respond(&Error::ProtocolMismatch, Stage::Configuration);
    }
    match providers::create(
        &state,
        &admin,
        &request.fields(),
        request.metadata_url.as_deref(),
        request.metadata.as_deref(),
        request.enabled,
    )
    .await
    {
        Ok(record) => (StatusCode::CREATED, Json(ProviderResponse::from(&record))).into_response(),
        Err(failure) => failed(failure),
    }
}

/// Disabling a provider and refreshing its metadata need no license, so a lapsed server can
/// still take a provider out of service and follow its identity provider's certificate
/// rotation.
pub async fn update(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<UpdateProviderRequest>, JsonRejection>,
) -> Response {
    let exempt = body.as_ref().is_ok_and(|Json(request)| {
        let revision = request.revision();
        revision.only_disables() || revision.only_refreshes_metadata()
    });
    let entitled = entitlement::require(&state, Feature::Sso).await;
    if !exempt && let Err(unentitled) = entitled {
        return unentitled.into_response();
    }
    let Ok(Path(id)) = path else {
        return missing();
    };
    let Json(request) = match body {
        Ok(body) => body,
        Err(rejection) => return unreadable(&rejection),
    };
    match providers::update(&state, &admin, id, &request.revision(), entitled.is_ok()).await {
        Ok(record) => Json(ProviderResponse::from(&record)).into_response(),
        Err(failure) => failed(failure),
    }
}

pub async fn delete(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(id)) = path else {
        return missing();
    };
    match providers::delete(&state, &admin, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(failure) => failed(failure),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zone_enterprise::sso::saml::Certificate;
    use zone_enterprise::sso::saml::testing::{CERTIFICATE, ENTITY_ID, SSO_URL};

    use super::*;

    const METADATA_URL: &str = "https://idp.example.com/app/metadata";

    fn saml_record() -> ProviderRecord {
        ProviderRecord {
            id: Uuid::nil(),
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
            created_at: "2026-11-01T00:00:00Z".parse().unwrap(),
            updated_at: "2026-11-02T00:00:00Z".parse().unwrap(),
        }
    }

    fn created(body: serde_json::Value) -> CreateProviderRequest {
        serde_json::from_value(body).unwrap()
    }

    #[test]
    fn a_provider_is_sent_without_its_secret() {
        let record = ProviderRecord {
            id: Uuid::nil(),
            protocol: Protocol::Oidc,
            display_name: "Okta".into(),
            issuer: "https://acme.okta.com".into(),
            connection: Connection::Oidc {
                client_id: "0oa1".into(),
                sealed_secret: "ENC[v1:s3cret]".into(),
            },
            allowed_domains: vec!["acme.com".into()],
            require_verified_email: true,
            enabled: false,
            created_at: "2026-11-01T00:00:00Z".parse().unwrap(),
            updated_at: "2026-11-02T00:00:00Z".parse().unwrap(),
        };
        let body = serde_json::to_value(ProviderResponse::from(&record)).unwrap();
        assert_eq!(
            body,
            json!({
                "id": Uuid::nil(),
                "protocol": "oidc",
                "display_name": "Okta",
                "issuer": "https://acme.okta.com",
                "client_id": "0oa1",
                "sso_url": null,
                "metadata_url": null,
                "certificate_fingerprints": [],
                "allowed_domains": ["acme.com"],
                "require_verified_email": true,
                "enabled": false,
                "created_at": "2026-11-01T00:00:00Z",
                "updated_at": "2026-11-02T00:00:00Z",
            })
        );
        assert!(!body.to_string().contains("s3cret"));
    }

    #[test]
    fn a_saml_provider_is_sent_with_fingerprints_instead_of_certificates() {
        let body = serde_json::to_value(ProviderResponse::from(&saml_record())).unwrap();
        let fingerprint = Certificate::parse(CERTIFICATE)
            .unwrap()
            .fingerprint()
            .to_owned();
        assert_eq!(
            body,
            json!({
                "id": Uuid::nil(),
                "protocol": "saml",
                "display_name": "Okta",
                "issuer": ENTITY_ID,
                "client_id": null,
                "sso_url": SSO_URL,
                "metadata_url": METADATA_URL,
                "certificate_fingerprints": [fingerprint],
                "allowed_domains": ["acme.com"],
                "require_verified_email": false,
                "enabled": true,
                "created_at": "2026-11-01T00:00:00Z",
                "updated_at": "2026-11-02T00:00:00Z",
            })
        );
        assert!(!body.to_string().contains(CERTIFICATE));
    }

    #[test]
    fn a_create_request_defaults_to_an_enabled_oidc_provider_that_requires_verified_email() {
        let request = created(json!({
            "display_name": "Okta", "issuer": "https://acme.okta.com",
            "client_id": "0oa1", "client_secret": "s3cret"
        }));
        assert!(request.enabled);
        assert!(request.allowed_domains.is_empty());
        assert!(!request.mismatches());
        let fields = request.fields();
        assert_eq!(fields.protocol, Protocol::Oidc);
        assert!(fields.require_verified_email);
        assert_eq!(fields.client_secret, "s3cret");
    }

    #[test]
    fn a_saml_create_request_defaults_to_not_requiring_a_verified_email() {
        let request = created(json!({
            "protocol": "saml", "display_name": "Okta",
            "metadata_url": METADATA_URL, "allowed_domains": ["acme.com"]
        }));
        assert!(request.enabled);
        assert!(!request.mismatches());
        let fields = request.fields();
        assert_eq!(fields.protocol, Protocol::Saml);
        assert!(!fields.require_verified_email);
        assert_eq!(request.metadata_url.as_deref(), Some(METADATA_URL));
        assert_eq!(request.metadata, None);
    }

    #[test]
    fn an_explicit_verified_email_choice_wins_over_the_protocol_default() {
        for protocol in Protocol::ALL {
            for choice in [true, false] {
                let request = created(json!({
                    "protocol": protocol, "display_name": "Okta",
                    "require_verified_email": choice
                }));
                assert_eq!(
                    request.fields().require_verified_email,
                    choice,
                    "{protocol}"
                );
            }
        }
    }

    #[test]
    fn a_field_of_the_other_protocol_mismatches() {
        for body in [
            json!({ "metadata_url": METADATA_URL }),
            json!({ "metadata": "<md:EntityDescriptor/>" }),
            json!({ "protocol": "saml", "metadata_url": METADATA_URL, "issuer": ENTITY_ID }),
            json!({ "protocol": "saml", "metadata_url": METADATA_URL, "client_id": "0oa1" }),
            json!({ "protocol": "saml", "metadata_url": METADATA_URL, "client_secret": "s3cret" }),
        ] {
            assert!(created(body.clone()).mismatches(), "{body}");
        }
        let empty = created(json!({
            "protocol": "saml", "metadata_url": METADATA_URL,
            "issuer": "", "client_id": "", "client_secret": ""
        }));
        assert!(!empty.mismatches());
    }

    #[test]
    fn a_missing_secret_reads_as_empty_so_it_gets_its_own_code() {
        let request = created(json!({
            "display_name": "Okta", "issuer": "https://acme.okta.com", "client_id": "0oa1"
        }));
        assert_eq!(
            zone_enterprise::sso::Provider::parse(&request.fields()).unwrap_err(),
            Error::ClientSecretInvalid
        );
    }

    #[test]
    fn saml_fields_are_never_parsed_as_an_oidc_provider() {
        let request = created(json!({ "protocol": "saml", "display_name": "Okta" }));
        assert_eq!(
            zone_enterprise::sso::Provider::parse(&request.fields()).unwrap_err(),
            Error::ProtocolMismatch
        );
    }

    #[test]
    fn an_update_request_carries_only_what_was_sent() {
        let request: UpdateProviderRequest =
            serde_json::from_value(json!({ "enabled": false, "display_name": "Okta SSO" }))
                .unwrap();
        assert_eq!(
            request.revision(),
            Revision {
                display_name: Some("Okta SSO"),
                enabled: Some(false),
                ..Revision::default()
            }
        );
    }

    #[test]
    fn an_update_request_carries_a_metadata_refresh() {
        let request: UpdateProviderRequest =
            serde_json::from_value(json!({ "metadata_url": METADATA_URL })).unwrap();
        let revision = request.revision();
        assert_eq!(
            revision,
            Revision {
                metadata_url: Some(METADATA_URL),
                ..Revision::default()
            }
        );
        assert!(revision.only_refreshes_metadata());
        let pasted: UpdateProviderRequest =
            serde_json::from_value(json!({ "metadata": "<md:EntityDescriptor/>" })).unwrap();
        assert_eq!(pasted.revision().metadata, Some("<md:EntityDescriptor/>"));
    }

    #[test]
    fn debug_never_shows_a_request_secret_or_pasted_metadata() {
        let create = created(json!({
            "display_name": "Okta", "issuer": "https://acme.okta.com",
            "client_id": "0oa1", "client_secret": "s3cret",
            "metadata": "<md:EntityDescriptor/>"
        }));
        let update: UpdateProviderRequest = serde_json::from_value(
            json!({ "client_secret": "s3cret", "metadata": "<md:EntityDescriptor/>" }),
        )
        .unwrap();
        for shown in [format!("{create:?}"), format!("{update:?}")] {
            assert!(!shown.contains("s3cret"), "{shown}");
            assert!(!shown.contains("EntityDescriptor"), "{shown}");
            assert!(shown.contains(REDACTED), "{shown}");
        }
    }
}
