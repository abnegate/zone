use url::Url;

use crate::sso::error::Error;
use crate::sso::issuer::endpoint_allowed;
use crate::sso::saml::certificate::Certificate;
use crate::sso::saml::document::Document;
use crate::sso::saml::entity_id::EntityId;
use crate::sso::saml::node_id::NodeId;
use crate::sso::saml::uri;

const ENTITY_DESCRIPTOR: &str = "EntityDescriptor";
const ENTITIES_DESCRIPTOR: &str = "EntitiesDescriptor";
const IDP_DESCRIPTOR: &str = "IDPSSODescriptor";
const KEY_DESCRIPTOR: &str = "KeyDescriptor";
const KEY_INFO: &str = "KeyInfo";
const X509_DATA: &str = "X509Data";
const X509_CERTIFICATE: &str = "X509Certificate";
const SINGLE_SIGN_ON_SERVICE: &str = "SingleSignOnService";
const ENTITY_ID: &str = "entityID";
const PROTOCOL_SUPPORT: &str = "protocolSupportEnumeration";
const USE: &str = "use";
const SIGNING: &str = "signing";
const BINDING: &str = "Binding";
const LOCATION: &str = "Location";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub entity_id: EntityId,
    pub sso_url: Url,
    pub certificates: Vec<Certificate>,
}

impl Metadata {
    pub fn parse(input: &[u8]) -> Result<Self, Error> {
        let document = Document::parse(input)
            .map_err(|malformed| invalid(&format!("It was refused because {malformed}.")))?;
        let root = document.root();
        if document.is(root, uri::METADATA, ENTITIES_DESCRIPTOR) {
            return Err(invalid(
                "It describes several entities. Use the metadata of a single identity provider.",
            ));
        }
        let Some(entity) = document
            .element(root)
            .filter(|element| element.is(uri::METADATA, ENTITY_DESCRIPTOR))
        else {
            return Err(invalid("Its root element isn't an md:EntityDescriptor."));
        };
        let entity_id = entity
            .attribute(ENTITY_ID)
            .ok_or_else(|| invalid("It has no entityID."))
            .and_then(EntityId::parse)?;
        let descriptor = descriptor(&document, root)?;
        Ok(Self {
            entity_id,
            sso_url: redirect_location(&document, descriptor)?,
            certificates: certificates(&document, descriptor)?,
        })
    }
}

pub(super) fn sso_url(location: &str) -> Result<Url, Error> {
    let refused = || {
        invalid(
            "The single sign-on URL must be an absolute https URL; plain http is only allowed for localhost.",
        )
    };
    let url = Url::parse(location.trim()).map_err(|_| refused())?;
    if !endpoint_allowed(&url) {
        return Err(refused());
    }
    Ok(url)
}

fn descriptor(document: &Document, root: NodeId) -> Result<NodeId, Error> {
    let mut supported = document
        .children_named(root, uri::METADATA, IDP_DESCRIPTOR)
        .filter(|descriptor| {
            document
                .element(*descriptor)
                .and_then(|element| element.attribute(PROTOCOL_SUPPORT))
                .is_some_and(|protocols| {
                    protocols
                        .split_whitespace()
                        .any(|protocol| protocol == uri::PROTOCOL)
                })
        });
    let descriptor = supported
        .next()
        .ok_or_else(|| invalid("It has no IDPSSODescriptor that supports SAML 2.0."))?;
    if supported.next().is_some() {
        return Err(invalid(
            "It has more than one IDPSSODescriptor for SAML 2.0.",
        ));
    }
    Ok(descriptor)
}

fn redirect_location(document: &Document, descriptor: NodeId) -> Result<Url, Error> {
    let service = document
        .children_named(descriptor, uri::METADATA, SINGLE_SIGN_ON_SERVICE)
        .filter_map(|service| document.element(service))
        .find(|service| service.attribute(BINDING) == Some(uri::HTTP_REDIRECT))
        .ok_or_else(|| invalid("It has no SingleSignOnService with the HTTP-Redirect binding."))?;
    let location = service
        .attribute(LOCATION)
        .ok_or_else(|| invalid("Its HTTP-Redirect SingleSignOnService has no Location."))?;
    sso_url(location)
}

fn certificates(document: &Document, descriptor: NodeId) -> Result<Vec<Certificate>, Error> {
    let mut certificates: Vec<Certificate> = Vec::new();
    for key in document.children_named(descriptor, uri::METADATA, KEY_DESCRIPTOR) {
        let signing = document
            .element(key)
            .and_then(|element| element.attribute(USE))
            .is_none_or(|purpose| purpose.trim() == SIGNING);
        if !signing {
            continue;
        }
        for info in document.children_named(key, uri::DSIG, KEY_INFO) {
            for data in document.children_named(info, uri::DSIG, X509_DATA) {
                for encoded in document.children_named(data, uri::DSIG, X509_CERTIFICATE) {
                    let text = document
                        .text(encoded)
                        .ok_or_else(|| invalid("A signing certificate isn't plain text."))?;
                    let certificate = Certificate::parse(&text)?;
                    if !certificates.contains(&certificate) {
                        certificates.push(certificate);
                    }
                }
            }
        }
    }
    if certificates.is_empty() {
        return Err(invalid("It has no signing certificate."));
    }
    Ok(certificates)
}

fn invalid(reason: &str) -> Error {
    Error::MetadataInvalid(reason.to_owned())
}
