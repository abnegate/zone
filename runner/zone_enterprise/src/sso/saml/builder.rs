use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use quick_xml::errors::IllFormedError;
use quick_xml::escape::EscapeError;
use quick_xml::events::attributes::AttrError;
use quick_xml::events::{BytesDecl, BytesPI, BytesRef, BytesStart, Event};
use quick_xml::name::{PrefixDeclaration, QName};
use quick_xml::{Reader, XmlVersion};

use crate::sso::saml::attribute::Attribute;
use crate::sso::saml::document::Document;
use crate::sso::saml::element::Element;
use crate::sso::saml::frame::Frame;
use crate::sso::saml::interner::Interner;
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
    open: Vec<Frame>,
    scope: Scope<Arc<str>, Arc<str>>,
    interner: Interner,
    text: String,
    root: Option<NodeId>,
    version: XmlVersion,
}

impl Builder {
    pub(super) fn new() -> Self {
        let mut interner = Interner::default();
        let mut scope = Scope::new();
        scope.bind(interner.intern(XML_PREFIX), interner.intern(uri::XML));
        Self {
            nodes: Vec::new(),
            parents: Vec::new(),
            open: Vec::new(),
            scope,
            interner,
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
            size: input.len(),
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
        if !self.open.is_empty() {
            self.flush();
            self.insert(node);
        }
    }

    fn parent(&self) -> Option<NodeId> {
        self.open.last().map(|frame| frame.id)
    }

    fn flush(&mut self) {
        if !self.text.is_empty() && !self.open.is_empty() {
            let text = std::mem::take(&mut self.text);
            self.insert(Node::Text(text));
        }
    }

    fn insert(&mut self, node: Node) -> NodeId {
        let id = NodeId(self.nodes.len());
        let parent = self.open.last_mut().map(|frame| {
            frame.children.push(id);
            frame.id
        });
        self.nodes.push(node);
        self.parents.push(parent);
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
                    default(&value)?;
                    declarations.push(self.bind(None, &value));
                }
                Some(PrefixDeclaration::Named(prefix)) => {
                    let prefix = utf8(prefix)?;
                    if named(prefix, &value)? {
                        declarations.push(self.bind(Some(prefix), &value));
                    }
                }
                None => attributes.push((attribute.key, value.into_owned().into_boxed_str())),
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
            let identity = namespace.as_deref().map(std::ptr::from_ref);
            if !expanded.insert((identity, local)) {
                return Err(Malformed::DuplicateAttribute);
            }
            resolved.push(Attribute {
                name: self.interner.name(prefix, local, namespace),
                value,
            });
        }
        let element = Node::Element(Element {
            name: self.interner.name(prefix, local, namespace),
            attributes: resolved.into_boxed_slice(),
            declarations: declarations.into_boxed_slice(),
            children: Box::default(),
        });
        self.flush();
        let id = self.insert(element);
        if parent.is_none() {
            self.root = Some(id);
        }
        self.open.push(Frame {
            id,
            mark,
            children: Vec::new(),
        });
        Ok(())
    }

    fn end(&mut self) {
        self.flush();
        if let Some(frame) = self.open.pop() {
            if let Some(Node::Element(element)) = self.nodes.get_mut(frame.id.0) {
                element.children = frame.children.into_boxed_slice();
            }
            self.scope.restore(frame.mark);
        }
    }

    fn bind(&mut self, prefix: Option<&str>, uri: &str) -> Namespace {
        let prefix = prefix.map(|prefix| self.interner.intern(prefix));
        let uri = self.interner.intern(uri);
        let key = match &prefix {
            Some(prefix) => Arc::clone(prefix),
            None => self.interner.intern(DEFAULT),
        };
        self.scope.bind(key, Arc::clone(&uri));
        Namespace { prefix, uri }
    }

    fn resolve(&self, prefix: &str) -> Result<Arc<str>, Malformed> {
        if prefix == XMLNS_PREFIX {
            return Err(Malformed::Namespace);
        }
        self.scope.lookup(prefix).cloned().ok_or(Malformed::Prefix)
    }
}

fn default(value: &str) -> Result<(), Malformed> {
    if value == uri::XML || value == uri::XMLNS {
        Err(Malformed::Namespace)
    } else {
        Ok(())
    }
}

fn named(prefix: &str, value: &str) -> Result<bool, Malformed> {
    if !ncname(prefix) {
        return Err(Malformed::Name);
    }
    if prefix == XML_PREFIX {
        return if value == uri::XML {
            Ok(false)
        } else {
            Err(Malformed::Namespace)
        };
    }
    if prefix == XMLNS_PREFIX || value.is_empty() || value == uri::XML || value == uri::XMLNS {
        return Err(Malformed::Namespace);
    }
    Ok(true)
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
