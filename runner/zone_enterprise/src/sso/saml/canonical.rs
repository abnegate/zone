use std::collections::{HashMap, HashSet};

use crate::sso::saml::attribute::Attribute;
use crate::sso::saml::budget::Budget;
use crate::sso::saml::document::Document;
use crate::sso::saml::element::Element;
use crate::sso::saml::node::Node;
use crate::sso::saml::node_id::NodeId;
use crate::sso::saml::refusal::Refusal;
use crate::sso::saml::scope::Scope;
use crate::sso::saml::sink::Sink;

pub const DEFAULT_PREFIX: &str = "#default";
pub(super) const GROWTH: usize = 8;
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
) -> Result<Vec<u8>, Refusal> {
    let mut output = Vec::new();
    stream(document, element, excluded, inclusive, &mut output)?;
    Ok(output)
}

pub(super) fn stream(
    document: &Document,
    element: NodeId,
    excluded: Option<NodeId>,
    inclusive: &[String],
    sink: &mut impl Sink,
) -> Result<(), Refusal> {
    let mut output = Budget::new(sink, document.size().saturating_mul(GROWTH));
    render(document, element, excluded, inclusive, &mut output);
    if output.exhausted() {
        Err(Refusal::Oversized)
    } else {
        Ok(())
    }
}

#[cfg(any(test, feature = "signing"))]
pub(super) fn unbounded(
    document: &Document,
    element: NodeId,
    excluded: Option<NodeId>,
    inclusive: &[String],
) -> Vec<u8> {
    let mut output = Vec::new();
    render(
        document,
        element,
        excluded,
        inclusive,
        &mut Budget::new(&mut output, usize::MAX),
    );
    output
}

fn render<S: Sink>(
    document: &Document,
    element: NodeId,
    excluded: Option<NodeId>,
    inclusive: &[String],
    output: &mut Budget<'_, S>,
) {
    if document.element(element).is_none() || excluded == Some(element) {
        return;
    }
    let prefixes = prefixes(inclusive);
    let ranks = ranks(document, element);
    let mut rendered = Scope::new();
    let mut steps = vec![Step::Enter(element)];
    while let Some(step) = steps.pop() {
        if output.exhausted() {
            return;
        }
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
                    start(current, inclusive, &ranks, &mut rendered, output);
                    steps.push(Step::Leave { id, mark });
                    steps.extend(current.children().iter().rev().copied().map(Step::Enter));
                }
                Some(Node::Text(text)) => escape_text(text, output),
                Some(Node::Instruction { target, data }) => {
                    output.push_str("<?");
                    output.push_str(target);
                    if !data.is_empty() {
                        output.push_str(" ");
                        output.push_str(data);
                    }
                    output.push_str("?>");
                }
                Some(Node::Comment) | None => {}
            },
            Step::Leave { id, mark } => {
                if let Some(current) = document.element(id) {
                    output.push_str("</");
                    qualified(current.prefix(), current.local(), output);
                    output.push_str(">");
                }
                rendered.restore(mark);
            }
        }
    }
}

pub(super) fn escape_text(value: &str, output: &mut impl Sink) {
    escape(value, output, |character| match character {
        '&' => Some("&amp;"),
        '<' => Some("&lt;"),
        '>' => Some("&gt;"),
        '\r' => Some("&#xD;"),
        _ => None,
    });
}

pub(super) fn escape_attribute(value: &str, output: &mut impl Sink) {
    escape(value, output, |character| match character {
        '&' => Some("&amp;"),
        '<' => Some("&lt;"),
        '"' => Some("&quot;"),
        '\t' => Some("&#x9;"),
        '\n' => Some("&#xA;"),
        '\r' => Some("&#xD;"),
        _ => None,
    });
}

fn escape(value: &str, output: &mut impl Sink, entity: impl Fn(char) -> Option<&'static str>) {
    let mut unescaped = 0;
    for (index, character) in value.char_indices() {
        if let Some(replacement) = entity(character) {
            output.push_str(&value[unescaped..index]);
            output.push_str(replacement);
            unescaped = index + character.len_utf8();
        }
    }
    output.push_str(&value[unescaped..]);
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
    output: &mut impl Sink,
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
    output.push_str("<");
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
            output.push_str(":");
            output.push_str(prefix);
        }
        output.push_str("=\"");
        escape_attribute(uri, output);
        output.push_str("\"");
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
        output.push_str(" ");
        qualified(attribute.prefix(), attribute.local(), output);
        output.push_str("=\"");
        escape_attribute(attribute.value(), output);
        output.push_str("\"");
    }
    output.push_str(">");
}

fn same(left: &str, right: &str) -> bool {
    std::ptr::eq(left, right) || left == right
}

fn qualified(prefix: Option<&str>, local: &str, output: &mut impl Sink) {
    if let Some(prefix) = prefix {
        output.push_str(prefix);
        output.push_str(":");
    }
    output.push_str(local);
}
