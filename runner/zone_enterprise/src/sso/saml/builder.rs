use std::borrow::Cow;
use std::collections::HashSet;

use quick_xml::errors::IllFormedError;
use quick_xml::escape::EscapeError;
use quick_xml::events::attributes::AttrError;
use quick_xml::events::{BytesDecl, BytesPI, BytesRef, BytesStart, Event};
use quick_xml::name::{PrefixDeclaration, QName};
use quick_xml::{Reader, XmlVersion};

use crate::sso::saml::attribute::Attribute;
use crate::sso::saml::document::Document;
use crate::sso::saml::element::Element;
use crate::sso::saml::malformed::Malformed;
use crate::sso::saml::namespace::Namespace;
use crate::sso::saml::node::Node;
use crate::sso::saml::node_id::NodeId;
use crate::sso::saml::scope::Scope;
use crate::sso::saml::uri;

const DEFAULT: &str = "";
const XML_PREFIX: &str = "xml";
const XMLNS_PREFIX: &str = "xmlns";
const ENCODING: &[u8] = b"UTF-8";
const ENTITY_DEPTH: usize = 1;

pub(super) struct Builder {
    nodes: Vec<Node>,
    parents: Vec<Option<NodeId>>,
    open: Vec<(NodeId, usize)>,
    scope: Scope<String, String>,
    text: String,
    root: Option<NodeId>,
    version: XmlVersion,
}

impl Builder {
    pub(super) fn new() -> Self {
        let mut scope = Scope::new();
        scope.bind(XML_PREFIX.to_owned(), uri::XML.to_owned());
        Self {
            nodes: Vec::new(),
            parents: Vec::new(),
            open: Vec::new(),
            scope,
            text: String::new(),
            root: None,
            version: XmlVersion::Implicit1_0,
        }
    }

    pub(super) fn build(mut self, input: &str) -> Result<Document, Malformed> {
        if !input.chars().all(allowed) {
            return Err(Malformed::Character);
        }
        let mut reader = Reader::from_str(input);
        reader.config_mut().check_comments = true;
        let mut leading = true;
        loop {
            let event = reader.read_event().map_err(malformed)?;
            let first = std::mem::replace(&mut leading, false);
            match event {
                Event::Decl(declaration) if first => self.declare(&declaration)?,
                Event::Decl(_) => return Err(Malformed::Syntax),
                Event::DocType(_) => return Err(Malformed::Doctype),
                Event::Start(start) => self.start(&start)?,
                Event::Empty(start) => {
                    self.start(&start)?;
                    self.end();
                }
                Event::End(_) => self.end(),
                Event::Text(text) => {
                    let content = text
                        .xml_content(self.version)
                        .map_err(|_| Malformed::Encoding)?;
                    self.characters(&content)?;
                }
                Event::CData(data) => {
                    let content = data
                        .xml_content(self.version)
                        .map_err(|_| Malformed::Encoding)?;
                    self.inside()?;
                    self.text.push_str(&content);
                }
                Event::GeneralRef(reference) => self.reference(&reference)?,
                Event::Comment(_) => self.leaf(Node::Comment),
                Event::PI(instruction) => self.instruction(&instruction)?,
                Event::Eof => break,
            }
        }
        if !self.open.is_empty() {
            return Err(Malformed::Syntax);
        }
        let root = self.root.ok_or(Malformed::Root)?;
        Ok(Document {
            nodes: self.nodes,
            parents: self.parents,
            root,
        })
    }

    fn declare(&mut self, declaration: &BytesDecl<'_>) -> Result<(), Malformed> {
        self.version = match declaration.xml_version().map_err(malformed)? {
            XmlVersion::Explicit1_1 => return Err(Malformed::Version),
            version => version,
        };
        match declaration.encoding() {
            None => Ok(()),
            Some(Ok(encoding)) if encoding.eq_ignore_ascii_case(ENCODING) => Ok(()),
            Some(Ok(_)) => Err(Malformed::Encoding),
            Some(Err(_)) => Err(Malformed::Syntax),
        }
    }

    fn inside(&self) -> Result<(), Malformed> {
        if self.open.is_empty() {
            Err(Malformed::Root)
        } else {
            Ok(())
        }
    }

    fn characters(&mut self, content: &str) -> Result<(), Malformed> {
        if self.open.is_empty() {
            return if content.chars().all(whitespace) {
                Ok(())
            } else {
                Err(Malformed::Root)
            };
        }
        self.text.push_str(content);
        Ok(())
    }

