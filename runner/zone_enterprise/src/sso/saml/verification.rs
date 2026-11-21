use aws_lc_rs::constant_time;
use aws_lc_rs::signature::UnparsedPublicKey;

use crate::sso::saml::canonical;
use crate::sso::saml::document::Document;
use crate::sso::saml::identifiers;
use crate::sso::saml::node_id::NodeId;
use crate::sso::saml::refusal::Refusal;
use crate::sso::saml::signature::{SIGNATURE, Signature};
use crate::sso::saml::uri;

const FRAGMENT: char = '#';

pub fn verify(document: &Document, element: NodeId, keys: &[&[u8]]) -> Result<bool, Refusal> {
    let mut signatures = document.children_named(element, uri::DSIG, SIGNATURE);
    let Some(signature) = signatures.next() else {
        return Ok(false);
    };
    if signatures.next().is_some() {
        return Err(Refusal::SignatureCount);
    }
    let signature = Signature::parse(document, signature)?;

    let identifier = document
        .element(element)
        .and_then(|parent| parent.attribute(identifiers::ATTRIBUTE))
        .filter(|identifier| !identifier.is_empty())
        .ok_or(Refusal::Reference)?;
    if signature.reference().strip_prefix(FRAGMENT) != Some(identifier) {
        return Err(Refusal::Reference);
    }
    if identifiers::index(document)?.get(identifier) != Some(&element) {
        return Err(Refusal::Reference);
    }

    let mut digest = signature.digest_method().context();
    canonical::stream(
        document,
        element,
        Some(signature.node()),
        signature.reference_prefixes(),
        &mut digest,
    )?;
    constant_time::verify_slices_are_equal(digest.finish().as_ref(), signature.digest())
        .map_err(|_| Refusal::Digest)?;

    let signed_info = canonical::exclusive(
        document,
        signature.signed_info(),
        None,
        signature.signed_info_prefixes(),
    )?;
    let parameters = signature.method().parameters();
    let verified = keys.iter().any(|key| {
        UnparsedPublicKey::new(parameters, key)
            .verify(&signed_info, signature.value())
            .is_ok()
    });
    if verified {
        Ok(true)
    } else {
        Err(Refusal::Signature)
    }
}
