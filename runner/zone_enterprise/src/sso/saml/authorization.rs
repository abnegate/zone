use std::fmt;
use std::io::Write;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, SecondsFormat, Utc};
use flate2::Compression;
use flate2::write::DeflateEncoder;
use url::Url;

use crate::sso::saml::canonical::{escape_attribute, escape_text};
use crate::sso::saml::service_provider::ServiceProvider;
use crate::sso::saml::uri;
use crate::sso::{REDACTED, random};

pub const SAML_REQUEST: &str = "SAMLRequest";
pub const RELAY_STATE: &str = "RelayState";

const ID_PREFIX: &str = "_";

#[derive(Clone, PartialEq, Eq)]
pub struct Authorization {
    pub url: Url,
    pub state: String,
    pub request_id: String,
}

impl Authorization {
    pub fn new(sso_url: &Url, service: &ServiceProvider, now: DateTime<Utc>) -> Self {
        let request_id = format!("{ID_PREFIX}{}", random::token());
        let state = random::token();
        let request = request(&request_id, sso_url, service, now);
        let mut url = sso_url.clone();
        url.query_pairs_mut()
            .append_pair(SAML_REQUEST, &STANDARD.encode(deflate(request.as_bytes())))
            .append_pair(RELAY_STATE, &state);
        Self {
            url,
            state,
            request_id,
        }
    }
}

impl fmt::Debug for Authorization {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut endpoint = self.url.clone();
        endpoint.set_query(None);
        formatter
            .debug_struct("Authorization")
            .field("endpoint", &endpoint.as_str())
            .field("state", &REDACTED)
            .field("request_id", &self.request_id)
            .finish()
    }
}

fn request(
    request_id: &str,
    sso_url: &Url,
    service: &ServiceProvider,
    now: DateTime<Utc>,
) -> String {
    let attribute = |value: &str| {
        let mut escaped = String::new();
        escape_attribute(value, &mut escaped);
        escaped
    };
    let mut issuer = String::new();
    escape_text(&service.entity_id, &mut issuer);
    format!(
        "<samlp:AuthnRequest xmlns:samlp=\"{protocol}\" xmlns:saml=\"{assertion}\" ID=\"{id}\" Version=\"2.0\" IssueInstant=\"{instant}\" Destination=\"{destination}\" AssertionConsumerServiceURL=\"{acs}\" ProtocolBinding=\"{binding}\">\
<saml:Issuer>{issuer}</saml:Issuer>\
<samlp:NameIDPolicy AllowCreate=\"true\"/>\
</samlp:AuthnRequest>",
        protocol = uri::PROTOCOL,
        assertion = uri::ASSERTION,
        id = attribute(request_id),
        instant = now.to_rfc3339_opts(SecondsFormat::Secs, true),
        destination = attribute(sso_url.as_str()),
        acs = attribute(&service.acs_url),
        binding = uri::HTTP_POST,
    )
}

fn deflate(input: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(input)
        .expect("deflating into memory never fails");
    encoder.finish().expect("deflating into memory never fails")
}
