pub const DISABLED: &str = "account_disabled";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_password_login_refusal_shares_the_sign_in_refusal_code() {
        assert_eq!(DISABLED, "account_disabled");
        assert_eq!(DISABLED, zone_enterprise::sso::ACCOUNT_DISABLED);
    }
}
