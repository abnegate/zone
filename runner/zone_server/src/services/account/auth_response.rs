use serde::Serialize;

use super::tokens::Tokens;
use super::user_response::UserResponse;
use crate::db::users::UserWithPermissions;

pub const TOKEN_TYPE: &str = "Bearer";

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: u64,
    pub user: UserResponse,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

impl AuthResponse {
    pub fn new(user: &UserWithPermissions, tokens: Tokens, expires_in: u64) -> Self {
        Self {
            access_token: tokens.access,
            refresh_token: tokens.refresh,
            token_type: TOKEN_TYPE,
            expires_in,
            user: UserResponse::from_user(user),
            roles: user.roles.clone(),
            permissions: user.permissions.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::db::users::UserRow;

    #[test]
    fn serializes_the_shape_the_console_reads() {
        let moment = NaiveDate::from_ymd_opt(2026, 11, 16)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap();
        let id = Uuid::from_u128(7);
        let user = UserWithPermissions {
            user: UserRow {
                id,
                email: "ada@acme.com".into(),
                display_name: Some("Ada".into()),
                is_active: None,
                is_admin: Some(true),
                email_verified: true,
                created_at: Some(moment),
                updated_at: Some(moment),
                last_login_at: Some(moment),
            },
            roles: vec!["admin".into()],
            permissions: vec!["users:read".into()],
        };
        let tokens = Tokens {
            access: "access".into(),
            refresh: "refresh".into(),
        };

        let body = serde_json::to_value(AuthResponse::new(&user, tokens, 900)).unwrap();

        assert_eq!(
            body,
            json!({
                "access_token": "access",
                "refresh_token": "refresh",
                "token_type": "Bearer",
                "expires_in": 900,
                "user": {
                    "id": id,
                    "email": "ada@acme.com",
                    "display_name": "Ada",
                    "is_admin": true,
                    "is_active": true,
                    "email_verified": true,
                    "created_at": "2026-11-16T09:30:00+00:00",
                    "updated_at": "2026-11-16T09:30:00+00:00",
                    "deleted_at": null,
                    "last_login_at": "2026-11-16T09:30:00+00:00",
                },
                "roles": ["admin"],
                "permissions": ["users:read"],
            })
        );
    }
}
