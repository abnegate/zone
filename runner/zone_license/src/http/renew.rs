use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;
use zone_enterprise::License;

use crate::http::context::Context;
use crate::http::renew_request::RenewRequest;
use crate::http::renewal_outcome::RenewalOutcome;
use crate::http::renewed::Renewed;
use crate::polar::ApiError;
use crate::renewal::{SETTLING, Verdict, decide};
use crate::terms::Terms;

/// Answers 404 whatever is posted while renewal is off, so a signer without
/// `POLAR_ACCESS_TOKEN` gives nothing away. Never sends email: the customer's
/// emailed key comes from the webhook alone.
pub async fn respond(
    State(context): State<Arc<Context>>,
    request: Result<Json<RenewRequest>, JsonRejection>,
) -> Response {
    let Some(renewer) = &context.renewer else {
        let outcome = RenewalOutcome::Disabled;
        tracing::info!(%outcome, "Refused a renewal because renewal is off");
        return outcome.status().into_response();
    };
    let request = match request {
        Ok(Json(request)) => request,
        Err(rejection) => return unreadable(&rejection),
    };
    let license = match renewer.keyring.verify(request.license.trim()) {
        Ok(license) => license,
        Err(error) => {
            let outcome = RenewalOutcome::Unverified;
            tracing::info!(%outcome, error = error.code(), "Refused a renewal for a key that doesn't verify");
            return outcome.status().into_response();
        }
    };
    let Some(subscription) = license.subscription() else {
        let outcome = RenewalOutcome::Unsubscribed;
        tracing::info!(%outcome, license = %license.id(), "Refused a renewal for a key without a subscription");
        return outcome.status().into_response();
    };
    let Ok(id) = Uuid::parse_str(subscription) else {
        let outcome = RenewalOutcome::Malformed;
        tracing::info!(
            %outcome,
            subscription,
            license = %license.id(),
            "Refused a renewal for a subscription id that isn't a UUID"
        );
        return outcome.status().into_response();
    };
    let found = match renewer.client.subscription(id).await {
        Ok(found) => found,
        Err(error) => return unanswered(error, &license, subscription),
    };
    let status = found.as_ref().map(|found| found.status.as_str());
    let now = (renewer.clock)();
    match decide(&license, found.as_ref(), &context.products, now) {
        Verdict::Renew(terms) => sign(&context, &license, status, &terms),
        Verdict::Current => {
            let outcome = RenewalOutcome::Current;
            tracing::info!(%outcome, subscription, license = %license.id(), status, "Had no newer license to renew to");
            outcome.status().into_response()
        }
        Verdict::Settling => {
            let outcome = RenewalOutcome::Current;
            tracing::info!(
                %outcome,
                subscription,
                license = %license.id(),
                status,
                reason = SETTLING,
                "Held a renewal until Polar has had time to collect the new period's payment"
            );
            outcome.status().into_response()
        }
        Verdict::Ended(reason) => {
            let outcome = RenewalOutcome::Ended;
            tracing::info!(
                %outcome,
                subscription,
                license = %license.id(),
                status,
                %reason,
                "Refused a renewal for a subscription that won't renew"
            );
            outcome.status().into_response()
        }
        Verdict::Misconfigured => {
            let outcome = RenewalOutcome::Misconfigured;
            tracing::error!(
                %outcome,
                subscription,
                license = %license.id(),
                status,
                product = found.as_ref().and_then(|found| found.product_id.as_deref()),
                "A subscription for a listed product has no seats; make the product seat-based"
            );
            outcome.status().into_response()
        }
    }
}

fn unreadable(rejection: &JsonRejection) -> Response {
    let status = match rejection {
        JsonRejection::BytesRejection(rejection) => rejection.status(),
        _ => RenewalOutcome::Unreadable.status(),
    };
    let outcome = RenewalOutcome::from_body(status);
    tracing::info!(%outcome, "Refused a renewal whose body can't be read");
    status.into_response()
}

fn unanswered(error: ApiError, license: &License, subscription: &str) -> Response {
    let outcome = RenewalOutcome::from(error);
    if outcome == RenewalOutcome::Unauthorized {
        tracing::error!(
            %outcome,
            subscription,
            license = %license.id(),
            %error,
            "Polar refused the access token; every renewal fails until it's replaced"
        );
    } else {
        tracing::warn!(
            %outcome,
            subscription,
            license = %license.id(),
            retryable = error.is_retryable(),
            %error,
            "Could not read a subscription from Polar"
        );
    }
    outcome.status().into_response()
}

fn sign(context: &Context, license: &License, status: Option<&str>, terms: &Terms) -> Response {
    let subscription = terms.subscription.as_deref();
    let key = match context.signer.sign(terms) {
        Ok(key) => key,
        Err(error) => {
            let outcome = RenewalOutcome::Unsignable;
            tracing::error!(
                %outcome,
                subscription,
                license = %license.id(),
                status,
                renewed = %terms.id,
                error = error.code(),
                "Could not sign a renewed license"
            );
            return outcome.status().into_response();
        }
    };
    let outcome = RenewalOutcome::Renewed;
    tracing::info!(
        %outcome,
        subscription,
        license = %license.id(),
        status,
        renewed = %terms.id,
        seats = terms.seats,
        "Renewed a license"
    );
    (outcome.status(), Json(Renewed { license: key })).into_response()
}

#[cfg(test)]
mod tests;
