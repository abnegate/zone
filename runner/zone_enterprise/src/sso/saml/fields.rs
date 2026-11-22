#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fields<'a> {
    pub display_name: &'a str,
    pub entity_id: &'a str,
    pub sso_url: &'a str,
    pub certificates: &'a [String],
    pub allowed_domains: &'a [String],
    pub require_verified_email: bool,
}
