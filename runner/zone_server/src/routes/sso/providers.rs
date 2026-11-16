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
use crate::db::sso::ProviderRecord;
use crate::services::sso::{Revision, providers};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ProviderResponse {
    pub id: Uuid,
    pub protocol: Protocol,
    pub display_name: String,
    pub issuer: String,
    pub client_id: String,
    pub allowed_domains: Vec<String>,
    pub require_verified_email: bool,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&ProviderRecord> for ProviderResponse {
    fn from(record: &ProviderRecord) -> Self {
        Self {
            id: record.id,
            protocol: record.protocol,
            display_name: record.display_name.clone(),
            issuer: record.issuer.clone(),
            client_id: record.client_id.clone(),
            allowed_domains: record.allowed_domains.clone(),
            require_verified_email: record.require_verified_email,
            enabled: record.enabled,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

/// A missing text field reads as empty, so it is refused with that field's own code rather than
/// a generic body error.
#[derive(Deserialize)]
pub struct CreateProviderRequest {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub issuer: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default = "on")]
    pub require_verified_email: bool,
    #[serde(default = "on")]
    pub enabled: bool,
}

impl CreateProviderRequest {
    fn fields(&self) -> Fields<'_> {
        Fields {
            protocol: Protocol::Oidc,
            display_name: &self.display_name,
            issuer: &self.issuer,
            client_id: &self.client_id,
            client_secret: &self.client_secret,
            allowed_domains: &self.allowed_domains,
            require_verified_email: self.require_verified_email,
        }
    }
}

impl fmt::Debug for CreateProviderRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateProviderRequest")
            .field("display_name", &self.display_name)
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("client_secret", &REDACTED)
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
            .field("display_name", &self.display_name)
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| REDACTED),
            )
            .field("allowed_domains", &self.allowed_domains)
            .field("require_verified_email", &self.require_verified_email)
            .field("enabled", &self.enabled)
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
    match providers::create(&state, &admin, &request.fields(), request.enabled).await {
        Ok(record) => (StatusCode::CREATED, Json(ProviderResponse::from(&record))).into_response(),
        Err(failure) => failed(failure),
    }
}

pub async fn update(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<UpdateProviderRequest>, JsonRejection>,
) -> Response {
    if let Err(unentitled) = entitlement::require(&state, Feature::Sso).await {
        return unentitled.into_response();
    }
    let Ok(Path(id)) = path else {
        return missing();
    };
    let Json(request) = match body {
        Ok(body) => body,
        Err(rejection) => return unreadable(&rejection),
    };
    match providers::update(&state, &admin, id, &request.revision()).await {
        Ok(record) => Json(ProviderResponse::from(&record)).into_response(),
        Err(failure) => failed(failure),
    }
}

pub async fn delete(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    if let Err(unentitled) = entitlement::require(&state, Feature::Sso).await {
        return unentitled.into_response();
    }
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

    use super::*;

    #[test]
    fn a_provider_is_sent_without_its_secret() {
        let record = ProviderRecord {
            id: Uuid::nil(),
            protocol: Protocol::Oidc,
            display_name: "Okta".into(),
            issuer: "https://acme.okta.com".into(),
            client_id: "0oa1".into(),
            sealed_secret: "ENC[v1:s3cret]".into(),
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
    fn a_create_request_defaults_to_an_enabled_provider_that_requires_verified_email() {
        let request: CreateProviderRequest = serde_json::from_value(json!({
            "display_name": "Okta", "issuer": "https://acme.okta.com",
            "client_id": "0oa1", "client_secret": "s3cret"
        }))
        .unwrap();
        assert!(request.enabled);
        assert!(request.require_verified_email);
        assert!(request.allowed_domains.is_empty());
        let fields = request.fields();
        assert_eq!(fields.protocol, Protocol::Oidc);
        assert_eq!(fields.client_secret, "s3cret");
    }

    #[test]
    fn a_missing_secret_reads_as_empty_so_it_gets_its_own_code() {
        let request: CreateProviderRequest = serde_json::from_value(json!({
            "display_name": "Okta", "issuer": "https://acme.okta.com", "client_id": "0oa1"
        }))
        .unwrap();
        assert_eq!(
            zone_enterprise::sso::Provider::parse(&request.fields()).unwrap_err(),
            Error::ClientSecretInvalid
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
    fn debug_never_shows_a_request_secret() {
        let create: CreateProviderRequest = serde_json::from_value(json!({
            "display_name": "Okta", "issuer": "https://acme.okta.com",
            "client_id": "0oa1", "client_secret": "s3cret"
        }))
        .unwrap();
        let update: UpdateProviderRequest =
            serde_json::from_value(json!({ "client_secret": "s3cret" })).unwrap();
        for shown in [format!("{create:?}"), format!("{update:?}")] {
            assert!(!shown.contains("s3cret"), "{shown}");
            assert!(shown.contains(REDACTED), "{shown}");
        }
    }
}
