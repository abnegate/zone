use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use chrono::Utc;

use crate::decision::{Decision, decide};
use crate::http::context::Context;
use crate::http::issued::Issued;
use crate::http::outcome::Outcome;
use crate::http::skipped::Skipped;
use crate::mail::Letter;
use crate::order_error::OrderError;
use crate::polar::{self, Event, Headers};

/// Answers `2xx` for an issuing order only once the relay has taken the
/// email, so a `2xx` always means the customer has their key.
pub async fn receive(
    State(context): State<Arc<Context>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let signed = Headers::read(|name| headers.get(name).and_then(|value| value.to_str().ok()));
    if let Err(error) = polar::verify(&context.secret, &signed, &body, Utc::now()) {
        let outcome = Outcome::Unsigned;
        tracing::info!(%outcome, %error, "Refused a webhook delivery");
        return outcome.status().into_response();
    }
    let webhook = signed.id.as_deref().unwrap_or_default();

    let event = match Event::parse(&body) {
        Ok(event) => event,
        Err(error) => {
            let outcome = Outcome::Malformed;
            tracing::warn!(webhook, %outcome, %error, "Could not read a webhook delivery");
            return outcome.status().into_response();
        }
    };
    let grant = match decide(&event, &context.products) {
        Ok(Decision::Issue(grant)) => grant,
        Ok(Decision::Ignore(reason)) => {
            let outcome = Outcome::Ignored;
            tracing::info!(
                webhook,
                event = event.kind.as_str(),
                %outcome,
                reason = reason.as_str(),
                "Acknowledged a webhook delivery that issues nothing"
            );
            return (outcome.status(), Json(Skipped::from(reason))).into_response();
        }
        Err(error) => {
            let outcome = Outcome::from(&error);
            match &error {
                OrderError::Misconfigured { order, product } => tracing::error!(
                    webhook,
                    order,
                    product,
                    %outcome,
                    %error,
                    "A paid order for a listed product issued nothing; hand-sign it"
                ),
                OrderError::Email { order } => tracing::error!(
                    webhook,
                    order,
                    %outcome,
                    %error,
                    "A paid order has no address to send its license to; hand-sign it"
                ),
                OrderError::Malformed => {
                    tracing::warn!(webhook, %outcome, %error, "Could not read a paid order");
                }
            }
            return outcome.status().into_response();
        }
    };

    let terms = grant.terms();
    let order = grant.order.as_str();
    let subscription = grant.subscription.as_str();
    let license = terms.id;
    let key = match context.signer.sign(&terms) {
        Ok(key) => key,
        Err(error) => {
            let outcome = Outcome::Unsignable;
            tracing::error!(webhook, order, subscription, %license, %outcome, %error, "Could not sign a license");
            return outcome.status().into_response();
        }
    };

    let letter = Letter::license(
        &terms.customer,
        terms.plan,
        terms.seats,
        terms.expires_at,
        &key,
    );
    if let Err(error) = context.outbox.send(&grant.email, &letter).await {
        let outcome = Outcome::from(&error);
        tracing::error!(
            webhook,
            order,
            subscription,
            %license,
            %outcome,
            retryable = error.is_retryable(),
            %error,
            "Could not email a license"
        );
        return outcome.status().into_response();
    }

    let outcome = Outcome::Sent;
    tracing::info!(webhook, order, subscription, %license, seats = terms.seats, plan = %terms.plan, %outcome, "Sent a license");
    (outcome.status(), Json(Issued { license })).into_response()
}
