use url::Url;

use crate::sso::display_name::DisplayName;
use crate::sso::domains::Domains;
use crate::sso::error::Error;
use crate::sso::policy::Policy;
use crate::sso::saml::certificate::Certificate;
use crate::sso::saml::entity_id::EntityId;
use crate::sso::saml::fields::Fields;
use crate::sso::saml::metadata::{self, Metadata};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    pub display_name: DisplayName,
    pub entity_id: EntityId,
    pub sso_url: Url,
    pub certificates: Vec<Certificate>,
    pub policy: Policy,
}

impl Provider {
    pub fn parse(fields: &Fields<'_>) -> Result<Self, Error> {
        let display_name = DisplayName::parse(fields.display_name)?;
        let entity_id = EntityId::parse(fields.entity_id)?;
        let sso_url = metadata::sso_url(fields.sso_url)?;
        let certificates = fields
            .certificates
            .iter()
            .map(|certificate| Certificate::parse(certificate))
            .collect::<Result<Vec<_>, _>>()?;
        if certificates.is_empty() {
            return Err(Error::MetadataInvalid(
                "It has no signing certificate.".to_owned(),
            ));
        }
        Ok(Self {
            display_name,
            entity_id,
            sso_url,
            certificates,
            policy: policy(fields.allowed_domains, fields.require_verified_email)?,
        })
    }

    pub fn from_metadata(
        display_name: &str,
        metadata: &Metadata,
        allowed_domains: &[String],
        require_verified_email: bool,
    ) -> Result<Self, Error> {
        Ok(Self {
            display_name: DisplayName::parse(display_name)?,
            entity_id: metadata.entity_id.clone(),
            sso_url: metadata.sso_url.clone(),
            certificates: metadata.certificates.clone(),
            policy: policy(allowed_domains, require_verified_email)?,
        })
    }
}

fn policy(allowed_domains: &[String], require_verified_email: bool) -> Result<Policy, Error> {
    let domains = Domains::parse(allowed_domains)?;
    if domains.is_empty() {
        return Err(Error::DomainRequired);
    }
    if require_verified_email {
        return Err(Error::VerifiedEmailUnsupported);
    }
    Ok(Policy {
        domains,
        require_verified_email,
    })
}
