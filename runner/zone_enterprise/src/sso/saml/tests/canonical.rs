use std::time::{Duration, Instant};

use super::{AMPLIFYING_CHILDREN, DEEP, WIDE, amplified, amplified_with, nested};
use crate::sso::saml::canonical::{DEFAULT_PREFIX, GROWTH, exclusive, unbounded};
use crate::sso::saml::{Document, NodeId, Refusal, uri};

const BOUND: Duration = Duration::from_secs(10);
const LONG_NAMESPACE_LENGTH: usize = 1_000_000;
const SHORT_NAMESPACE_LENGTH: usize = 100;
const BOUNDARY_CHILDREN: usize = 64;
const EMPTY_ROOT: &str = "<r></r>";
const ROOT_START: &str = "<r";
const ROOT_END: &str = "</r>";

fn document(xml: &str) -> Document {
    Document::parse(xml.as_bytes()).unwrap()
}

fn whole(input: &[u8]) -> String {
    let document = Document::parse(input).unwrap();
    String::from_utf8(exclusive(&document, document.root(), None, &[]).unwrap()).unwrap()
}

fn named(document: &Document, local: &str) -> NodeId {
    document
        .descendants(document.root())
        .find(|id| {
            document
                .element(*id)
                .is_some_and(|element| element.local() == local)
        })
        .unwrap_or_else(|| panic!("no {local}"))
}

fn subset(xml: &str, local: &str, inclusive: &[&str]) -> String {
    let document = document(xml);
    let apex = named(&document, local);
    let inclusive: Vec<String> = inclusive
        .iter()
        .map(|prefix| (*prefix).to_owned())
        .collect();
    String::from_utf8(exclusive(&document, apex, None, &inclusive).unwrap()).unwrap()
}

fn matches_xmllint(input: &[u8], expected: &[u8]) {
    assert_eq!(whole(input), String::from_utf8(expected.to_vec()).unwrap());
}

/// Exc-c14n section 3 with C14N 1.0 section 2.3: whitespace outside the root
/// and the XML declaration are dropped, whitespace text inside is kept byte
/// for byte, empty elements become start-end pairs, attributes sort by
/// namespace URI then local name with unqualified ones first, a namespace is
/// rendered only where it is visibly used and not already in the output
/// scope, and a processing instruction's data loses its leading whitespace.
#[test]
fn an_okta_response_matches_xmllint() {
    matches_xmllint(
        include_bytes!("fixtures/canonical/okta.xml"),
        include_bytes!("fixtures/canonical/okta.c14n"),
    );
}

/// Exc-c14n section 3: the default namespace is visibly used by an
/// unprefixed element; a redundant redeclaration of the same value is not
/// rendered again; `xmlns=""` is rendered only when the nearest output
/// ancestor rendered a non-empty default namespace.
#[test]
fn an_entra_default_namespace_response_matches_xmllint() {
    matches_xmllint(
        include_bytes!("fixtures/canonical/entra.xml"),
        include_bytes!("fixtures/canonical/entra.c14n"),
    );
}

/// C14N 1.0 section 2.2: line ends normalize to `#xA`, CDATA sections become
/// escaped text, text escapes `&`, `<`, `>` and `#xD`, and attribute values
/// escape `&`, `<`, `"`, `#x9`, `#xA` and `#xD` after attribute-value
/// normalization.
#[test]
fn line_ends_cdata_and_escaping_match_xmllint() {
    matches_xmllint(
        include_bytes!("fixtures/canonical/escaping.xml"),
        include_bytes!("fixtures/canonical/escaping.c14n"),
    );
}

/// Exc-c14n section 3: a namespace used only inside an attribute value
/// (`xsi:type="xs:string"`) is not visibly used, so without an
/// InclusiveNamespaces PrefixList its declaration is dropped.
#[test]
fn an_unused_schema_prefix_is_dropped_like_xmllint() {
    matches_xmllint(
        include_bytes!("fixtures/canonical/xsi.xml"),
        include_bytes!("fixtures/canonical/xsi.c14n"),
    );
}

