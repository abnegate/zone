use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::sso::saml::digest_method::DigestMethod;
use crate::sso::saml::document::Document;
use crate::sso::saml::node_id::NodeId;
use crate::sso::saml::refusal::Refusal;
use crate::sso::saml::signature_method::SignatureMethod;
use crate::sso::saml::uri;

pub const SIGNATURE: &str = "Signature";
pub const SIGNED_INFO: &str = "SignedInfo";
pub const CANONICALIZATION_METHOD: &str = "CanonicalizationMethod";
pub const SIGNATURE_METHOD: &str = "SignatureMethod";
pub const REFERENCE: &str = "Reference";
pub const TRANSFORMS: &str = "Transforms";
pub const TRANSFORM: &str = "Transform";
pub const DIGEST_METHOD: &str = "DigestMethod";
pub const DIGEST_VALUE: &str = "DigestValue";
pub const SIGNATURE_VALUE: &str = "SignatureValue";
pub const KEY_INFO: &str = "KeyInfo";
pub const INCLUSIVE_NAMESPACES: &str = "InclusiveNamespaces";
pub const ALGORITHM: &str = "Algorithm";
pub const URI: &str = "URI";
pub const PREFIX_LIST: &str = "PrefixList";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    node: NodeId,
    signed_info: NodeId,
    signed_info_prefixes: Vec<String>,
    method: SignatureMethod,
    reference: String,
    reference_prefixes: Vec<String>,
    digest_method: DigestMethod,
    digest: Vec<u8>,
    value: Vec<u8>,
}

impl Signature {
    pub fn parse(document: &Document, signature: NodeId) -> Result<Self, Refusal> {
        if !document.is(signature, uri::DSIG, SIGNATURE) {
            return Err(Refusal::Signature);
        }
        let mut children = document.child_elements(signature);
        let signed_info = children
            .next()
            .filter(|id| document.is(*id, uri::DSIG, SIGNED_INFO))
            .ok_or(Refusal::Signature)?;
        let value = children
            .next()
            .filter(|id| document.is(*id, uri::DSIG, SIGNATURE_VALUE))
            .ok_or(Refusal::Signature)?;
        if children
            .next()
            .is_some_and(|id| !document.is(id, uri::DSIG, KEY_INFO))
            || children.next().is_some()
        {
            return Err(Refusal::Signature);
        }
        let value = decoded(document, value).ok_or(Refusal::Signature)?;

        let mut children = document.child_elements(signed_info);
        let canonicalization = children
            .next()
            .filter(|id| document.is(*id, uri::DSIG, CANONICALIZATION_METHOD))
            .ok_or(Refusal::Transform)?;
        let method = children
            .next()
            .filter(|id| document.is(*id, uri::DSIG, SIGNATURE_METHOD))
            .ok_or(Refusal::Algorithm)?;
        let reference = children
            .next()
            .filter(|id| document.is(*id, uri::DSIG, REFERENCE))
            .ok_or(Refusal::Reference)?;
        if children.next().is_some() {
            return Err(Refusal::Reference);
        }
        let signed_info_prefixes = exclusive(document, canonicalization)?;
        let method = SignatureMethod::parse(algorithm(document, method)?)?;

        let target = document
            .element(reference)
            .and_then(|element| element.attribute(URI))
            .ok_or(Refusal::Reference)?
            .to_owned();
        let mut children = document.child_elements(reference);
        let transforms = children
            .next()
            .filter(|id| document.is(*id, uri::DSIG, TRANSFORMS))
            .ok_or(Refusal::Transform)?;
        let digest_method = children
            .next()
            .filter(|id| document.is(*id, uri::DSIG, DIGEST_METHOD))
            .ok_or(Refusal::Algorithm)?;
        let digest = children
            .next()
            .filter(|id| document.is(*id, uri::DSIG, DIGEST_VALUE))
            .ok_or(Refusal::Digest)?;
        if children.next().is_some() {
            return Err(Refusal::Reference);
        }
        let reference_prefixes = self::transforms(document, transforms)?;
        let digest_method = DigestMethod::parse(algorithm(document, digest_method)?)?;
        let digest = decoded(document, digest).ok_or(Refusal::Digest)?;

        Ok(Self {
            node: signature,
            signed_info,
            signed_info_prefixes,
            method,
            reference: target,
            reference_prefixes,
            digest_method,
            digest,
            value,
        })
    }

    pub fn node(&self) -> NodeId {
        self.node
    }

    pub fn signed_info(&self) -> NodeId {
        self.signed_info
    }

    pub fn signed_info_prefixes(&self) -> &[String] {
        &self.signed_info_prefixes
    }

    pub fn method(&self) -> SignatureMethod {
        self.method
    }

    pub fn reference(&self) -> &str {
        &self.reference
    }

    pub fn reference_prefixes(&self) -> &[String] {
        &self.reference_prefixes
    }

    pub fn digest_method(&self) -> DigestMethod {
        self.digest_method
    }

    pub fn digest(&self) -> &[u8] {
        &self.digest
    }

    pub fn value(&self) -> &[u8] {
        &self.value
    }
}

fn transforms(document: &Document, transforms: NodeId) -> Result<Vec<String>, Refusal> {
    let mut children = document.child_elements(transforms);
    let enveloped = children
        .next()
        .filter(|id| document.is(*id, uri::DSIG, TRANSFORM))
        .ok_or(Refusal::Transform)?;
    let canonicalization = children
        .next()
        .filter(|id| document.is(*id, uri::DSIG, TRANSFORM))
        .ok_or(Refusal::Transform)?;
    if children.next().is_some() {
        return Err(Refusal::Transform);
    }
    let algorithm = document
        .element(enveloped)
        .and_then(|element| element.attribute(ALGORITHM));
    if algorithm != Some(uri::ENVELOPED_SIGNATURE)
        || document.child_elements(enveloped).next().is_some()
    {
        return Err(Refusal::Transform);
    }
    exclusive(document, canonicalization)
}

fn exclusive(document: &Document, method: NodeId) -> Result<Vec<String>, Refusal> {
    let algorithm = document
        .element(method)
        .and_then(|element| element.attribute(ALGORITHM));
    if algorithm != Some(uri::EXCLUSIVE_CANONICALIZATION) {
        return Err(Refusal::Transform);
    }
    let mut children = document.child_elements(method);
    let Some(inclusive) = children.next() else {
        return Ok(Vec::new());
    };
    if children.next().is_some()
        || !document.is(
            inclusive,
            uri::EXCLUSIVE_CANONICALIZATION,
            INCLUSIVE_NAMESPACES,
        )
    {
        return Err(Refusal::Transform);
    }
    let prefixes = document
        .element(inclusive)
        .and_then(|element| element.attribute(PREFIX_LIST))
        .ok_or(Refusal::Transform)?;
    Ok(prefixes
        .split(is_whitespace)
        .filter(|prefix| !prefix.is_empty())
        .map(str::to_owned)
        .collect())
}

fn algorithm(document: &Document, method: NodeId) -> Result<&str, Refusal> {
    if document.child_elements(method).next().is_some() {
        return Err(Refusal::Algorithm);
    }
    document
        .element(method)
        .and_then(|element| element.attribute(ALGORITHM))
        .ok_or(Refusal::Algorithm)
}

fn decoded(document: &Document, element: NodeId) -> Option<Vec<u8>> {
    let text = document.text(element)?;
    let compact: String = text.chars().filter(|c| !is_whitespace(*c)).collect();
    STANDARD
        .decode(compact)
        .ok()
        .filter(|bytes| !bytes.is_empty())
}

fn is_whitespace(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r')
}
