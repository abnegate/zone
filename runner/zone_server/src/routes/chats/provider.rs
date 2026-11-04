//! The AI provider one chat runs on, chosen by its members from those its
//! workspace can use.

mod choose_request;
mod providers_response;
mod selection;
mod workspace_providers_response;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use uuid::Uuid;

use super::{
    SingleChatResponse, chat_with_messages, check_workspace_read_access,
    check_workspace_write_access, get_chat_with_access,
};
use crate::auth::AuthUser;
use crate::db::ai_settings::EffectiveAiSettings;
use crate::db::chats::{self, provider::Choice};
use crate::error::ServerError;
use crate::routes::common::ErrorResponse;
use crate::services::provider::{self, Offer, Provider, Refusal};
use crate::services::route::{self, Route};
use crate::state::AppState;
use choose_request::ChooseRequest;
use providers_response::ProvidersResponse;
use selection::Selection;
use workspace_providers_response::WorkspaceProvidersResponse;

const CHAT_WITHOUT_WORKSPACE: &str = "Chat has no workspace association";
const CHAT_NOT_FOUND: &str = "Chat not found";
const WORKSPACE_NOT_FOUND: &str = "Workspace not found";
const INTERNAL: &str = "Internal server error";

/// GET /api/workspaces/{id}/chat-providers
pub async fn workspace(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<WorkspaceProvidersResponse>, ServerError> {
    check_workspace_read_access(&state, &auth, id).await?;
    let (organization, settings) = saved(&state, id).await?;
    let offers = provider::offers(&state, organization, &settings).await?;
    let (default, providers) = split(offers, &settings);
    Ok(Json(WorkspaceProvidersResponse { default, providers }))
}

/// GET /api/chats/{id}/providers
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ProvidersResponse>, ServerError> {
    let chat = get_chat_with_access(&state, &auth, id).await?;
    let workspace = chat
        .workspace_id
        .ok_or_else(|| ServerError::Forbidden(CHAT_WITHOUT_WORKSPACE.to_string()))?;
    let (organization, settings) = saved(&state, workspace).await?;
    let choice = chats::provider::get(state.db(), id).await?;
    let offers = provider::offers(&state, organization, &settings).await?;
    let agent = Route::of(
        state.config(),
        workspace,
        organization,
        settings.clone(),
        choice.clone(),
    )
    .agent(state.config())
    .map(Provider::of);
    let selected = choice.map(|choice| Selection {
        offer: offered(&offers, choice.provider),
        model: choice.model,
    });
    let (default, providers) = split(offers, &settings);
    Ok(Json(ProvidersResponse {
        default,
        selected,
        providers,
        agent,
    }))
}

/// PUT /api/chats/{id}/provider
pub async fn choose(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(request): Json<ChooseRequest>,
) -> Response {
    let chat = match get_chat_with_access(&state, &auth, id).await {
        Ok(chat) => chat,
        Err(error) => return error.into_response(),
    };
    let Some(workspace) = chat.workspace_id else {
        return ServerError::Forbidden(CHAT_WITHOUT_WORKSPACE.to_string()).into_response();
    };
    if let Err(error) = check_workspace_write_access(&state, &auth, workspace).await {
        return error.into_response();
    }
    let choice = match chosen(
        &state,
        workspace,
        request.provider.as_deref(),
        request.model.as_deref(),
    )
    .await
    {
        Ok(choice) => choice,
        Err(refusal) => return refused(refusal),
    };
    match chats::provider::set(state.db(), id, choice.as_ref()).await {
        Ok(true) => {}
        Ok(false) => return ServerError::NotFound(CHAT_NOT_FOUND.to_string()).into_response(),
        Err(error) => {
            tracing::error!(chat = %id, %error, "Could not save the chat's provider");
            return ServerError::Internal(INTERNAL.to_string()).into_response();
        }
    }
    match chats::get_chat(state.db(), id).await {
        Ok(Some(chat)) => Json(SingleChatResponse {
            chat: chat_with_messages(&state, &auth, chat).await,
        })
        .into_response(),
        Ok(None) => ServerError::NotFound(CHAT_NOT_FOUND.to_string()).into_response(),
        Err(error) => {
            tracing::error!(chat = %id, %error, "Could not read the chat back");
            ServerError::Internal(INTERNAL.to_string()).into_response()
        }
    }
}

/// The choice `provider` and `model` make for a chat in `workspace`, checked
/// against what the workspace can use. No provider follows the workspace.
pub(super) async fn chosen(
    state: &AppState,
    workspace: Uuid,
    provider: Option<&str>,
    model: Option<&str>,
) -> Result<Option<Choice>, Refusal> {
    let Some(provider) = provider else {
        return match model {
            Some(_) => Err(Refusal::Orphaned),
            None => Ok(None),
        };
    };
    let (organization, settings) = saved(state, workspace)
        .await
        .map_err(|_| Refusal::Unreadable)?;
    provider::choose(state, organization, &settings, provider, model)
        .await
        .map(Some)
}

pub(super) fn refused(refusal: Refusal) -> Response {
    let status = match refusal {
        Refusal::Unavailable(..) => StatusCode::CONFLICT,
        Refusal::Unreadable => StatusCode::INTERNAL_SERVER_ERROR,
        Refusal::Unnamed
        | Refusal::Blank
        | Refusal::Orphaned
        | Refusal::Unoffered(_)
        | Refusal::Unset(_) => StatusCode::BAD_REQUEST,
    };
    let message = refusal.to_string();
    let body = match refusal.code() {
        Some(code) => ErrorResponse::with_code(message, code),
        None => ErrorResponse::new(message),
    };
    (status, Json(body)).into_response()
}

/// The organization `workspace` belongs to and its effective AI settings.
async fn saved(
    state: &AppState,
    workspace: Uuid,
) -> Result<(Uuid, EffectiveAiSettings), ServerError> {
    match route::saved(state, workspace).await {
        Ok(Some(saved)) => Ok(saved),
        Ok(None) => Err(ServerError::NotFound(WORKSPACE_NOT_FOUND.to_string())),
        Err(_) => Err(ServerError::Internal(INTERNAL.to_string())),
    }
}

/// The workspace's default offer, reported however it stands, and every
/// offer a chat can choose.
fn split(offers: Vec<Offer>, settings: &EffectiveAiSettings) -> (Offer, Vec<Offer>) {
    let default = offered(
        &offers,
        Provider::named(&settings.provider).unwrap_or(Provider::SelfHosted),
    );
    let providers = offers
        .into_iter()
        .filter(|offer| offer.usable && offer.provider.choosable())
        .collect();
    (default, providers)
}

fn offered(offers: &[Offer], provider: Provider) -> Offer {
    offers
        .iter()
        .find(|offer| offer.provider == provider)
        .cloned()
        .expect("every provider is offered")
}

#[cfg(test)]
mod tests {
    use zone_context::embeddings::providers::PROVIDER_BEDROCK;

    use super::*;
    use crate::services::endpoint::testing::settings;

    fn offer(provider: Provider, usable: bool) -> Offer {
        Offer {
            provider,
            label: provider.label(),
            usable,
            reason: (!usable).then(|| "Not here.".to_string()),
            models: Vec::new(),
            efforts: &[],
            requires_model: false,
        }
    }

    fn providers(offers: &[Offer]) -> Vec<Provider> {
        offers.iter().map(|offer| offer.provider).collect()
    }

    #[test]
    fn an_unchoosable_default_is_reported_but_never_offered() {
        let offers = Provider::ALL
            .map(|provider| offer(provider, provider != Provider::Anthropic))
            .to_vec();

        let (default, offered) = split(offers, &settings(PROVIDER_BEDROCK));

        assert_eq!(default.provider, Provider::Bedrock);
        assert_eq!(
            providers(&offered),
            [
                Provider::SelfHosted,
                Provider::OpenAI,
                Provider::ClaudeCode,
                Provider::Codex
            ],
            "only usable, choosable providers are offered, in order"
        );
    }

    #[test]
    fn an_unusable_default_is_reported_but_never_offered() {
        let offers = Provider::ALL
            .map(|provider| offer(provider, provider == Provider::SelfHosted))
            .to_vec();
        let (default, offered) = split(offers, &settings(Provider::Anthropic.as_str()));

        assert_eq!(default.provider, Provider::Anthropic);
        assert!(!default.usable);
        assert_eq!(providers(&offered), [Provider::SelfHosted]);
    }
}
