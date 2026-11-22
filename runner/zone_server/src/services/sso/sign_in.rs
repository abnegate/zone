use axum::http::HeaderMap;
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_enterprise::sso::saml::{self, Refusal};
use zone_enterprise::sso::{Error, Grant, Identity, Provider, Resolution, resolve};

use super::callback::Callback;
use super::configured::Configured;
use super::console::{redirect_uri, service_provider};
use super::handoff::{self, Handoff};
use super::parse_permit;
use super::posted::Posted;
use super::settlement::Settlement;
use super::sign_in_failure::SignInFailure;
use super::start::Start;
use crate::auth::client_address::ClientAddress;
use crate::auth::hash_password;
use crate::auth::password::PasswordError;
use crate::db::audit::{actions, resources};
use crate::db::sso::{Attempt, assertions, attempts, identities, providers};
use crate::db::users::{self, UserRow};
use crate::db::{refresh_tokens, sessions};
use crate::routes::common::{AuditEvent, audit};
use crate::services::account::{AuthResponse, Credential, provision, session};
use crate::services::members;
use crate::state::AppState;
use crate::utils::crypto::generate_token;

const PROVIDER_ID: &str = "provider_id";

const RACED: [&str; 4] = [
    users::EMAIL_KEY,
    users::EMAIL_LOWER_KEY,
    identities::SUBJECT_KEY,
    identities::USER_KEY,
];

pub async fn authorize(state: &AppState, provider_id: Uuid) -> Result<Start, SignInFailure> {
    let base = &state.config().app_base_url;
    match enabled(state, provider_id).await? {
        Configured::Oidc(provider) => {
            let authorization = state
                .oidc()
                .authorize(&provider, &redirect_uri(base))
                .await?;
            attempts::issue(state.db(), provider_id, &authorization)
                .await?
                .ok_or(SignInFailure::Busy)?;
            Ok(authorization.into())
        }
        Configured::Saml(provider) => {
            let authorization =
                saml::Authorization::new(&provider.sso_url, &service_provider(base), Utc::now());
            attempts::issue_request(
                state.db(),
                provider_id,
                &authorization.state,
                &authorization.request_id,
            )
            .await?
            .ok_or(SignInFailure::Busy)?;
            Ok(authorization.into())
        }
    }
}

/// The post claims its attempt before anything else, so a request has one response checked however
/// many posts carry its state. A refused response discards the attempt, so it can't be retried
/// against the same request; a post dropped mid-check leaves it claimed until it expires.
pub async fn assert(state: &AppState, posted: &Posted<'_>) -> Result<Handoff, SignInFailure> {
    let attempt = attempts::claim(state.db(), posted.state)
        .await?
        .ok_or(Error::StateInvalid)?;
    match hand_off(state, &attempt, posted.response).await {
        Ok(code) => Ok(Handoff {
            state: posted.state.to_owned(),
            code,
        }),
        Err(failure) => {
            if let Err(error) = attempts::discard(state.db(), attempt.id).await {
                tracing::error!(
                    attempt_id = %attempt.id,
                    %error,
                    "Failed to discard a refused SAML sign-in"
                );
            }
            Err(failure)
        }
    }
}

async fn hand_off(
    state: &AppState,
    attempt: &Attempt,
    response: &str,
) -> Result<String, SignInFailure> {
    let Configured::Saml(provider) = enabled(state, attempt.provider_id).await? else {
        return Err(Error::ProviderNotFound.into());
    };
    let request_id = attempt.request_id.clone().ok_or(Error::StateInvalid)?;
    let service = service_provider(&state.config().app_base_url);
    let response = response.to_owned();
    let permit =
        parse_permit::acquire(state.saml_semaphore(), state.config().saml_permit_wait).await?;
    let consumed = tokio::task::spawn_blocking(move || {
        let consumed = saml::consume(&provider, &service, &request_id, &response, Utc::now());
        drop(permit);
        consumed
    })
    .await
    .map_err(SignInFailure::Interrupted)??;
    if !assertions::record(
        state.db(),
        attempt.provider_id,
        &consumed.assertion_id,
        consumed.expires_at,
    )
    .await?
    {
        return Err(Error::AssertionInvalid(Refusal::Replayed).into());
    }
    let code = generate_token();
    let identity = &consumed.identity;
    if !attempts::assert(
        state.db(),
        attempt.id,
        &code,
        &identity.subject,
        &identity.email,
        identity.name.as_deref(),
    )
    .await?
    {
        return Err(Error::StateInvalid.into());
    }
    Ok(code)
}

pub async fn complete(
    state: &AppState,
    callback: &Callback<'_>,
    headers: &HeaderMap,
    address: ClientAddress,
) -> Result<AuthResponse, SignInFailure> {
    let attempt = attempts::consume(state.db(), callback.state)
        .await?
        .ok_or(Error::StateInvalid)?;
    let configured = enabled(state, attempt.provider_id).await?;
    let identity = match &configured {
        Configured::Oidc(provider) => exchange(state, provider, &attempt, callback.code).await?,
        Configured::Saml(provider) => handoff::redeem(
            attempt.asserted.as_ref().ok_or(Error::StateInvalid)?,
            callback.code,
            &provider.policy,
        )?,
    };

    let settlement = settle(state, attempt.provider_id, &configured, &identity).await?;
    if settlement.linked {
        record(
            state,
            settlement.user,
            &identity.email,
            actions::USER_SSO_LINKED,
            settlement.linkage(attempt.provider_id, &identity.subject),
        )
        .await;
    }
    if let Err(error) = users::update_last_login(state.db(), settlement.user).await {
        tracing::warn!(user_id = %settlement.user, %error, "Failed to update last login");
    }
    let response = session::issue(
        state,
        settlement.user,
        headers,
        address,
        Credential::Provider(attempt.provider_id),
    )
    .await?;
    record(
        state,
        settlement.user,
        &identity.email,
        actions::USER_SSO_SIGNED_IN,
        settlement.recorded(attempt.provider_id, &identity.subject),
    )
    .await;
    Ok(response)
}

