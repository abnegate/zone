//! Requiring single sign-on: the enforcement switch and the password paths it closes for everyone
//! but server admins.

use axum::http::StatusCode;
use serde_json::json;
use uuid::Uuid;
use zone_enterprise::LICENSE_REQUIRED;
use zone_enterprise::sso::{Error, PROVIDER_REQUIRED, SSO_REQUIRED};
use zone_server::db::audit::{actions, resources};
use zone_server::db::password_reset;

use crate::common::identity_provider::IdentityProvider;
use crate::common::sso::{Account, Fixture};
use crate::common::{TestResponse, test_email, test_password};

const PREFIX: &str = "sso_enforcement";
const NEW_PASSWORD: &str = "Brand-new-passw0rd";
const RESET_REQUESTED: &str = "If the email exists, a password reset email has been sent";

struct Server {
    fixture: Fixture,
    admin: Account,
    member: Account,
    provider: Uuid,
    _identity_provider: IdentityProvider,
}

impl Server {
    /// An Enterprise server with an admin, a member and one enabled provider; enforcement is off.
    async fn new() -> Self {
        let fixture = Fixture::new(PREFIX).await;
        let admin = fixture.register().await;
        let member = fixture.register().await;
        fixture.license(&admin).await;
        let identity_provider = IdentityProvider::start().await;
        let provider = fixture
            .created(&admin.token, &identity_provider.provider())
            .await;
        Self {
            fixture,
            admin,
            member,
            provider,
            _identity_provider: identity_provider,
        }
    }

    async fn enforced() -> Self {
        let server = Self::new().await;
        server.enforce(true).await.assert_status(StatusCode::OK);
        server
    }

    async fn enforce(&self, required: bool) -> TestResponse {
        self.fixture.enforce(&self.admin.token, required).await
    }

    async fn login(&self, account: &Account) -> TestResponse {
        self.fixture.login(&account.email, &test_password()).await
    }

    async fn refresh(&self, refresh_token: &str) -> TestResponse {
        self.fixture
            .client
            .post_json(
                "/api/auth/refresh",
                &json!({ "refresh_token": refresh_token }),
            )
            .await
    }

    async fn forgot(&self, account: &Account) -> TestResponse {
        self.fixture
            .client
            .post_json(
                "/api/auth/forgot-password",
                &json!({ "email": account.email }),
            )
            .await
    }

    async fn reset(&self, token: &str) -> TestResponse {
        self.fixture
            .client
            .post_json(
                "/api/auth/reset-password",
                &json!({ "token": token, "new_password": NEW_PASSWORD }),
            )
            .await
    }

    async fn reset_tokens(&self, account: &Account) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM password_reset_tokens WHERE user_id = $1")
            .bind(account.id)
            .fetch_one(self.fixture.pool())
            .await
            .unwrap()
    }

    async fn unused_reset_tokens(&self, account: &Account) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM password_reset_tokens WHERE user_id = $1 AND used_at IS NULL",
        )
        .bind(account.id)
        .fetch_one(self.fixture.pool())
        .await
        .unwrap()
    }

    async fn cleanup(self) {
        self.fixture.cleanup().await;
    }
}

fn assert_sso_required(response: &TestResponse) {
    response.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(
        response.json_value(),
        json!({ "error": Error::Required.to_string(), "code": SSO_REQUIRED })
    );
}

fn assert_provider_required(response: &TestResponse) {
    response.assert_status(StatusCode::CONFLICT);
    assert_eq!(response.json_value()["code"], PROVIDER_REQUIRED);
}