/// Exc-c14n section 2.2, the example document pair: canonicalizing the
/// `n1:elem2` subset gives the same bytes in both contexts, because
/// exclusive canonicalization neither imports unused ancestor namespaces
/// nor inherits ancestor `xml:*` attributes.
#[test]
fn the_specification_example_is_independent_of_its_context() {
    let first = r#"<n0:local xmlns:n0="foo:bar" xmlns:n3="ftp://example.org">
  <n1:elem2 xmlns:n1="http://example.net" xml:lang="en">
    <n3:stuff xmlns:n3="ftp://example.org"/>
  </n1:elem2>
</n0:local>"#;
    let second = r#"<n2:pdu xmlns:n1="http://example.com" xmlns:n2="http://foo.example" xml:lang="fr" xml:space="retain">
  <n1:elem2 xmlns:n1="http://example.net" xml:lang="en">
    <n3:stuff xmlns:n3="ftp://example.org"/>
  </n1:elem2>
</n2:pdu>"#;
    let expected = "<n1:elem2 xmlns:n1=\"http://example.net\" xml:lang=\"en\">\n    <n3:stuff xmlns:n3=\"ftp://example.org\"></n3:stuff>\n  </n1:elem2>";
    assert_eq!(subset(first, "elem2", &[]), expected);
    assert_eq!(subset(second, "elem2", &[]), expected);
}

/// Exc-c14n section 3: an ancestor's `xml:*` attributes are not inherited by
/// the apex, unlike inclusive C14N 1.0.
#[test]
fn the_apex_does_not_inherit_xml_attributes() {
    assert_eq!(
        subset(
            r#"<a xml:lang="fr" xml:space="preserve"><b/></a>"#,
            "b",
            &[]
        ),
        "<b></b>"
    );
}

/// Exc-c14n section 3: the apex renders every namespace it visibly uses even
/// when that namespace is declared only on an ancestor outside the subset,
/// which is how an Okta Assertion inherits `saml2` from its Response. The
/// expected bytes are xmllint's output for a standalone copy of the
/// Assertion that declares only the namespaces it visibly uses.
#[test]
fn the_apex_renders_namespaces_declared_outside_the_subset() {
    let response = r#"<saml2p:Response xmlns:saml2p="urn:oasis:names:tc:SAML:2.0:protocol" xmlns:saml2="urn:oasis:names:tc:SAML:2.0:assertion" xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" ID="r"><saml2:Assertion Version="2.0" ID="a"><saml2:Issuer>idp</saml2:Issuer><saml2:AttributeStatement><saml2:Attribute Name="email"><saml2:AttributeValue xsi:type="xs:string">jane@acme.com</saml2:AttributeValue></saml2:Attribute></saml2:AttributeStatement></saml2:Assertion></saml2p:Response>"#;
    assert_eq!(
        subset(response, "Assertion", &[]),
        r#"<saml2:Assertion xmlns:saml2="urn:oasis:names:tc:SAML:2.0:assertion" ID="a" Version="2.0"><saml2:Issuer>idp</saml2:Issuer><saml2:AttributeStatement><saml2:Attribute Name="email"><saml2:AttributeValue xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="xs:string">jane@acme.com</saml2:AttributeValue></saml2:Attribute></saml2:AttributeStatement></saml2:Assertion>"#
    );
    assert_eq!(
        subset(response, "Assertion", &["xs"]),
        r#"<saml2:Assertion xmlns:saml2="urn:oasis:names:tc:SAML:2.0:assertion" xmlns:xs="http://www.w3.org/2001/XMLSchema" ID="a" Version="2.0"><saml2:Issuer>idp</saml2:Issuer><saml2:AttributeStatement><saml2:Attribute Name="email"><saml2:AttributeValue xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="xs:string">jane@acme.com</saml2:AttributeValue></saml2:Attribute></saml2:AttributeStatement></saml2:Assertion>"#
    );
}

/// Exc-c14n section 3: a prefix on the InclusiveNamespaces PrefixList is
/// treated as in C14N 1.0, rendered where it is in scope and not already
/// rendered with the same value by an output ancestor.
#[test]
fn an_inclusive_prefix_is_rendered_once_where_it_is_in_scope() {
    let xml = include_str!("fixtures/canonical/xsi.xml");
    let document = document(xml);
    let inclusive = vec!["xs".to_owned()];
    assert_eq!(
        String::from_utf8(exclusive(&document, document.root(), None, &inclusive).unwrap())
            .unwrap(),
        "<saml:AttributeStatement xmlns:saml=\"urn:oasis:names:tc:SAML:2.0:assertion\" xmlns:xs=\"http://www.w3.org/2001/XMLSchema\">\n  <saml:Attribute Name=\"email\">\n    <saml:AttributeValue xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"xs:string\">jane@acme.com</saml:AttributeValue>\n  </saml:Attribute>\n</saml:AttributeStatement>"
    );
    assert_eq!(
        subset(xml, "AttributeValue", &["xs"]),
        "<saml:AttributeValue xmlns:saml=\"urn:oasis:names:tc:SAML:2.0:assertion\" xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"xs:string\">jane@acme.com</saml:AttributeValue>"
    );
}