async fn exchange(
    state: &AppState,
    provider: &Provider,
    attempt: &Attempt,
    code: &str,
) -> Result<Identity, SignInFailure> {
    let redirect_uri = redirect_uri(&state.config().app_base_url);
    Ok(state
        .oidc()
        .exchange(
            provider,
            &Grant {
                code,
                verifier: attempt.verifier.as_deref().ok_or(Error::StateInvalid)?,
                nonce: attempt.nonce.as_deref().ok_or(Error::StateInvalid)?,
                redirect_uri: &redirect_uri,
            },
        )
        .await?)
}

async fn enabled(state: &AppState, provider_id: Uuid) -> Result<Configured, SignInFailure> {
    let record = providers::get(state.db(), provider_id)
        .await?
        .filter(|record| record.enabled)
        .ok_or(Error::ProviderNotFound)?;
    Configured::parse(&record, state.master_key())
}

async fn settle(
    state: &AppState,
    provider_id: Uuid,
    configured: &Configured,
    identity: &Identity,
) -> Result<Settlement, SignInFailure> {
    match land(state, provider_id, configured, identity).await {
        Err(failure) if failure.raced(&RACED) => {}
        landed => return landed,
    }
    match land(state, provider_id, configured, identity).await {
        Err(failure) if failure.raced(&RACED) => Err(Error::EmailConflict.into()),
        landed => landed,
    }
}

async fn land(
    state: &AppState,
    provider_id: Uuid,
    configured: &Configured,
    identity: &Identity,
) -> Result<Settlement, SignInFailure> {
    let known = identities::find(state.db(), provider_id, &identity.subject).await?;
    let by_subject = match &known {
        Some(known) => users::get_user_by_id(state.db(), known.user_id)
            .await?
            .map(|user| user.account()),
        None => None,
    };
    let by_email = users::find_by_email_ignoring_case(state.db(), &identity.email).await?;
    let linked = match by_email.as_slice() {
        [user] if known.is_none() => identities::linked(state.db(), user.id).await?,
        _ => false,
    };
    let accounts: Vec<_> = by_email.iter().map(UserRow::account).collect();

    match resolve(
        by_subject,
        &accounts,
        linked,
        configured.policy().require_verified_email,
    )? {
        Resolution::SignIn(user) => {
            if let Some(known) = &known {
                identities::touch(state.db(), known.id, &identity.email).await?;
            }
            Ok(Settlement::signed_in(user))
        }
        Resolution::Link { user, reclaim } => {
            link(state, provider_id, identity, user, reclaim).await?;
            Ok(Settlement::linked(user, reclaim))
        }
        Resolution::Provision => {
            let user = create(state, provider_id, identity, configured.vouches_for_email()).await?;
            Ok(Settlement::provisioned(user))
        }
    }
}

async fn link(
    state: &AppState,
    provider_id: Uuid,
    identity: &Identity,
    user: Uuid,
    reclaim: bool,
) -> Result<(), SignInFailure> {
    let password_hash = if reclaim {
        Some(unusable().await?)
    } else {
        None
    };
    let mut transaction = state.db().begin().await?;
    if let Some(password_hash) = &password_hash
        && !users::reclaim(&mut transaction, user, password_hash).await?
    {
        return Err(SignInFailure::Stale);
    }
    identities::link(
        &mut *transaction,
        provider_id,
        user,
        &identity.subject,
        &identity.email,
    )
    .await?;
    if reclaim {
        refresh_tokens::revoke_all_user_tokens(&mut *transaction, user).await?;
        sessions::revoke_all_user_sessions(&mut *transaction, user).await?;
    }
    transaction.commit().await?;
    Ok(())
}

async fn create(
    state: &AppState,
    provider_id: Uuid,
    identity: &Identity,
    email_verified: bool,
) -> Result<Uuid, SignInFailure> {
    members::admit(state, None).await?;
    let password_hash = unusable().await?;
    let mut transaction = state.db().begin().await?;
    let user = provision::insert(
        &mut transaction,
        &identity.email,
        &password_hash,
        identity.name.as_deref(),
        false,
        email_verified,
    )
    .await?;
    identities::link(
        &mut *transaction,
        provider_id,
        user.id,
        &identity.subject,
        &identity.email,
    )
    .await?;
    transaction.commit().await?;
    provision::furnish(state.db(), &user).await;
    record(
        state,
        user.id,
        &user.email,
        actions::USER_CREATED,
        json!({ PROVIDER_ID: provider_id }),
    )
    .await;
    Ok(user.id)
}

async fn unusable() -> Result<String, SignInFailure> {
    let hashed = tokio::task::spawn_blocking(|| hash_password(&generate_token()))
        .await
        .map_err(|_| PasswordError::HashingFailed)?;
    Ok(hashed?)
}

async fn record(state: &AppState, user: Uuid, email: &str, action: &str, new_values: Value) {
    audit(
        state.db(),
        AuditEvent {
            organization_id: None,
            workspace_id: None,
            actor_id: user,
            actor_email: email,
            action,
            resource_type: resources::USER,
            resource_id: Some(user),
            old_values: None,
            new_values: Some(new_values),
        },
    )
    .await;
}