#[tokio::test]
async fn enforcement_needs_an_enabled_provider() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = fixture.register().await;
    fixture.license(&admin).await;

    assert_provider_required(&fixture.enforce(&admin.token, true).await);

    let identity_provider = IdentityProvider::start().await;
    let mut disabled = identity_provider.provider();
    disabled["enabled"] = json!(false);
    fixture.created(&admin.token, &disabled).await;
    assert_provider_required(&fixture.enforce(&admin.token, true).await);
    assert_eq!(
        fixture.settings(&admin.token).await.json_value()["required"],
        false
    );
    assert!(
        fixture
            .audits(actions::SSO_ENFORCEMENT_UPDATED)
            .await
            .is_empty()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn turning_enforcement_on_and_off_is_answered_shown_and_audited() {
    let server = Server::new().await;

    let on = server.enforce(true).await;
    on.assert_status(StatusCode::OK);
    assert_eq!(on.json_value(), json!({ "required": true }));
    assert_eq!(
        server
            .fixture
            .settings(&server.admin.token)
            .await
            .json_value()["required"],
        true
    );
    server.enforce(true).await.assert_status(StatusCode::OK);
    let off = server.enforce(false).await;
    off.assert_status(StatusCode::OK);
    assert_eq!(off.json_value(), json!({ "required": false }));

    let audits = server
        .fixture
        .audits(actions::SSO_ENFORCEMENT_UPDATED)
        .await;
    assert_eq!(
        audits.len(),
        2,
        "setting the same value again is not a change"
    );
    assert_eq!(audits[0].old_values, Some(json!({ "required": false })));
    assert_eq!(audits[0].new_values, Some(json!({ "required": true })));
    assert_eq!(audits[1].old_values, Some(json!({ "required": true })));
    assert_eq!(audits[1].new_values, Some(json!({ "required": false })));
    for audit in &audits {
        assert_eq!(audit.actor_id, Some(server.admin.id));
        assert_eq!(audit.resource_type, resources::SSO_SETTINGS);
        assert_eq!(audit.organization_id, None);
    }
    server.cleanup().await;
}

#[tokio::test]
async fn the_last_enabled_provider_stays_while_enforcement_is_on() {
    let server = Server::enforced().await;
    let token = &server.admin.token;

    assert_provider_required(&server.fixture.delete(token, server.provider).await);
    assert_provider_required(
        &server
            .fixture
            .update(token, server.provider, &json!({ "enabled": false }))
            .await,
    );

    let second_provider = IdentityProvider::start().await;
    let second = server
        .fixture
        .created(token, &second_provider.provider())
        .await;
    let disabled_provider = IdentityProvider::start().await;
    let mut disabled_body = disabled_provider.provider();
    disabled_body["enabled"] = json!(false);
    let disabled = server.fixture.created(token, &disabled_body).await;

    server
        .fixture
        .delete(token, disabled)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    server
        .fixture
        .update(token, server.provider, &json!({ "enabled": false }))
        .await
        .assert_status(StatusCode::OK);
    assert_provider_required(&server.fixture.delete(token, second).await);
    assert_provider_required(
        &server
            .fixture
            .update(token, second, &json!({ "enabled": false }))
            .await,
    );
    server
        .fixture
        .delete(token, server.provider)
        .await
        .assert_status(StatusCode::NO_CONTENT);

    server.enforce(false).await.assert_status(StatusCode::OK);
    server
        .fixture
        .delete(token, second)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(server.fixture.providers().await, 0);
    server.cleanup().await;
}

#[tokio::test]
async fn a_member_password_login_is_refused_with_the_terminal_friendly_message() {
    let server = Server::enforced().await;

    let response = server.login(&server.member).await;

    assert_sso_required(&response);
    let message = response.json_value()["error"].as_str().unwrap().to_owned();
    assert_eq!(
        message,
        "This server requires single sign-on. Sign in from the Zone console in your browser; only server admins can sign in with a password."
    );
    assert!(!message.contains('\n') && !message.contains('<') && !message.contains('>'));
    server.cleanup().await;
}

#[tokio::test]
async fn a_member_is_refused_the_same_way_whatever_the_password() {
    let server = Server::enforced().await;

    let right = server.login(&server.member).await;
    let wrong = server
        .fixture
        .login(&server.member.email, "Wrong-passw0rd")
        .await;
    let unknown = server.fixture.login(&test_email(), "Wrong-passw0rd").await;

    for response in [&right, &wrong, &unknown] {
        assert_sso_required(response);
    }
    assert_eq!(right.json_value(), wrong.json_value());
    server.cleanup().await;
}

#[tokio::test]
async fn a_wrong_admin_password_is_still_wrong_under_enforcement() {
    let server = Server::enforced().await;

    let response = server
        .fixture
        .login(&server.admin.email, "Wrong-passw0rd")
        .await;

    response.assert_status(StatusCode::UNAUTHORIZED);
    assert!(response.json_value().get("code").is_none());
    server.cleanup().await;
}

#[tokio::test]
async fn a_member_password_session_ends_at_its_next_refresh_once_sso_is_required() {
    let server = Server::new().await;
    let member = server.login(&server.member).await.json_value();
    let admin = server.login(&server.admin).await.json_value();
    let member_access = member["access_token"].as_str().unwrap();
    let member_refresh = member["refresh_token"].as_str().unwrap();

    server.enforce(true).await.assert_status(StatusCode::OK);

    assert_sso_required(&server.refresh(member_refresh).await);
    server
        .fixture
        .client
        .get_auth("/api/auth/sessions", member_access)
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    server
        .refresh(member_refresh)
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    server
        .refresh(admin["refresh_token"].as_str().unwrap())
        .await
        .assert_status(StatusCode::OK);
    server.cleanup().await;
}

#[tokio::test]
async fn a_member_password_session_refreshes_while_sso_is_not_required() {
    let server = Server::new().await;
    let member = server.login(&server.member).await.json_value();

    server
        .refresh(member["refresh_token"].as_str().unwrap())
        .await
        .assert_status(StatusCode::OK);
    server.cleanup().await;
}

#[tokio::test]
async fn an_admin_password_login_breaks_the_glass_and_is_audited() {
    let server = Server::enforced().await;

    let response = server.login(&server.admin).await;

    response.assert_status(StatusCode::OK);
    let token = response.json_value()["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    server
        .fixture
        .settings(&token)
        .await
        .assert_status(StatusCode::OK);
    let audits = server
        .fixture
        .audits(actions::USER_BREAK_GLASS_SIGNED_IN)
        .await;
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].actor_id, Some(server.admin.id));
    assert_eq!(audits[0].resource_type, resources::USER);
    assert_eq!(audits[0].resource_id, Some(server.admin.id));
    assert_eq!(audits[0].organization_id, None);
    assert_eq!(audits[0].old_values, None);
    assert_eq!(audits[0].new_values, None);
    server.cleanup().await;
}

