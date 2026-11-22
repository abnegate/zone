use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::sso::saml::document::Document;
use crate::sso::saml::node_id::NodeId;
use crate::sso::saml::refusal::Refusal;

pub const ATTRIBUTE: &str = "ID";

pub fn index(document: &Document) -> Result<HashMap<String, NodeId>, Refusal> {
    let mut identifiers = HashMap::new();
    for id in document.descendants(document.root()) {
        let Some(identifier) = document
            .element(id)
            .and_then(|element| element.attribute(ATTRIBUTE))
        else {
            continue;
        };
        match identifiers.entry(identifier.to_owned()) {
            Entry::Occupied(_) => return Err(Refusal::DuplicateId),
            Entry::Vacant(entry) => {
                entry.insert(id);
            }
        }
    }
    Ok(identifiers)
}
