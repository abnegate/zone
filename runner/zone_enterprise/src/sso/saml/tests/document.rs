use std::time::{Duration, Instant};

use super::{DEEP, nested};
use crate::sso::saml::{Document, Malformed, Node, uri};

const OKTA: &[u8] = include_bytes!("fixtures/canonical/okta.xml");
const ENTRA: &[u8] = include_bytes!("fixtures/canonical/entra.xml");
const BOUND: Duration = Duration::from_secs(10);

fn parse(xml: &str) -> Result<Document, Malformed> {
    Document::parse(xml.as_bytes())
}

fn refused(xml: &str) -> Malformed {
    parse(xml).expect_err(xml)
}

fn first(document: &Document, namespace: &str, local: &str) -> crate::sso::saml::NodeId {
    document
        .descendants(document.root())
        .find(|id| document.is(*id, namespace, local))
        .unwrap_or_else(|| panic!("no {local}"))
}

#[test]
fn resolves_prefixed_namespaces() {
    let document = Document::parse(OKTA).unwrap();
    let root = document.root();
    assert!(document.is(root, uri::PROTOCOL, "Response"));
    let response = document.element(root).unwrap();
    assert_eq!(response.prefix(), Some("saml2p"));
    assert_eq!(response.local(), "Response");
    assert_eq!(response.namespace(), Some(uri::PROTOCOL));
    assert_eq!(response.attribute("ID"), Some("id-response"));
    assert_eq!(response.attribute("Missing"), None);
    assert_eq!(response.declarations().len(), 1);
    assert_eq!(response.declarations()[0].prefix(), Some("saml2p"));
    assert_eq!(response.declarations()[0].uri(), uri::PROTOCOL);
    assert_eq!(document.parent(root), None);

    let assertions: Vec<_> = document
        .children_named(root, uri::ASSERTION, "Assertion")
        .collect();
    assert_eq!(assertions.len(), 1);
    let assertion = assertions[0];
    assert_eq!(document.parent(assertion), Some(root));
    assert_eq!(
        document.element(assertion).unwrap().attribute("ID"),
        Some("id-assertion")
    );
    let names: Vec<&str> = document
        .child_elements(assertion)
        .map(|id| document.element(id).unwrap().local())
        .collect();
    assert_eq!(
        names,
        [
            "Issuer",
            "Subject",
            "Conditions",
            "AuthnStatement",
            "AttributeStatement"
        ]
    );
    let name_id = first(&document, uri::ASSERTION, "NameID");
    assert_eq!(document.text(name_id).as_deref(), Some("jane@acme.com"));
}

#[test]
fn qualified_attributes_resolve_and_unqualified_ones_have_no_namespace() {
    let document = Document::parse(OKTA).unwrap();
    let value = first(&document, uri::ASSERTION, "AttributeValue");
    let element = document.element(value).unwrap();
    assert_eq!(
        element.attribute_in(uri::XML_SCHEMA_INSTANCE, "nil"),
        Some("false")
    );
    assert_eq!(element.attribute_in(uri::XML, "lang"), Some("en"));
    assert_eq!(element.attribute("nil"), None);
    assert_eq!(element.attribute("Extra"), Some("1"));
    let extra = element
        .attributes()
        .iter()
        .find(|attribute| attribute.local() == "Extra")
        .unwrap();
    assert_eq!(extra.namespace(), None);
    assert_eq!(extra.prefix(), None);
}

#[test]
fn resolves_default_namespaces_and_their_undeclaration() {
    let document = Document::parse(ENTRA).unwrap();
    let assertion = first(&document, uri::ASSERTION, "Assertion");
    let element = document.element(assertion).unwrap();
    assert_eq!(element.prefix(), None);
    assert_eq!(element.namespace(), Some(uri::ASSERTION));
    let advice = document
        .child_elements(assertion)
        .find(|id| document.element(*id).unwrap().local() == "Advice")
        .unwrap();
    assert_eq!(document.element(advice).unwrap().namespace(), None);
    let back = document
        .descendants(advice)
        .find(|id| document.is(*id, uri::ASSERTION, "Back"))
        .unwrap();
    let inner = document.child_elements(back).next().unwrap();
    assert_eq!(document.element(inner).unwrap().namespace(), None);
}

