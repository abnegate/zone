use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::response::Response;
use zone_enterprise::scim::{ListQuery, PatchRequest, UserChanges, UserDocument};

use super::document::Document;
use super::identifier::Identifier;
use super::{listing, response};
use crate::services::scim::{Failure, ScimClient, ScimWriter, users};
use crate::state::AppState;

pub async fn list(
    client: ScimClient,
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Response, Failure> {
    let query = listing(query)?;
    Ok(response::ok(&users::list(&state, &client, &query).await?))
}

pub async fn create(
    ScimWriter(client): ScimWriter,
    State(state): State<AppState>,
    Document(document): Document<UserDocument>,
) -> Result<Response, Failure> {
    let resource = users::create(&state, &client, &document).await?;
    Ok(response::created(&resource, &resource.meta.location))
}

pub async fn get(
    client: ScimClient,
    State(state): State<AppState>,
    Identifier(user_id): Identifier,
) -> Result<Response, Failure> {
    Ok(response::ok(&users::get(&state, &client, user_id).await?))
}

pub async fn replace(
    client: ScimClient,
    State(state): State<AppState>,
    Identifier(user_id): Identifier,
    Document(document): Document<UserDocument>,
) -> Result<Response, Failure> {
    document.validate()?;
    let changes = users::narrow(client.entitlement, UserChanges::from_document(&document))?;
    Ok(response::ok(
        &users::update(&state, &client, user_id, changes).await?,
    ))
}

pub async fn patch(
    client: ScimClient,
    State(state): State<AppState>,
    Identifier(user_id): Identifier,
    Document(request): Document<PatchRequest>,
) -> Result<Response, Failure> {
    let changes = users::narrow(client.entitlement, UserChanges::from_patch(&request)?)?;
    Ok(response::ok(
        &users::update(&state, &client, user_id, changes).await?,
    ))
}

pub async fn delete(
    client: ScimClient,
    State(state): State<AppState>,
    Identifier(user_id): Identifier,
) -> Result<Response, Failure> {
    users::delete(&state, &client, user_id).await?;
    Ok(response::no_content())
}