#[tokio::test]
async fn a_disabled_member_is_told_only_that_sso_is_required() {
    let server = Server::enforced().await;
    sqlx::query("UPDATE users SET is_active = false WHERE id = $1")
        .bind(server.member.id)
        .execute(server.fixture.pool())
        .await
        .unwrap();

    assert_sso_required(&server.login(&server.member).await);
    server.enforce(false).await.assert_status(StatusCode::OK);
    let response = server.login(&server.member).await;
    response.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(response.json_value()["code"], "account_disabled");
    server.cleanup().await;
}

#[tokio::test]
async fn registration_is_refused_for_everyone() {
    let server = Server::enforced().await;

    let response = server
        .fixture
        .register_as(&crate::common::test_email())
        .await;

    assert_sso_required(&response);
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(server.fixture.pool())
        .await
        .unwrap();
    assert_eq!(users, 2);
    server.cleanup().await;
}

#[tokio::test]
async fn forgot_password_issues_no_reset_to_a_member_but_does_to_an_admin() {
    let server = Server::enforced().await;

    let member = server.forgot(&server.member).await;
    let admin = server.forgot(&server.admin).await;

    for response in [&member, &admin] {
        response.assert_status(StatusCode::OK);
        assert_eq!(
            response.json_value(),
            json!({ "success": true, "message": RESET_REQUESTED })
        );
    }
    assert_eq!(server.reset_tokens(&server.member).await, 0);
    assert_eq!(server.reset_tokens(&server.admin).await, 1);
    assert!(server.fixture.mailed(&server.admin.email).await);
    assert_eq!(
        server.fixture.mail.recipients(),
        [server.admin.email.as_str()]
    );
    server.cleanup().await;
}