    fn reference(&mut self, reference: &BytesRef<'_>) -> Result<(), Malformed> {
        if reference.is_char_ref() {
            let character = reference
                .resolve_char_ref()
                .ok()
                .flatten()
                .filter(|character| allowed(*character))
                .ok_or(Malformed::Character)?;
            self.inside()?;
            self.text.push(character);
            return Ok(());
        }
        let name = reference.decode().map_err(|_| Malformed::Encoding)?;
        let replacement = predefined(&name).ok_or(Malformed::Entity)?;
        self.inside()?;
        self.text.push_str(replacement);
        Ok(())
    }

    fn instruction(&mut self, instruction: &BytesPI<'_>) -> Result<(), Malformed> {
        let target = utf8(instruction.target())?;
        if !name(target) || target.eq_ignore_ascii_case(XML_PREFIX) {
            return Err(Malformed::Name);
        }
        let data = utf8(instruction.content())?.trim_start_matches(whitespace);
        let node = Node::Instruction {
            target: target.to_owned(),
            data: lines(data).into_owned(),
        };
        self.leaf(node);
        Ok(())
    }

    fn leaf(&mut self, node: Node) {
        if let Some(parent) = self.parent() {
            self.flush(parent);
            self.insert(node, Some(parent));
        }
    }

    fn parent(&self) -> Option<NodeId> {
        self.open.last().map(|(id, _)| *id)
    }

    fn flush(&mut self, parent: NodeId) {
        if !self.text.is_empty() {
            let text = std::mem::take(&mut self.text);
            self.insert(Node::Text(text), Some(parent));
        }
    }

    fn insert(&mut self, node: Node, parent: Option<NodeId>) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(node);
        self.parents.push(parent);
        if let Some(parent) = parent
            && let Some(Node::Element(element)) = self.nodes.get_mut(parent.0)
        {
            element.children.push(id);
        }
        id
    }

    fn start(&mut self, start: &BytesStart<'_>) -> Result<(), Malformed> {
        let parent = self.parent();
        if parent.is_none() && self.root.is_some() {
            return Err(Malformed::Root);
        }
        let mark = self.scope.mark();
        let mut declarations = Vec::new();
        let mut attributes = Vec::new();
        for attribute in start.attributes() {
            let attribute = attribute.map_err(|error| match error {
                AttrError::Duplicated(..) => Malformed::DuplicateAttribute,
                _ => Malformed::Syntax,
            })?;
            if attribute.value.contains(&b'<') {
                return Err(Malformed::Syntax);
            }
            let value = attribute
                .normalized_value_with(self.version, ENTITY_DEPTH, predefined)
                .map_err(malformed)?;
            if !value.chars().all(allowed) {
                return Err(Malformed::Character);
            }
            match attribute.key.as_namespace_binding() {
                Some(PrefixDeclaration::Default) => {
                    let declaration = default(&value)?;
                    self.scope.bind(DEFAULT.to_owned(), value.into_owned());
                    declarations.push(declaration);
                }
                Some(PrefixDeclaration::Named(prefix)) => {
                    let prefix = utf8(prefix)?;
                    if let Some(declaration) = named(prefix, &value)? {
                        self.scope.bind(prefix.to_owned(), value.into_owned());
                        declarations.push(declaration);
                    }
                }
                None => attributes.push((attribute.key, value.into_owned())),
            }
        }
        let (prefix, local) = split(start.name())?;
        let namespace = match prefix {
            None => self
                .scope
                .lookup(DEFAULT)
                .filter(|namespace| !namespace.is_empty())
                .cloned(),
            Some(prefix) => Some(self.resolve(prefix)?),
        };
        let mut expanded = HashSet::with_capacity(attributes.len());
        let mut resolved = Vec::with_capacity(attributes.len());
        for (key, value) in attributes {
            let (prefix, local) = split(key)?;
            let namespace = prefix.map(|prefix| self.resolve(prefix)).transpose()?;
            if !expanded.insert((namespace.clone(), local.to_owned())) {
                return Err(Malformed::DuplicateAttribute);
            }
            resolved.push(Attribute {
                prefix: prefix.map(str::to_owned),
                local: local.to_owned(),
                namespace,
                value,
            });
        }
        let element = Node::Element(Element {
            prefix: prefix.map(str::to_owned),
            local: local.to_owned(),
            namespace,
            attributes: resolved,
            declarations,
            children: Vec::new(),
        });
        if let Some(parent) = parent {
            self.flush(parent);
        }
        let id = self.insert(element, parent);
        if parent.is_none() {
            self.root = Some(id);
        }
        self.open.push((id, mark));
        Ok(())
    }

    fn end(&mut self) {
        if let Some((id, mark)) = self.open.pop() {
            self.flush(id);
            self.scope.restore(mark);
        }
    }

    fn resolve(&self, prefix: &str) -> Result<String, Malformed> {
        if prefix == XMLNS_PREFIX {
            return Err(Malformed::Namespace);
        }
        self.scope.lookup(prefix).cloned().ok_or(Malformed::Prefix)
    }
}

