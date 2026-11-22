use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::{ExtensionRejection, PathRejection};
use axum::extract::{ConnectInfo, Path, State};
use axum::http::HeaderMap;
use axum::http::header::CACHE_CONTROL;
use axum::response::{IntoResponse, Response};
use tracing::field;
use uuid::Uuid;

use crate::bundle::{Architecture, BundleVersion, Object, Presence, StorageError};
use crate::gate::{Denial, Limited};
use crate::http::bundle_outcome::BundleOutcome;
use crate::http::bundle_response::BundleResponse;
use crate::http::context::Context;
use crate::http::distributor::Distributor;

const NO_STORE: &str = "no-store";

/// The version and architecture are parsed only after the gate, and only
/// parsed values reach S3 or the log, so a hostile path segment never does.
pub async fn respond(
    State(context): State<Arc<Context>>,
    peer: Result<ConnectInfo<SocketAddr>, ExtensionRejection>,
    path: Result<Path<(String, String)>, PathRejection>,
    headers: HeaderMap,
) -> Response {
    let Some(distributor) = &context.distributor else {
        let outcome = BundleOutcome::Disabled;
        tracing::info!(%outcome, "Refused a bundle because bundles are off");
        return outcome.status().into_response();
    };
    let address = distributor
        .gate
        .client(peer.ok().map(|ConnectInfo(peer)| peer), &headers);
    if let Some(client) = address
        && let Err(limited) = distributor.throttle.check_address(client)
    {
        return throttled(limited, None, address);
    }
    let license = match distributor.gate.admit(&headers) {
        Ok(license) => license.id(),
        Err(denial) => return denied(denial, address),
    };
    if let Err(limited) = distributor.throttle.check_license(license) {
        return throttled(limited, Some(license), address);
    }
    let (version, architecture) = match path {
        Ok(Path((version, architecture))) => (
            BundleVersion::parse(&version),
            Architecture::parse(&architecture),
        ),
        Err(_) => (None, None),
    };
    let (Some(version), Some(architecture)) = (version, architecture) else {
        let outcome = BundleOutcome::Unknown;
        tracing::info!(
            %outcome,
            %license,
            address = address.map(field::display),
            version = version.map(field::display),
            architecture = architecture.map(field::display),
            "Refused a bundle for a version or architecture that doesn't exist"
        );
        return outcome.status().into_response();
    };
    distribute(distributor, license, address, version, architecture).await
}

async fn distribute(
    distributor: &Distributor,
    license: Uuid,
    address: Option<IpAddr>,
    version: BundleVersion,
    architecture: Architecture,
) -> Response {
    let marker = Object::marker(version, architecture);
    match distributor.presigner.probe(&marker).await {
        Ok(Presence::Present) => {
            let body = BundleResponse::presign(&distributor.presigner, version, architecture);
            let outcome = BundleOutcome::Issued;
            tracing::info!(
                %outcome,
                %license,
                address = address.map(field::display),
                %version,
                %architecture,
                "Issued bundle download links"
            );
            (outcome.status(), [(CACHE_CONTROL, NO_STORE)], Json(body)).into_response()
        }
        Ok(Presence::Absent { code }) => {
            let outcome = BundleOutcome::Unknown;
            tracing::info!(
                %outcome,
                %license,
                address = address.map(field::display),
                %version,
                %architecture,
                code = code.as_deref(),
                "Refused a bundle that isn't in storage"
            );
            outcome.status().into_response()
        }
        Err(error) => unavailable(&error, license, address, version, architecture),
    }
}

fn unavailable(
    error: &StorageError,
    license: Uuid,
    address: Option<IpAddr>,
    version: BundleVersion,
    architecture: Architecture,
) -> Response {
    let outcome = BundleOutcome::Unavailable;
    match error {
        StorageError::Unreachable => tracing::warn!(
            %outcome,
            %license,
            address = address.map(field::display),
            %version,
            %architecture,
            cause = %error,
            "Could not reach the bundle storage"
        ),
        StorageError::Refused { code } => tracing::error!(
            %outcome,
            %license,
            address = address.map(field::display),
            %version,
            %architecture,
            code = code.as_str(),
            "The bundle storage refused the probe; check the presigner's key and policy"
        ),
        StorageError::Unexpected { .. } => tracing::error!(
            %outcome,
            %license,
            address = address.map(field::display),
            %version,
            %architecture,
            cause = %error,
            "The bundle storage answered the probe with an unexpected status"
        ),
    }
    outcome.status().into_response()
}

fn throttled(limited: Limited, license: Option<Uuid>, address: Option<IpAddr>) -> Response {
    let outcome = BundleOutcome::Limited;
    tracing::info!(
        %outcome,
        license = license.map(field::display),
        address = address.map(field::display),
        retry_after = limited.retry_after(),
        "Refused a bundle over a rate limit"
    );
    limited.respond()
}

fn denied(denial: Denial, address: Option<IpAddr>) -> Response {
    tracing::info!(
        outcome = %denial.refusal,
        license = denial.license.map(field::display),
        address = address.map(field::display),
        plan = denial.plan.map(field::display),
        status = denial.status.map(field::display),
        error = denial.error,
        "Refused a bundle at the license gate"
    );
    denial.respond()
}

#[cfg(test)]
mod tests;