#[tokio::test]
async fn a_member_reset_is_refused_and_leaves_the_token_and_password_alone() {
    let server = Server::new().await;
    let (token, _) = password_reset::create_reset_token(server.fixture.pool(), server.member.id)
        .await
        .unwrap();
    server.enforce(true).await.assert_status(StatusCode::OK);

    assert_sso_required(&server.reset(&token).await);
    assert_eq!(server.unused_reset_tokens(&server.member).await, 1);

    server.enforce(false).await.assert_status(StatusCode::OK);
    server
        .login(&server.member)
        .await
        .assert_status(StatusCode::OK);
    server.reset(&token).await.assert_status(StatusCode::OK);
    server
        .fixture
        .login(&server.member.email, NEW_PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    server.cleanup().await;
}

#[tokio::test]
async fn an_admin_can_still_reset_their_password() {
    let server = Server::enforced().await;
    let (token, _) = password_reset::create_reset_token(server.fixture.pool(), server.admin.id)
        .await
        .unwrap();

    server.reset(&token).await.assert_status(StatusCode::OK);

    server
        .fixture
        .login(&server.admin.email, NEW_PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    server.cleanup().await;
}

#[tokio::test]
async fn an_invalid_reset_token_is_refused_as_before_under_enforcement() {
    let server = Server::enforced().await;

    let response = server.reset("not-a-token").await;

    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(
        response.json_value(),
        json!({ "error": "Invalid or expired reset token" })
    );
    server.cleanup().await;
}

#[tokio::test]
async fn without_enforcement_every_password_path_works_as_before() {
    let server = Server::new().await;

    server
        .login(&server.member)
        .await
        .assert_status(StatusCode::OK);
    server
        .login(&server.admin)
        .await
        .assert_status(StatusCode::OK);
    assert!(
        server
            .fixture
            .audits(actions::USER_BREAK_GLASS_SIGNED_IN)
            .await
            .is_empty()
    );
    server
        .fixture
        .register_as(&crate::common::test_email())
        .await
        .assert_status(StatusCode::CREATED);
    server
        .forgot(&server.member)
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(server.reset_tokens(&server.member).await, 1);
    assert!(server.fixture.mailed(&server.member.email).await);
    let (token, _) = password_reset::create_reset_token(server.fixture.pool(), server.member.id)
        .await
        .unwrap();
    server.reset(&token).await.assert_status(StatusCode::OK);
    server.cleanup().await;
}

#[tokio::test]
async fn enforcement_outlives_the_license_and_can_still_be_turned_off() {
    let server = Server::enforced().await;

    server.fixture.remove_license(&server.admin).await;

    assert_sso_required(&server.login(&server.member).await);
    server
        .login(&server.admin)
        .await
        .assert_status(StatusCode::OK);
    let body = server
        .fixture
        .settings(&server.admin.token)
        .await
        .json_value();
    assert_eq!(body["required"], true, "{body}");
    assert_eq!(body["entitled"], false, "{body}");

    let off = server.enforce(false).await;
    off.assert_status(StatusCode::OK);
    assert_eq!(off.json_value(), json!({ "required": false }));
    server
        .login(&server.member)
        .await
        .assert_status(StatusCode::OK);
    let on = server.enforce(true).await;
    on.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(on.json_value()["code"], LICENSE_REQUIRED);
    server.cleanup().await;
}

#[tokio::test]
async fn enforcement_outlives_a_lapsed_license() {
    let server = Server::enforced().await;

    server
        .fixture
        .store_license(&crate::common::sso::lapsed())
        .await;

    assert_sso_required(&server.login(&server.member).await);
    assert_provider_required(
        &server
            .fixture
            .delete(&server.admin.token, server.provider)
            .await,
    );
    server.enforce(false).await.assert_status(StatusCode::OK);
    server
        .fixture
        .update(
            &server.admin.token,
            server.provider,
            &json!({ "enabled": false }),
        )
        .await
        .assert_status(StatusCode::OK);
    server
        .fixture
        .delete(&server.admin.token, server.provider)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(server.fixture.providers().await, 0);
    server.cleanup().await;
}
