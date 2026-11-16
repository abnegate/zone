use serde::Serialize;
use uuid::Uuid;

use crate::db::users::UserWithPermissions;
use crate::routes::common::Timestamps;

#[derive(Debug, Serialize)]
pub struct UserResponse {
    id: Uuid,
    email: String,
    display_name: Option<String>,
    is_admin: bool,
    is_active: bool,
    email_verified: bool,
    #[serde(flatten)]
    timestamps: Timestamps,
    last_login_at: Option<String>,
}

impl UserResponse {
    pub fn from_user(user: &UserWithPermissions) -> Self {
        Self {
            id: user.user.id,
            email: user.user.email.clone(),
            display_name: user.user.display_name.clone(),
            is_admin: user.user.is_admin.unwrap_or(false),
            is_active: user.user.is_active.unwrap_or(true),
            email_verified: user.user.email_verified,
            timestamps: Timestamps::from_naive(user.user.created_at, user.user.updated_at),
            last_login_at: user
                .user
                .last_login_at
                .map(|moment| moment.and_utc().to_rfc3339()),
        }
    }
}
