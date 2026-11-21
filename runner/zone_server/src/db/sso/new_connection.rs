use abnegate_secret::SecretValue;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewConnection {
    Oidc {
        client_id: String,
        client_secret: SecretValue,
    },
    Saml {
        sso_url: String,
        certificates: Vec<String>,
        metadata_url: Option<String>,
    },
}