/// Exc-c14n section 3: an inclusive prefix that isn't in scope renders
/// nothing, and `xml` is never rendered.
#[test]
fn an_inclusive_prefix_out_of_scope_renders_nothing() {
    assert_eq!(
        subset(r#"<a xmlns:p="urn:p"><b/></a>"#, "b", &["q", "xml"]),
        "<b></b>"
    );
}

/// Exc-c14n section 3: `#default` on the PrefixList stands for the default
/// namespace, rendered at the apex when it is in scope and non-empty.
#[test]
fn the_default_token_renders_the_inherited_default_namespace() {
    let xml = r#"<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" xmlns="urn:x"><samlp:Status/></samlp:Response>"#;
    assert_eq!(
        subset(xml, "Status", &[]),
        r#"<samlp:Status xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol"></samlp:Status>"#
    );
    assert_eq!(
        subset(xml, "Status", &[DEFAULT_PREFIX]),
        r#"<samlp:Status xmlns="urn:x" xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol"></samlp:Status>"#
    );
    assert_eq!(
        subset(
            r#"<a xmlns="urn:x"><b xmlns=""/></a>"#,
            "b",
            &[DEFAULT_PREFIX]
        ),
        "<b></b>"
    );
}

/// Exc-c14n section 3: at the apex nothing has been rendered yet, so an
/// unprefixed element without a namespace gets no `xmlns=""`, while one in
/// an inherited default namespace renders it.
#[test]
fn the_apex_renders_the_default_namespace_only_when_it_is_used() {
    assert_eq!(
        subset(r#"<a xmlns="urn:a"><b xmlns=""><c/></b></a>"#, "b", &[]),
        "<b><c></c></b>"
    );
    assert_eq!(
        subset(r#"<a xmlns="urn:a"><b><c/></b></a>"#, "b", &[]),
        r#"<b xmlns="urn:a"><c></c></b>"#
    );
}

/// XMLDSig section 6.6.4, the enveloped-signature transform: the excluded
/// Signature subtree is omitted while the text around it is kept. The
/// expected bytes are xmllint's output for the same document with the
/// Signature element deleted.
#[test]
fn omits_the_excluded_node() {
    let xml = "<saml2:Assertion xmlns:saml2=\"urn:a\" xmlns:ds=\"http://www.w3.org/2000/09/xmldsig#\" ID=\"a\">\n  <saml2:Issuer>idp</saml2:Issuer>\n  <ds:Signature><ds:SignedInfo/></ds:Signature>\n  <saml2:Subject/>\n</saml2:Assertion>";
    let document = document(xml);
    let signature = document
        .descendants(document.root())
        .find(|id| document.is(*id, uri::DSIG, "Signature"))
        .unwrap();
    assert_eq!(
        String::from_utf8(exclusive(&document, document.root(), Some(signature), &[]).unwrap())
            .unwrap(),
        "<saml2:Assertion xmlns:saml2=\"urn:a\" ID=\"a\">\n  <saml2:Issuer>idp</saml2:Issuer>\n  \n  <saml2:Subject></saml2:Subject>\n</saml2:Assertion>"
    );
}

#[test]
fn excluding_the_apex_or_naming_a_text_node_renders_nothing() {
    let document = document("<a>text</a>");
    let root = document.root();
    assert!(
        exclusive(&document, root, Some(root), &[])
            .unwrap()
            .is_empty()
    );
    let text = document.children(root)[0];
    assert!(exclusive(&document, text, None, &[]).unwrap().is_empty());
}

/// C14N 1.0 section 1.1, without comments: comment nodes are dropped, so a
/// document with comments canonicalizes to the bytes xmllint gives for the
/// same document without them.
#[test]
fn drops_comments() {
    let with = "<a><!-- one --><b>x<!-- two -->y</b><!-- three --></a>";
    let without = "<a><b>xy</b></a>";
    assert_eq!(whole(with.as_bytes()), whole(without.as_bytes()));
    assert_eq!(whole(without.as_bytes()), "<a><b>xy</b></a>");
}

#[test]
fn canonicalizes_a_hundred_thousand_levels_without_recursion() {
    let xml = nested(DEEP);
    let document = document(&xml);
    let started = Instant::now();
    let canonical = exclusive(&document, document.root(), None, &[]).unwrap();
    assert!(started.elapsed() < BOUND, "{:?}", started.elapsed());
    assert_eq!(canonical, xml.as_bytes());
}

/// Exc-c14n section 3 with C14N 1.0 section 2.3: below the apex an inclusive
/// prefix is rendered only where an element's own declaration changes the
/// value its nearest output ancestor rendered.
#[test]
fn an_inclusive_prefix_redeclared_below_the_apex_renders_where_its_value_changes() {
    let xml = r#"<r xmlns:p="urn:p" xmlns="urn:d"><s xmlns:q="urn:q"><t xmlns:p="urn:p2" xmlns=""><u xmlns:p="urn:p2" xmlns:q="urn:q"/></t><v xmlns:p="urn:p"/></s></r>"#;
    assert_eq!(
        subset(xml, "s", &["p", "q", DEFAULT_PREFIX]),
        r#"<s xmlns="urn:d" xmlns:p="urn:p" xmlns:q="urn:q"><t xmlns="" xmlns:p="urn:p2"><u></u></t><v></v></s>"#
    );
    assert_eq!(
        subset(xml, "s", &[]),
        r#"<s xmlns="urn:d"><t xmlns=""><u></u></t><v></v></s>"#
    );
}

#[test]
fn a_hundred_thousand_inclusive_prefixes_over_a_hundred_thousand_elements_finish_in_bounded_time() {
    let inclusive: Vec<String> = (0..WIDE).map(|index| format!("p{index}")).collect();
    let xml = format!(r#"<r xmlns:p0="urn:p">{}</r>"#, "<a/>".repeat(WIDE));
    let document = document(&xml);
    let started = Instant::now();
    let canonical = exclusive(&document, document.root(), None, &inclusive).unwrap();
    assert!(started.elapsed() < BOUND, "{:?}", started.elapsed());
    assert_eq!(
        String::from_utf8(canonical).unwrap(),
        format!(r#"<r xmlns:p0="urn:p">{}</r>"#, "<a></a>".repeat(WIDE))
    );
}

/// Exc-c14n section 3 with C14N 1.0 section 2.2: attributes sort by
/// namespace URI then local name, even when the namespaces share a long
/// common prefix.
#[test]
fn attributes_in_namespaces_with_a_long_common_prefix_sort_in_bounded_time() {
    let stem = format!("urn:{}", "n".repeat(LONG_NAMESPACE_LENGTH));
    let mut locals: Vec<String> = (0..WIDE / 2).map(|index| format!("x{index}")).collect();
    let attributes: String = locals
        .iter()
        .map(|local| format!(r#" q:{local}="" p:{local}="""#))
        .collect();
    let xml = format!(r#"<a xmlns:p="{stem}1" xmlns:q="{stem}2"{attributes}/>"#);
    let document = document(&xml);
    let started = Instant::now();
    let canonical = exclusive(&document, document.root(), None, &[]).unwrap();
    assert!(started.elapsed() < BOUND, "{:?}", started.elapsed());
    locals.sort_unstable();
    let sorted: String = ["p", "q"]
        .iter()
        .flat_map(|prefix| {
            locals
                .iter()
                .map(move |local| format!(r#" {prefix}:{local}="""#))
        })
        .collect();
    assert_eq!(
        String::from_utf8(canonical).unwrap(),
        format!(r#"<a xmlns:p="{stem}1" xmlns:q="{stem}2"{sorted}></a>"#)
    );
}

/// Exc-c14n section 3: `r` never uses `p`, so every `p:a` child renders
/// `xmlns:p` itself. The canonical form would be gigabytes, so it is refused
/// as soon as it outgrows the document instead of being built.
#[test]
fn a_namespace_amplified_onto_every_child_is_refused_in_bounded_time() {
    let document = document(&amplified(
        EMPTY_ROOT,
        ROOT_START,
        ROOT_END,
        AMPLIFYING_CHILDREN,
    ));
    let started = Instant::now();
    assert_eq!(
        exclusive(&document, document.root(), None, &[]),
        Err(Refusal::Oversized)
    );
    assert!(started.elapsed() < BOUND, "{:?}", started.elapsed());
}

#[test]
fn amplified_output_is_kept_up_to_growth_times_the_document_size() {
    let mut kept = false;
    let mut refused = false;
    for children in 1..=BOUNDARY_CHILDREN {
        let document = document(&amplified_with(
            EMPTY_ROOT,
            ROOT_START,
            ROOT_END,
            children,
            SHORT_NAMESPACE_LENGTH,
        ));
        let full = unbounded(&document, document.root(), None, &[]);
        let bounded = exclusive(&document, document.root(), None, &[]);
        if full.len() <= GROWTH * document.size() {
            assert_eq!(bounded, Ok(full), "{children} children");
            kept = true;
        } else {
            assert_eq!(bounded, Err(Refusal::Oversized), "{children} children");
            refused = true;
        }
    }
    assert!(kept && refused);
}
