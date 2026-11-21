use std::collections::{HashMap, HashSet};

use crate::sso::saml::attribute::Attribute;
use crate::sso::saml::document::Document;
use crate::sso::saml::element::Element;
use crate::sso::saml::node::Node;
use crate::sso::saml::node_id::NodeId;
use crate::sso::saml::scope::Scope;

pub const DEFAULT_PREFIX: &str = "#default";
const DEFAULT: &str = "";
const XML_PREFIX: &str = "xml";

enum Step {
    Enter(NodeId),
    Leave { id: NodeId, mark: usize },
}

pub fn exclusive(
    document: &Document,
    element: NodeId,
    excluded: Option<NodeId>,
    inclusive: &[String],
) -> Vec<u8> {
    let mut output = String::new();
    if document.element(element).is_none() || excluded == Some(element) {
        return output.into_bytes();
    }
    let prefixes = prefixes(inclusive);
    let ranks = ranks(document, element);
    let mut rendered = Scope::new();
    let mut steps = vec![Step::Enter(element)];
    while let Some(step) = steps.pop() {
        match step {
            Step::Enter(id) if excluded == Some(id) => {}
            Step::Enter(id) => match document.node(id) {
                Some(Node::Element(current)) => {
                    let mark = rendered.mark();
                    // Every element below the apex has its parent in the output, so an
                    // inclusive prefix it does not redeclare is already rendered with its
                    // in-scope value.
                    let inclusive = if id == element {
                        in_scope(document, id, &prefixes)
                    } else {
                        declared(current, &prefixes)
                    };
                    start(current, inclusive, &ranks, &mut rendered, &mut output);
                    steps.push(Step::Leave { id, mark });
                    steps.extend(current.children().iter().rev().copied().map(Step::Enter));
                }
                Some(Node::Text(text)) => escape_text(text, &mut output),
                Some(Node::Instruction { target, data }) => {
                    output.push_str("<?");
                    output.push_str(target);
                    if !data.is_empty() {
                        output.push(' ');
                        output.push_str(data);
                    }
                    output.push_str("?>");
                }
                Some(Node::Comment) | None => {}
            },
            Step::Leave { id, mark } => {
                if let Some(current) = document.element(id) {
                    output.push_str("</");
                    qualified(current.prefix(), current.local(), &mut output);
                    output.push('>');
                }
                rendered.restore(mark);
            }
        }
    }
    output.into_bytes()
}

pub(super) fn escape_text(value: &str, output: &mut String) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '\r' => output.push_str("&#xD;"),
            _ => output.push(character),
        }
    }
}

pub(super) fn escape_attribute(value: &str, output: &mut String) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '"' => output.push_str("&quot;"),
            '\t' => output.push_str("&#x9;"),
            '\n' => output.push_str("&#xA;"),
            '\r' => output.push_str("&#xD;"),
            _ => output.push(character),
        }
    }
}

fn prefixes(inclusive: &[String]) -> HashSet<&str> {
    inclusive
        .iter()
        .map(|prefix| {
            if prefix == DEFAULT_PREFIX {
                DEFAULT
            } else {
                prefix.as_str()
            }
        })
        .filter(|prefix| *prefix != XML_PREFIX)
        .collect()
}

fn ranks(document: &Document, apex: NodeId) -> HashMap<*const str, usize> {
    let mut seen = HashSet::new();
    let mut namespaces: Vec<&str> = document
        .descendants(apex)
        .filter_map(|id| document.element(id))
        .flat_map(Element::attributes)
        .filter_map(Attribute::namespace)
        .filter(|namespace| seen.insert(std::ptr::from_ref(*namespace)))
        .collect();
    namespaces.sort_unstable();
    let mut ranks = HashMap::with_capacity(namespaces.len());
    let mut rank = 0;
    let mut previous = None;
    for namespace in namespaces {
        if previous.is_some_and(|last| last != namespace) {
            rank += 1;
        }
        ranks.insert(std::ptr::from_ref(namespace), rank);
        previous = Some(namespace);
    }
    ranks
}

fn in_scope<'a>(
    document: &'a Document,
    apex: NodeId,
    prefixes: &HashSet<&str>,
) -> Vec<(&'a str, &'a str)> {
    let mut found: HashMap<&str, &str> = HashMap::new();
    let mut ancestor = Some(apex);
    while let Some(id) = ancestor {
        if let Some(current) = document.element(id) {
            for declaration in current.declarations() {
                let prefix = declaration.prefix().unwrap_or(DEFAULT);
                if prefixes.contains(prefix) {
                    found.entry(prefix).or_insert(declaration.uri());
                }
            }
        }
        ancestor = document.parent(id);
    }
    found.into_iter().collect()
}

fn declared<'a>(element: &'a Element, prefixes: &HashSet<&str>) -> Vec<(&'a str, &'a str)> {
    element
        .declarations()
        .iter()
        .map(|declaration| (declaration.prefix().unwrap_or(DEFAULT), declaration.uri()))
        .filter(|(prefix, _)| prefixes.contains(prefix))
        .collect()
}

fn start<'a>(
    element: &'a Element,
    mut candidates: Vec<(&'a str, &'a str)>,
    ranks: &HashMap<*const str, usize>,
    rendered: &mut Scope<&'a str, &'a str>,
    output: &mut String,
) {
    candidates.push((
        element.prefix().unwrap_or(DEFAULT),
        element.namespace().unwrap_or(DEFAULT),
    ));
    for attribute in element.attributes() {
        if let Some(prefix) = attribute.prefix() {
            candidates.push((prefix, attribute.namespace().unwrap_or(DEFAULT)));
        }
    }
    candidates.retain(|(prefix, _)| *prefix != XML_PREFIX);
    candidates.sort_unstable_by_key(|(prefix, _)| *prefix);
    candidates.dedup_by_key(|(prefix, _)| *prefix);
    output.push('<');
    qualified(element.prefix(), element.local(), output);
    for (prefix, uri) in candidates {
        let differs = match rendered.lookup(prefix) {
            Some(current) => !same(current, uri),
            None => !(prefix.is_empty() && uri.is_empty()),
        };
        if !differs {
            continue;
        }
        rendered.bind(prefix, uri);
        output.push_str(" xmlns");
        if !prefix.is_empty() {
            output.push(':');
            output.push_str(prefix);
        }
        output.push_str("=\"");
        escape_attribute(uri, output);
        output.push('"');
    }
    let mut attributes: Vec<&Attribute> = element.attributes().iter().collect();
    attributes.sort_unstable_by_key(|attribute| {
        let rank = attribute
            .namespace()
            .and_then(|namespace| ranks.get(&std::ptr::from_ref(namespace)))
            .copied();
        (rank, attribute.local())
    });
    for attribute in attributes {
        output.push(' ');
        qualified(attribute.prefix(), attribute.local(), output);
        output.push_str("=\"");
        escape_attribute(attribute.value(), output);
        output.push('"');
    }
    output.push('>');
}

fn same(left: &str, right: &str) -> bool {
    std::ptr::eq(left, right) || left == right
}

fn qualified(prefix: Option<&str>, local: &str, output: &mut String) {
    if let Some(prefix) = prefix {
        output.push_str(prefix);
        output.push(':');
    }
    output.push_str(local);
}
