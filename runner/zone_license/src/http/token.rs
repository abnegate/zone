use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::ExtensionRejection;
use axum::extract::{ConnectInfo, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;
use zone_enterprise::License;

use crate::http::admitted::Admitted;
use crate::http::context::Context;
use crate::http::registrar::Registrar;
use crate::http::token_outcome::TokenOutcome;
use crate::http::token_response::TokenResponse;
use crate::registry::{Scopes, grant, mint};

pub const SERVICE_PARAMETER: &str = "service";
pub const SCOPE_PARAMETER: &str = "scope";

const UNADDRESSED_MESSAGE: &str = "The client address can't be read.";

/// The Docker registry token endpoint. Parameters other than `service` and
/// `scope`, such as the `account`, `client_id` and `offline_token` that
/// Docker adds, are ignored.
pub async fn respond(
    State(context): State<Arc<Context>>,
    peer: Result<ConnectInfo<SocketAddr>, ExtensionRejection>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let Some(registrar) = &context.registrar else {
        let outcome = TokenOutcome::Disabled;
        tracing::info!(%outcome, "Refused a token request because registry tokens are off");
        return outcome.status().into_response();
    };
    let peer = peer.ok().map(|ConnectInfo(peer)| peer);
    let Some(address) = registrar.gate.client(peer, &headers) else {
        let outcome = TokenOutcome::Unreadable;
        tracing::error!(
            %outcome,
            "Refused a token request with no peer address; the server must be run with connect info"
        );
        return (outcome.status(), UNADDRESSED_MESSAGE).into_response();
    };
    if let Err(limited) = registrar.throttle.check_address(address) {
        let outcome = TokenOutcome::Limited;
        tracing::info!(%outcome, %address, "Refused a token request over the address limit");
        return limited.respond();
    }
    let license = match registrar.gate.admit(&headers) {
        Ok(license) => license,
        Err(denial) => {
            let outcome = TokenOutcome::from(denial.refusal);
            tracing::info!(
                %outcome,
                license = denial.license.map(display),
                %address,
                plan = denial.plan.map(display),
                status = denial.status.map(display),
                error = denial.error,
                "Refused a token request at the license gate"
            );
            return denial.respond();
        }
    };
    issue(
        registrar,
        &license,
        address,
        query.as_deref().unwrap_or_default(),
    )
}

fn issue(registrar: &Registrar, license: &License, address: IpAddr, query: &str) -> Response {
    let now = (registrar.clock)();
    let admitted = Admitted {
        license: license.id(),
        address,
        plan: license.plan(),
        status: license.status(now),
    };
    if let Err(limited) = registrar.throttle.check_license(admitted.license) {
        admitted.refuse(
            TokenOutcome::Limited,
            "Refused a token request over the license limit",
        );
        return limited.respond();
    }
    let (services, scopes) = parameters(query);
    let service = registrar.service();
    if !matches!(services.as_slice(), [only] if only == service) {
        admitted.refuse(
            TokenOutcome::Unreadable,
            "Refused a token request without the registry's service",
        );
        let message = format!("Send {SERVICE_PARAMETER}={service} once.");
        return (TokenOutcome::Unreadable.status(), message).into_response();
    }
    let scopes = match Scopes::parse(scopes.iter().map(String::as_str)) {
        Ok(scopes) => scopes,
        Err(error) => {
            admitted.refuse(
                TokenOutcome::Unreadable,
                "Refused a token request whose scope can't be parsed",
            );
            return (TokenOutcome::Unreadable.status(), error.to_string()).into_response();
        }
    };
    let grant = grant(&scopes, &registrar.settings.repositories);
    let token = mint(
        &registrar.key,
        service,
        admitted.license,
        &grant.access,
        now,
        Uuid::new_v4(),
    );
    let outcome = TokenOutcome::Issued;
    tracing::info!(
        %outcome,
        license = %admitted.license,
        address = %admitted.address,
        granted = %grant.granted(),
        dropped = grant.dropped,
        plan = %admitted.plan,
        status = %admitted.status,
        "Issued a registry token"
    );
    (outcome.status(), Json(TokenResponse::new(token, now))).into_response()
}

fn parameters(query: &str) -> (Vec<String>, Vec<String>) {
    let mut services = Vec::new();
    let mut scopes = Vec::new();
    for (name, value) in form_urlencoded::parse(query.as_bytes()) {
        match name.as_ref() {
            SERVICE_PARAMETER => services.push(value.into_owned()),
            SCOPE_PARAMETER => scopes.push(value.into_owned()),
            _ => {}
        }
    }
    (services, scopes)
}

#[cfg(test)]
mod tests;
