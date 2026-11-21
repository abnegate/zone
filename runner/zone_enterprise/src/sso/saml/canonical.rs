use std::collections::{HashMap, HashSet};

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
    Leave(NodeId, usize, usize),
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
    let mut scope = inherited(document, element, &prefixes);
    let mut rendered = Scope::new();
    let mut steps = vec![Step::Enter(element)];
    while let Some(step) = steps.pop() {
        match step {
            Step::Enter(id) if excluded == Some(id) => {}
            Step::Enter(id) => match document.node(id) {
                Some(Node::Element(current)) => {
                    let marks = (scope.mark(), rendered.mark());
                    for declaration in current.declarations() {
                        let prefix = declaration.prefix().unwrap_or(DEFAULT);
                        if prefixes.contains(prefix) {
                            scope.bind(prefix, declaration.uri());
                        }
                    }
                    start(current, &prefixes, &scope, &mut rendered, &mut output);
                    steps.push(Step::Leave(id, marks.0, marks.1));
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
            Step::Leave(id, scope_mark, rendered_mark) => {
                if let Some(current) = document.element(id) {
                    output.push_str("</");
                    qualified(current.prefix(), current.local(), &mut output);
                    output.push('>');
                }
                scope.restore(scope_mark);
                rendered.restore(rendered_mark);
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

fn inherited<'a>(
    document: &'a Document,
    element: NodeId,
    prefixes: &HashSet<&'a str>,
) -> Scope<&'a str, &'a str> {
    let mut found: HashMap<&str, &str> = HashMap::new();
    let mut ancestor = document.parent(element);
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
    let mut scope = Scope::new();
    for (prefix, uri) in found {
        scope.bind(prefix, uri);
    }
    scope
}

fn start<'a>(
    element: &'a Element,
    prefixes: &HashSet<&'a str>,
    scope: &Scope<&'a str, &'a str>,
    rendered: &mut Scope<&'a str, &'a str>,
    output: &mut String,
) {
    let mut candidates = vec![(
        element.prefix().unwrap_or(DEFAULT),
        element.namespace().unwrap_or(DEFAULT),
    )];
    for attribute in element.attributes() {
        if let Some(prefix) = attribute.prefix() {
            candidates.push((prefix, attribute.namespace().unwrap_or(DEFAULT)));
        }
    }
    for prefix in prefixes {
        match scope.lookup(prefix) {
            Some(uri) => candidates.push((prefix, uri)),
            None if prefix.is_empty() => candidates.push((DEFAULT, DEFAULT)),
            None => {}
        }
    }
    candidates.retain(|(prefix, _)| *prefix != XML_PREFIX);
    candidates.sort_unstable();
    candidates.dedup_by_key(|(prefix, _)| *prefix);
    output.push('<');
    qualified(element.prefix(), element.local(), output);
    for (prefix, uri) in candidates {
        let differs = match rendered.lookup(prefix) {
            Some(current) => *current != uri,
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
    let mut attributes: Vec<_> = element.attributes().iter().collect();
    attributes.sort_unstable_by_key(|attribute| {
        (attribute.namespace().unwrap_or(DEFAULT), attribute.local())
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

fn qualified(prefix: Option<&str>, local: &str, output: &mut String) {
    if let Some(prefix) = prefix {
        output.push_str(prefix);
        output.push(':');
    }
    output.push_str(local);
}
