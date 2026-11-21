#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    Oidc {
        client_id: String,
        sealed_secret: String,
    },
    Saml {
        sso_url: String,
        certificates: Vec<String>,
        metadata_url: Option<String>,
    },
}
