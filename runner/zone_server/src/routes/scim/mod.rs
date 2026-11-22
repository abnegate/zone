//! The SCIM 2.0 service provider an identity provider pushes users and groups to, one per
//! organization. Every route needs a SCIM bearer token of that organization; past the license,
//! only deprovisioning writes are accepted.

pub mod console;
pub mod discovery;
pub mod document;
pub mod groups;
pub mod identifier;
pub mod refusal;
pub mod response;
pub mod users;

use axum::Router;
use axum::extract::rejection::QueryRejection;
use axum::extract::{MatchedPath, Query, Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use uuid::Uuid;
use zone_enterprise::scim::{self, Error, ListQuery};

use crate::services::scim::{Failure, Slot, record};
use crate::services::sso::console::callback_origin;
use crate::state::AppState;

pub const BASE_PATH: &str = "/api/scim/v2/organizations";

const UNMATCHED_PATH: &str = "unmatched";

pub fn base_url(app_base_url: &str, organization_id: Uuid) -> String {
    format!(
        "{}{BASE_PATH}/{organization_id}",
        callback_origin(app_base_url)
    )
}

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/{org_id}/ServiceProviderConfig",
            get(discovery::service_provider_config),
        )
        .route("/{org_id}/ResourceTypes", get(discovery::resource_types))
        .route(
            "/{org_id}/ResourceTypes/{name}",
            get(discovery::resource_type),
        )
        .route("/{org_id}/Schemas", get(discovery::schemas))
        .route("/{org_id}/Schemas/{urn}", get(discovery::schema))
        .route("/{org_id}/Users", get(users::list).post(users::create))
        .route(
            "/{org_id}/Users/{user_id}",
            get(users::get)
                .put(users::replace)
                .patch(users::patch)
                .delete(users::delete),
        )
        .route("/{org_id}/Groups", get(groups::list).post(groups::create))
        .route(
            "/{org_id}/Groups/{group_id}",
            get(groups::get)
                .put(groups::replace)
                .patch(groups::patch)
                .delete(groups::delete),
        )
        .fallback(unrouted)
        .layer(middleware::from_fn_with_state(state, exchange))
}

pub(crate) fn listing(
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<ListQuery, Failure> {
    query
        .map(|Query(query)| query)
        .map_err(|rejection| Failure::Protocol(Error::InvalidSyntax(rejection.body_text())))
}

async fn unrouted() -> Failure {
    Failure::Protocol(Error::NotFound)
}

async fn exchange(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    let slot = Slot::default();
    request.extensions_mut().insert(slot.clone());
    let method = request.method().to_string();
    let path = request
        .extensions()
        .get::<MatchedPath>()
        .map_or(UNMATCHED_PATH, MatchedPath::as_str)
        .to_owned();

    let response = refusal::conform(next.run(request).await);
    if let Some(client) = slot.client()
        && let Some(failure) = response.extensions().get::<scim::Failure>()
    {
        record::failure(&state, client, &method, &path, failure).await;
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_base_url_is_the_console_origin_and_the_organization() {
        let organization = Uuid::new_v4();
        assert_eq!(
            base_url("https://zone.example/app/", organization),
            format!("https://zone.example/api/scim/v2/organizations/{organization}")
        );
    }
}
