use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::response::Response;
use zone_enterprise::scim::{GroupDocument, ListQuery, PatchRequest};

use super::document::Document;
use super::identifier::Identifier;
use super::{listing, response};
use crate::services::scim::{Failure, ScimClient, ScimWriter, groups};
use crate::state::AppState;

pub async fn list(
    client: ScimClient,
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Response, Failure> {
    let query = listing(query)?;
    Ok(response::ok(&groups::list(&state, &client, &query).await?))
}

pub async fn create(
    ScimWriter(client): ScimWriter,
    State(state): State<AppState>,
    Document(document): Document<GroupDocument>,
) -> Result<Response, Failure> {
    let resource = groups::create(&state, &client, &document).await?;
    Ok(response::created(&resource, &resource.meta.location))
}

pub async fn get(
    client: ScimClient,
    State(state): State<AppState>,
    Identifier(group_id): Identifier,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Response, Failure> {
    let query = listing(query)?;
    Ok(response::ok(
        &groups::get(&state, &client, group_id, query.excludes_members()).await?,
    ))
}

pub async fn replace(
    ScimWriter(client): ScimWriter,
    State(state): State<AppState>,
    Identifier(group_id): Identifier,
    Document(document): Document<GroupDocument>,
) -> Result<Response, Failure> {
    Ok(response::ok(
        &groups::replace(&state, &client, group_id, &document).await?,
    ))
}

pub async fn patch(
    ScimWriter(client): ScimWriter,
    State(state): State<AppState>,
    Identifier(group_id): Identifier,
    Document(request): Document<PatchRequest>,
) -> Result<Response, Failure> {
    Ok(response::ok(
        &groups::patch(&state, &client, group_id, &request).await?,
    ))
}

pub async fn delete(
    ScimWriter(client): ScimWriter,
    State(state): State<AppState>,
    Identifier(group_id): Identifier,
) -> Result<Response, Failure> {
    groups::delete(&state, &client, group_id).await?;
    Ok(response::no_content())
}