#[test]
fn an_unprefixed_attribute_ignores_the_default_namespace() {
    let document = parse(r#"<a xmlns="urn:a" b="1"/>"#).unwrap();
    let element = document.element(document.root()).unwrap();
    assert_eq!(element.namespace(), Some("urn:a"));
    assert_eq!(element.attributes()[0].namespace(), None);
}

#[test]
fn descendants_walk_in_document_order_from_the_node_itself() {
    let document = parse("<a><b><c/></b>t<d/></a>").unwrap();
    let names: Vec<String> = document
        .descendants(document.root())
        .map(|id| match document.node(id).unwrap() {
            Node::Element(element) => element.local().to_owned(),
            Node::Text(text) => format!("'{text}'"),
            Node::Comment => "comment".to_owned(),
            Node::Instruction { target, .. } => target.clone(),
        })
        .collect();
    assert_eq!(names, ["a", "b", "c", "'t'", "d"]);
}

#[test]
fn text_joins_around_comments_and_instructions() {
    let document = parse("<NameID>jane<!-- injected -->@acme<?pi data?>.com</NameID>").unwrap();
    let root = document.root();
    assert_eq!(document.children(root).len(), 5);
    assert_eq!(document.text(root).as_deref(), Some("jane@acme.com"));
}

#[test]
fn text_is_none_for_mixed_content() {
    let document = parse("<a>one<b/>two</a>").unwrap();
    assert_eq!(document.text(document.root()), None);
    let empty = parse("<a/>").unwrap();
    assert_eq!(empty.text(empty.root()).as_deref(), Some(""));
}

#[test]
fn cdata_and_references_merge_into_one_text_node() {
    let document = parse("<a>x<![CDATA[<y>]]>&amp;&#x7A;</a>").unwrap();
    let root = document.root();
    assert_eq!(document.children(root).len(), 1);
    assert_eq!(
        document.node(document.children(root)[0]),
        Some(&Node::Text("x<y>&z".to_owned()))
    );
}

#[test]
fn a_character_reference_carriage_return_survives_line_end_normalization() {
    let document = parse("<a b=\"1&#13;2\r\n3\">x\r\ny\rz&#13;</a>").unwrap();
    let root = document.root();
    assert_eq!(document.text(root).as_deref(), Some("x\ny\nz\r"));
    assert_eq!(
        document.element(root).unwrap().attribute("b"),
        Some("1\r2 3")
    );
}

#[test]
fn attribute_values_resolve_the_predefined_entities() {
    let document = parse(r#"<a b="&lt;&gt;&amp;&apos;&quot;&#x41;"/>"#).unwrap();
    assert_eq!(
        document.element(document.root()).unwrap().attribute("b"),
        Some("<>&'\"A")
    );
}

#[test]
fn strips_a_byte_order_mark_and_accepts_a_utf8_declaration() {
    let mut input = vec![0xEF, 0xBB, 0xBF];
    input.extend_from_slice(br#"<?xml version="1.0" encoding="utf-8"?><a/>"#);
    let document = Document::parse(&input).unwrap();
    assert_eq!(document.element(document.root()).unwrap().local(), "a");
}

#[test]
fn keeps_processing_instructions_without_leading_space() {
    let document = parse("<a><?target   some data?></a>").unwrap();
    let child = document.children(document.root())[0];
    assert_eq!(
        document.node(child),
        Some(&Node::Instruction {
            target: "target".to_owned(),
            data: "some data".to_owned()
        })
    );
}

#[test]
fn ignores_comments_and_instructions_outside_the_root() {
    let document = parse("<!-- before --><?pi x?>\n<a/>\n<!-- after -->").unwrap();
    assert!(document.children(document.root()).is_empty());
}

#[test]
fn refuses_a_document_type_with_an_external_entity() {
    assert_eq!(
        refused(
            r#"<?xml version="1.0"?><!DOCTYPE foo [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><foo>&xxe;</foo>"#
        ),
        Malformed::Doctype
    );
}

#[test]
fn refuses_the_billion_laughs() {
    let xml = r#"<?xml version="1.0"?>
<!DOCTYPE lolz [
  <!ENTITY lol "lol">
  <!ENTITY lol1 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">
  <!ENTITY lol2 "&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;">
]>
<lolz>&lol2;</lolz>"#;
    assert_eq!(refused(xml), Malformed::Doctype);
}

#[test]
fn refuses_an_unknown_entity() {
    assert_eq!(refused("<a>&nbsp;</a>"), Malformed::Entity);
    assert_eq!(refused(r#"<a b="&nbsp;"/>"#), Malformed::Entity);
    assert_eq!(refused("<a>&xxe;</a>"), Malformed::Entity);
}

#[test]
fn refuses_input_that_is_not_utf8() {
    assert_eq!(
        Document::parse(b"<a>\xff</a>").unwrap_err(),
        Malformed::Encoding
    );
    assert_eq!(
        Document::parse(&[0xFF, 0xFE, b'<', 0, b'a', 0, b'/', 0, b'>', 0]).unwrap_err(),
        Malformed::Encoding
    );
    assert_eq!(
        refused(r#"<?xml version="1.0" encoding="ISO-8859-1"?><a/>"#),
        Malformed::Encoding
    );
}

#[test]
fn refuses_xml_other_than_version_one_point_zero() {
    assert_eq!(refused(r#"<?xml version="1.1"?><a/>"#), Malformed::Version);
    assert_eq!(refused(r#"<?xml version="2.0"?><a/>"#), Malformed::Version);
}

#[test]
fn refuses_a_misplaced_declaration() {
    assert_eq!(refused(r#" <?xml version="1.0"?><a/>"#), Malformed::Syntax);
    assert_eq!(
        refused(r#"<a><?xml version="1.0"?></a>"#),
        Malformed::Syntax
    );
}

#[test]
fn refuses_unbound_prefixes() {
    assert_eq!(refused("<p:a/>"), Malformed::Prefix);
    assert_eq!(refused(r#"<a p:b="1"/>"#), Malformed::Prefix);
    assert_eq!(
        refused(r#"<a><p:b xmlns:p="urn:p"/><p:c/></a>"#),
        Malformed::Prefix
    );
}

#[test]
fn refuses_misused_reserved_namespaces() {
    for xml in [
        r#"<a xmlns:p=""/>"#,
        r#"<a xmlns:xml="urn:other"/>"#,
        r#"<a xmlns:xmlns="urn:other"/>"#,
        r#"<a xmlns:p="http://www.w3.org/XML/1998/namespace"/>"#,
        r#"<a xmlns:p="http://www.w3.org/2000/xmlns/"/>"#,
        r#"<a xmlns="http://www.w3.org/2000/xmlns/"/>"#,
        "<xmlns:a/>",
    ] {
        assert_eq!(refused(xml), Malformed::Namespace, "{xml}");
    }
    assert!(
        parse(r#"<a xmlns:xml="http://www.w3.org/XML/1998/namespace" xml:lang="en"/>"#).is_ok()
    );
}

#[test]
fn refuses_duplicate_attributes() {
    assert_eq!(
        refused(r#"<a b="1" b="2"/>"#),
        Malformed::DuplicateAttribute
    );
    assert_eq!(
        refused(r#"<a xmlns:p="urn:x" xmlns:q="urn:x" p:b="1" q:b="2"/>"#),
        Malformed::DuplicateAttribute
    );
    assert!(parse(r#"<a xmlns:p="urn:x" xmlns:q="urn:y" p:b="1" q:b="2" b="3"/>"#).is_ok());
}

#[test]
fn refuses_anything_but_exactly_one_root() {
    assert_eq!(refused("<a/><b/>"), Malformed::Root);
    assert_eq!(refused(""), Malformed::Root);
    assert_eq!(refused("<!-- only a comment -->"), Malformed::Root);
    assert_eq!(refused("text<a/>"), Malformed::Root);
    assert_eq!(refused("<a/>text"), Malformed::Root);
    assert_eq!(refused("<a/>&amp;"), Malformed::Root);
}

#[test]
fn refuses_unbalanced_markup() {
    assert_eq!(refused("<a>"), Malformed::Syntax);
    assert_eq!(refused("<a></b>"), Malformed::Syntax);
    assert_eq!(refused("<a/></a>"), Malformed::Syntax);
    assert_eq!(refused(r#"<a b="1></a>"#), Malformed::Syntax);
    assert_eq!(refused("<a><!-- x -- y --></a>"), Malformed::Syntax);
}

#[test]
fn refuses_characters_xml_does_not_allow() {
    assert_eq!(refused("<a>\u{1}</a>"), Malformed::Character);
    assert_eq!(refused("<a>&#1;</a>"), Malformed::Character);
    assert_eq!(refused("<a>&#0;</a>"), Malformed::Character);
    assert_eq!(refused(r#"<a b="&#1;"/>"#), Malformed::Character);
    assert_eq!(refused("<a>&#xFFFE;</a>"), Malformed::Character);
}

#[test]
fn refuses_invalid_names() {
    assert_eq!(refused("<1a/>"), Malformed::Name);
    assert_eq!(refused("<a:b:c xmlns:a=\"urn:a\"/>"), Malformed::Name);
    assert_eq!(refused(r#"<a xmlns:1="urn:a"/>"#), Malformed::Name);
}

#[test]
fn parses_a_hundred_thousand_levels_without_recursion() {
    let xml = nested(DEEP);
    let started = Instant::now();
    let document = parse(&xml).unwrap();
    assert_eq!(document.descendants(document.root()).count(), DEEP + 1);
    let mut deepest = document
        .descendants(document.root())
        .find(|id| matches!(document.node(*id), Some(Node::Text(_))))
        .unwrap();
    let mut depth = 0;
    while let Some(parent) = document.parent(deepest) {
        deepest = parent;
        depth += 1;
    }
    assert_eq!(depth, DEEP);
    drop(document);
    assert!(started.elapsed() < BOUND, "{:?}", started.elapsed());
}
