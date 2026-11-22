use std::collections::HashMap;

use axum::extract::{FromRef, FromRequestParts, Path};
use axum::http::request::Parts;
use axum::http::{HeaderMap, header};
use chrono::Utc;
use uuid::Uuid;
use zone_enterprise::scim::Error;

use super::entitlement::Entitlement;
use super::failure::Failure;
use super::slot::Slot;
use super::{limit, secret};
use crate::db::organizations;
use crate::db::scim::{TokenCredential, tokens};
use crate::state::AppState;

pub const ORGANIZATION_PARAMETER: &str = "org_id";

const BEARER: &str = "bearer";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScimClient {
    pub organization_id: Uuid,
    pub token_id: Uuid,
    pub token_name: String,
    pub entitlement: Entitlement,
}

impl<S> FromRequestParts<S> for ScimClient
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = Failure;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        if let Some(client) = parts.extensions.get::<Self>() {
            return Ok(client.clone());
        }
        let state = AppState::from_ref(state);
        let organization_id = organization(parts, &state).await?;
        let secret = bearer(&parts.headers).ok_or(Failure::Unauthenticated)?;
        let credential = authenticate(&state, organization_id, secret).await?;

        if let Err(error) = tokens::touch(state.db(), credential.id).await {
            tracing::warn!(%error, token_id = %credential.id, "Failed to record a SCIM token's use");
        }

        let license = state.license().await;
        let client = Self {
            organization_id,
            token_id: credential.id,
            token_name: credential.name,
            entitlement: Entitlement::of(license.as_ref(), Utc::now()),
        };
        if let Some(slot) = parts.extensions.get::<Slot>() {
            slot.fill(&client);
        }

        let limiter = state.scim_limiter();
        let decision = limiter.check_rate_limit(organization_id);
        if !decision.allowed {
            return Err(Failure::Throttled {
                retry_after: limit::retry_after(decision.reset_at, limiter.config().window),
            });
        }

        parts.extensions.insert(client.clone());
        Ok(client)
    }
}

async fn organization(parts: &mut Parts, state: &AppState) -> Result<Uuid, Failure> {
    let Path(parameters) = Path::<HashMap<String, String>>::from_request_parts(parts, state)
        .await
        .map_err(|_| Failure::Protocol(Error::NotFound))?;
    parameters
        .get(ORGANIZATION_PARAMETER)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or(Failure::Protocol(Error::NotFound))
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, secret) = value.trim().split_once(' ')?;
    let secret = secret.trim();
    (scheme.eq_ignore_ascii_case(BEARER) && secret.starts_with(secret::PREFIX)).then_some(secret)
}

async fn authenticate(
    state: &AppState,
    organization_id: Uuid,
    secret: &str,
) -> Result<TokenCredential, Failure> {
    let credentials = tokens::active(state.db(), organization_id).await?;
    let matched = credentials
        .into_iter()
        .fold(None, |matched: Option<TokenCredential>, credential| {
            let matches = secret::matches(&credential.token_hash, secret);
            match matched {
                Some(found) => Some(found),
                None if matches => Some(credential),
                None => None,
            }
        })
        .ok_or(Failure::Unauthenticated)?;

    let active = organizations::get_organization(state.db(), organization_id)
        .await?
        .is_some_and(|organization| organization.is_active.unwrap_or(true));
    if !active {
        return Err(Failure::Unauthenticated);
    }
    Ok(matched)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn headers(authorization: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(authorization).unwrap(),
        );
        headers
    }

    #[test]
    fn a_bearer_secret_is_read_whatever_the_scheme_case() {
        let secret = secret::generate();
        for authorization in [
            format!("Bearer {secret}"),
            format!("bearer {secret}"),
            format!("BEARER  {secret} "),
        ] {
            assert_eq!(bearer(&headers(&authorization)), Some(secret.as_str()));
        }
    }

    #[test]
    fn anything_but_a_scim_bearer_secret_is_refused() {
        let secret = secret::generate();
        for authorization in [
            format!("Basic {secret}"),
            "Bearer ".to_owned(),
            "Bearer eyJhbGciOiJIUzI1NiJ9.e30.signature".to_owned(),
            secret.clone(),
        ] {
            assert_eq!(bearer(&headers(&authorization)), None, "{authorization}");
        }
        assert_eq!(bearer(&HeaderMap::new()), None);
    }
}