fn default(value: &str) -> Result<Namespace, Malformed> {
    if value == uri::XML || value == uri::XMLNS {
        return Err(Malformed::Namespace);
    }
    Ok(Namespace {
        prefix: None,
        uri: value.to_owned(),
    })
}

fn named(prefix: &str, value: &str) -> Result<Option<Namespace>, Malformed> {
    if !ncname(prefix) {
        return Err(Malformed::Name);
    }
    if prefix == XML_PREFIX {
        return if value == uri::XML {
            Ok(None)
        } else {
            Err(Malformed::Namespace)
        };
    }
    if prefix == XMLNS_PREFIX || value.is_empty() || value == uri::XML || value == uri::XMLNS {
        return Err(Malformed::Namespace);
    }
    Ok(Some(Namespace {
        prefix: Some(prefix.to_owned()),
        uri: value.to_owned(),
    }))
}

fn split(name: QName<'_>) -> Result<(Option<&str>, &str), Malformed> {
    let (local, prefix) = name.decompose();
    let local = utf8(local.into_inner())?;
    let prefix = prefix.map(|prefix| utf8(prefix.into_inner())).transpose()?;
    if !ncname(local) || prefix.is_some_and(|prefix| !ncname(prefix)) {
        return Err(Malformed::Name);
    }
    Ok((prefix, local))
}

fn utf8(bytes: &[u8]) -> Result<&str, Malformed> {
    std::str::from_utf8(bytes).map_err(|_| Malformed::Encoding)
}

fn predefined(entity: &str) -> Option<&'static str> {
    match entity {
        "lt" => Some("<"),
        "gt" => Some(">"),
        "amp" => Some("&"),
        "apos" => Some("'"),
        "quot" => Some("\""),
        _ => None,
    }
}

fn malformed(error: quick_xml::Error) -> Malformed {
    match error {
        quick_xml::Error::Escape(EscapeError::UnrecognizedEntity(..)) => Malformed::Entity,
        quick_xml::Error::Escape(EscapeError::InvalidCharRef(_)) => Malformed::Character,
        quick_xml::Error::InvalidAttr(AttrError::Duplicated(..)) => Malformed::DuplicateAttribute,
        quick_xml::Error::IllFormed(IllFormedError::UnknownVersion) => Malformed::Version,
        quick_xml::Error::Encoding(_) => Malformed::Encoding,
        _ => Malformed::Syntax,
    }
}

fn lines(data: &str) -> Cow<'_, str> {
    if data.contains('\r') {
        Cow::Owned(data.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(data)
    }
}

fn allowed(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\r')
        || (character >= ' ' && !matches!(character, '\u{FFFE}' | '\u{FFFF}'))
}

fn whitespace(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r')
}

fn name(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|first| first == ':' || name_start(first))
        && characters.all(|character| character == ':' || name_character(character))
}

fn ncname(value: &str) -> bool {
    let mut characters = value.chars();
    characters.next().is_some_and(name_start) && characters.all(name_character)
}

fn name_start(character: char) -> bool {
    matches!(character,
        'A'..='Z'
        | '_'
        | 'a'..='z'
        | '\u{C0}'..='\u{D6}'
        | '\u{D8}'..='\u{F6}'
        | '\u{F8}'..='\u{2FF}'
        | '\u{370}'..='\u{37D}'
        | '\u{37F}'..='\u{1FFF}'
        | '\u{200C}'..='\u{200D}'
        | '\u{2070}'..='\u{218F}'
        | '\u{2C00}'..='\u{2FEF}'
        | '\u{3001}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FDCF}'
        | '\u{FDF0}'..='\u{FFFD}'
        | '\u{10000}'..='\u{EFFFF}')
}

fn name_character(character: char) -> bool {
    name_start(character)
        || matches!(character,
            '-'
            | '.'
            | '0'..='9'
            | '\u{B7}'
            | '\u{300}'..='\u{36F}'
            | '\u{203F}'..='\u{2040}')
}
