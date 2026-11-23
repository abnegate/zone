use uuid::Uuid;

pub const ACCOUNT_DISABLED: &str = "account_disabled";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Account {
    pub id: Uuid,
    pub active: bool,
    pub email_verified: bool,
    pub admin: bool,
    pub pristine: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_code_matches_the_password_login_refusal() {
        assert_eq!(ACCOUNT_DISABLED, "account_disabled");
    }
}
