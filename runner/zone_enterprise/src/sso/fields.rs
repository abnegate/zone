use crate::sso::protocol::Protocol;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fields<'a> {
    pub protocol: Protocol,
    pub display_name: &'a str,
    pub issuer: &'a str,
    pub client_id: &'a str,
    pub client_secret: &'a str,
    pub allowed_domains: &'a [String],
    pub require_verified_email: bool,
}
