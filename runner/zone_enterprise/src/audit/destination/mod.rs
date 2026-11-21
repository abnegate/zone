mod s3;
mod syslog;
mod webhook;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use url::Url;

use crate::audit::error::Error;
use crate::audit::kind::Kind;
use crate::audit::url_reason::UrlReason;

pub use s3::{Addressing, S3Secrets, S3Settings};
pub use syslog::{Facility, Protocol, SyslogSecrets, SyslogSettings};
pub use webhook::{WebhookSecrets, WebhookSettings};

pub(crate) const REDACTED: &str = "<redacted>";
const SCHEMES: [&str; 2] = ["http", "https"];

pub fn name(value: &str) -> Result<String, Error> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return Err(Error::NameInvalid);
    }
    Ok(trimmed.to_owned())
}

fn url(raw: &str) -> Result<String, Error> {
    let trimmed = raw.trim();
    let url = Url::parse(trimmed).map_err(|_| Error::UrlInvalid(UrlReason::Unparseable))?;
    let reason = if !SCHEMES.contains(&url.scheme()) {
        Some(UrlReason::Scheme)
    } else if url.host_str().is_none_or(str::is_empty) {
        Some(UrlReason::Host)
    } else if !url.username().is_empty() || url.password().is_some() {
        Some(UrlReason::Userinfo)
    } else if url.query().is_some() {
        Some(UrlReason::Query)
    } else if url.fragment().is_some() {
        Some(UrlReason::Fragment)
    } else {
        None
    };
    match reason {
        Some(reason) => Err(Error::UrlInvalid(reason)),
        None => Ok(trimmed.to_owned()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Settings {
    Webhook(WebhookSettings),
    S3(S3Settings),
    Syslog(SyslogSettings),
}

impl Settings {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Webhook(_) => Kind::Webhook,
            Self::S3(_) => Kind::S3,
            Self::Syslog(_) => Kind::Syslog,
        }
    }

    pub fn validated(self) -> Result<Self, Error> {
        Ok(match self {
            Self::Webhook(settings) => Self::Webhook(settings.validated()?),
            Self::S3(settings) => Self::S3(settings.validated()?),
            Self::Syslog(settings) => Self::Syslog(settings.validated()?),
        })
    }

    pub fn admits(&self, secrets: &Secrets) -> Result<(), Error> {
        match (self, secrets) {
            (Self::Syslog(settings), Secrets::Syslog(_))
                if settings.protocol == Protocol::Udp && !secrets.names().is_empty() =>
            {
                Err(Error::CertificateWithoutTls)
            }
            (Self::Webhook(_), Secrets::Webhook(_))
            | (Self::S3(_), Secrets::S3(_))
            | (Self::Syslog(_), Secrets::Syslog(_)) => Ok(()),
            _ => Err(Error::KindInvalid),
        }
    }

    pub fn from_stored(kind: Kind, value: Value) -> serde_json::Result<Self> {
        let settings = match kind {
            Kind::Webhook => Self::Webhook(serde_json::from_value(value)?),
            Kind::S3 => Self::S3(serde_json::from_value(value)?),
            Kind::Syslog => Self::Syslog(serde_json::from_value(value)?),
        };
        settings.validated().map_err(serde::de::Error::custom)
    }

    pub fn stored(&self) -> Value {
        match self {
            Self::Webhook(settings) => serde_json::to_value(settings),
            Self::S3(settings) => serde_json::to_value(settings),
            Self::Syslog(settings) => serde_json::to_value(settings),
        }
        .expect("settings hold only strings, numbers and enums")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretName {
    SigningSecret,
    AccessKeyId,
    SecretAccessKey,
    CaCertificate,
    ClientCertificate,
    ClientKey,
}

impl SecretName {
    pub const ALL: [Self; 6] = [
        Self::SigningSecret,
        Self::AccessKeyId,
        Self::SecretAccessKey,
        Self::CaCertificate,
        Self::ClientCertificate,
        Self::ClientKey,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SigningSecret => "signing_secret",
            Self::AccessKeyId => "access_key_id",
            Self::SecretAccessKey => "secret_access_key",
            Self::CaCertificate => "ca_certificate",
            Self::ClientCertificate => "client_certificate",
            Self::ClientKey => "client_key",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SigningSecret => "signing secret",
            Self::AccessKeyId => "access key ID",
            Self::SecretAccessKey => "secret access key",
            Self::CaCertificate => "CA certificate",
            Self::ClientCertificate => "client certificate",
            Self::ClientKey => "client key",
        }
    }

    pub fn kind(self) -> Kind {
        match self {
            Self::SigningSecret => Kind::Webhook,
            Self::AccessKeyId | Self::SecretAccessKey => Kind::S3,
            Self::CaCertificate | Self::ClientCertificate | Self::ClientKey => Kind::Syslog,
        }
    }

    pub fn required(self) -> bool {
        self.kind() != Kind::Syslog
    }

    pub fn of(kind: Kind) -> impl Iterator<Item = Self> {
        Self::ALL
            .into_iter()
            .filter(move |name| name.kind() == kind)
    }

    fn parse(kind: Kind, value: &str) -> Option<Self> {
        Self::of(kind).find(|name| name.as_str() == value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Secrets {
    Webhook(WebhookSecrets),
    S3(S3Secrets),
    Syslog(SyslogSecrets),
}

impl Secrets {
    pub fn parse(kind: Kind, value: Value) -> Result<Self, Error> {
        let object = match value {
            Value::Null => Map::new(),
            Value::Object(object) => object,
            _ => return Err(Error::SecretsMalformed),
        };
        let mut entries = Map::new();
        for (key, value) in object {
            let name = SecretName::parse(kind, &key).ok_or(Error::SecretUnknown(key))?;
            let Value::String(text) = value else {
                return Err(Error::SecretInvalid(name));
            };
            if !text.trim().is_empty() {
                entries.insert(name.as_str().to_owned(), Value::String(text));
            }
        }
        if let Some(missing) = SecretName::of(kind)
            .find(|name| name.required() && !entries.contains_key(name.as_str()))
        {
            return Err(Error::SecretInvalid(missing));
        }
        Self::from_entries(kind, entries)
            .map_err(|_| Error::SecretsMalformed)?
            .validated()
    }

    pub fn merged(&self, patch: Value) -> Result<Self, Error> {
        let patch = match patch {
            Value::Null => return Ok(self.clone()),
            Value::Object(patch) => patch,
            _ => return Err(Error::SecretsMalformed),
        };
        let kind = self.kind();
        let mut entries = self.entries();
        for (key, value) in patch {
            let name = SecretName::parse(kind, &key).ok_or(Error::SecretUnknown(key))?;
            match value {
                Value::String(text) if text.trim().is_empty() => {
                    entries.remove(name.as_str());
                }
                Value::String(text) => {
                    entries.insert(name.as_str().to_owned(), Value::String(text));
                }
                _ => return Err(Error::SecretInvalid(name)),
            }
        }
        Self::parse(kind, Value::Object(entries))
    }

    pub fn kind(&self) -> Kind {
        match self {
            Self::Webhook(_) => Kind::Webhook,
            Self::S3(_) => Kind::S3,
            Self::Syslog(_) => Kind::Syslog,
        }
    }

    pub fn names(&self) -> Vec<SecretName> {
        let entries = self.entries();
        SecretName::of(self.kind())
            .filter(|name| entries.contains_key(name.as_str()))
            .collect()
    }

    pub fn sealed_form(&self) -> String {
        Value::Object(self.entries()).to_string()
    }

    pub fn from_sealed_form(kind: Kind, sealed: &str) -> serde_json::Result<Self> {
        Self::from_entries(kind, serde_json::from_str(sealed)?)
    }

    fn from_entries(kind: Kind, entries: Map<String, Value>) -> serde_json::Result<Self> {
        let object = Value::Object(entries);
        Ok(match kind {
            Kind::Webhook => Self::Webhook(serde_json::from_value(object)?),
            Kind::S3 => Self::S3(serde_json::from_value(object)?),
            Kind::Syslog => Self::Syslog(serde_json::from_value(object)?),
        })
    }

    fn entries(&self) -> Map<String, Value> {
        let value = match self {
            Self::Webhook(secrets) => serde_json::to_value(secrets),
            Self::S3(secrets) => serde_json::to_value(secrets),
            Self::Syslog(secrets) => serde_json::to_value(secrets),
        }
        .expect("secrets hold only strings");
        let Value::Object(entries) = value else {
            unreachable!("secrets serialize to an object");
        };
        entries
    }

    fn validated(self) -> Result<Self, Error> {
        match self {
            Self::Syslog(secrets) => secrets.validated().map(Self::Syslog),
            other => Ok(other),
        }
    }
}

#[cfg(test)]
mod tests;
