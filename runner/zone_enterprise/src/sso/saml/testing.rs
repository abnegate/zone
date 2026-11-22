mod claim;
mod key;
mod options;
mod signing;

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{
    RSA_PKCS1_SHA256, RSA_PKCS1_SHA384, RSA_PKCS1_SHA512, RsaEncoding, RsaKeyPair,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, SecondsFormat, Utc};

use crate::sso::saml::canonical::{self, DEFAULT_PREFIX, escape_attribute, escape_text};
use crate::sso::saml::certificate::Certificate;
use crate::sso::saml::digest_method::DigestMethod;
use crate::sso::saml::document::Document;
use crate::sso::saml::identifiers;
use crate::sso::saml::node::Node;
use crate::sso::saml::node_id::NodeId;
use crate::sso::saml::signature::SIGNATURE_VALUE;
use crate::sso::saml::signature_method::SignatureMethod;
use crate::sso::saml::uri;

pub use claim::Claim;
pub use key::Key;
pub use options::Options;
pub use signing::Signing;

pub const ENTITY_ID: &str = "https://idp.example.com/metadata";
pub const SSO_URL: &str = "https://idp.example.com/sso";
pub const SERVICE_ENTITY_ID: &str = "https://zone.example.com/api/auth/sso/saml/metadata";
pub const ACS_URL: &str = "https://zone.example.com/api/auth/sso/saml/acs";
pub const REQUEST_ID: &str = "_request-7f3c9a";
pub const RESPONSE_ID: &str = "_response-5b21d4";
pub const ASSERTION_ID: &str = "_assertion-9e84c1";
pub const SCHEMA_PREFIX: &str = "xs";

const VERSION: &str = "2.0";
const ISSUER: &str = "Issuer";
const BASIC: &str = "urn:oasis:names:tc:SAML:2.0:attrname-format:basic";
const PASSWORD_PROTECTED_TRANSPORT: &str =
    "urn:oasis:names:tc:SAML:2.0:ac:classes:PasswordProtectedTransport";
const PLACEHOLDER: &str = "AA==";

pub const CERTIFICATE: &str = "MIIC9DCCAdygAwIBAgIUVnF14cF3LqJvPpec5D2ZXJg6gikwDQYJKoZIhvcNAQELBQAwGTEXMBUGA1UEAwwOem9uZS1zYW1sLXRlc3QwIBcNMjYxMTIxMDk1NTI3WhgPMjEyNjEwMjgwOTU1MjdaMBkxFzAVBgNVBAMMDnpvbmUtc2FtbC10ZXN0MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAnupMGbVq5q/Uj/SpnnHFLLlN891yK06s0OsVemQHGy5pLogTgcvM/X35MXXrm4vGTwpGmsOKKS/vGNe0VmVb37Z+Z9+TQXFiGI53cwNQtkWlIDmIvfbjhgOPssaT0/JjM9TLOdwJ6Mt8KZFevK/o+blmcbIJwIemRWNLuHrqQCi/MeDFGvz3Pmn6KFPlNQgzG5eUZ1u3hdvDzzxeLdWVi/lYPBEBVB2rRy4Zp7+DxQ2gDbZmb+pyW/dbVAlvqTOFBwIRQL9Nrm7YHqUQndqO97/vw0TQgIduqExRqhQfX/qfvptO00+ce4ikMhQOE6VkBltOO4hub1YOo/H7rNCg9QIDAQABozIwMDAdBgNVHQ4EFgQU+1cqOMOmEVYRhK7wP8G2YxUUvDgwDwYDVR0TAQH/BAUwAwEB/zANBgkqhkiG9w0BAQsFAAOCAQEAP9TWhzii5lBF8uApLxxLX80PrDflG5zzjJMVBWn8z5NfgKHTgZ5a5BfXaLBY5bD4MgGhexsFwu2Ukp5DHgW6P4LUo9ULXGQzrmfmmTu5MbMsodKAPjd5OqfUnEQltdm4sj9pn1WQvNdgzfSnW27rQRPAtGIz+pWKUbvV8UPxpFOqQ+ZHKNBYM9TvS9Qjw979zbbAz5hjc0FoAhvsFsJd4Re1C1z3hWthIptGA+ApvepuYbU0eLxvknT0xZX+kqf3gI0Y4UYn0VfJaBGWXYa8P80QGz7e5it+0RQUktFyF5SHQuEHtK6FP/BwKDVMq4vzF2elnU+d/k61zata69FTnw==";
pub const OTHER_CERTIFICATE: &str = "MIIC9jCCAd6gAwIBAgIUUtOVyQTkRWJ4fxBKYp3Zc8wqj/EwDQYJKoZIhvcNAQELBQAwGjEYMBYGA1UEAwwPem9uZS1zYW1sLW90aGVyMCAXDTI2MTEyMTA5NTUyN1oYDzIxMjYxMDI4MDk1NTI3WjAaMRgwFgYDVQQDDA96b25lLXNhbWwtb3RoZXIwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQCXaPEKPA8QErdPC/Vq9t5TKiN8j/32ihHFW3fYvLsMgBd9rQPgXm+dur68/UADtCGBvkptlvoKbAu1QW2hSfe4zMrqpbMJel5nXY8+34VDtbjy9zemXALIzS28MbDbqcVBTKuht8SOJybWqrURT6EJ++eDQHQiEROoO+f4qstJb+X22YRkRyMa9VRqMYQjs46TmRS93GcrOLJrSqdk4xlk7HvdFUexid3j5rNEszu4xlJagUNdKwSyuVEiwmXBPaGIUCTPcDC2YvaRBoSfH54Gfdjv2EhORLPoy10c+Ez2YD58fxMMlqYZ1Qft4Nh9O5y1qgQTtipGp34e4Vdl+hXVAgMBAAGjMjAwMB0GA1UdDgQWBBRlWUR47h0lae1Gcg0Yo2RK+JpeTzAPBgNVHRMBAf8EBTADAQH/MA0GCSqGSIb3DQEBCwUAA4IBAQCJh4fvdU0uuf0l5XUG8xFOxM6FIM9UoAboVHV1dGi+/2uce1GNaPnSWeDIpQDKEsJUrTMzyGpZL1CPkusWucGyxkCHv0cKYqlz5ArYdJ5NW3lmABMCtjEyXePamyLvkOHA07HLl3DBaOXbYUBQQSMrGmxz5UQTW0J2KWu6+2VK0WS+PFwaDXlwZz/ikeSzOq1OjDR2gpzEQ9z6zlumK1/GOc1hjYbfG7nd4ycjbtgCaIpgOHFtz9RVETeO/FWUNx0ioXY/y2gEP/bVfukCdLBKwACwY6WcVtd0+ZaKRdL+ZsgRwcx5cG752AUsGSFsdExLjpTQMnILb3ptgBpefKuq";
pub const OTHER_PRIVATE_KEY: &str = "\
-----BEGIN PRIVATE KEY-----\n\
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQCXaPEKPA8QErdP\n\
C/Vq9t5TKiN8j/32ihHFW3fYvLsMgBd9rQPgXm+dur68/UADtCGBvkptlvoKbAu1\n\
QW2hSfe4zMrqpbMJel5nXY8+34VDtbjy9zemXALIzS28MbDbqcVBTKuht8SOJybW\n\
qrURT6EJ++eDQHQiEROoO+f4qstJb+X22YRkRyMa9VRqMYQjs46TmRS93GcrOLJr\n\
Sqdk4xlk7HvdFUexid3j5rNEszu4xlJagUNdKwSyuVEiwmXBPaGIUCTPcDC2YvaR\n\
BoSfH54Gfdjv2EhORLPoy10c+Ez2YD58fxMMlqYZ1Qft4Nh9O5y1qgQTtipGp34e\n\
4Vdl+hXVAgMBAAECggEAGODg7lYVAYaZwS7NWhCLUm3zm8WLLMXeCqgvZpkIQRMR\n\
CRDzOJw7ydaD/ci9Cn7AHLH4R86NckXznIpOKbjeL+rSksC8TNvjBCf+4A1ZN9l1\n\
tNKDPd4ThQwVrj0Z+tYH9+VRtEZeUR9ExfQRwXDsnJfcIBhonKpiqPuC77ycn4m3\n\
L8vESzAFVLsoMvFxWTfEY/hGIYl4HgFdL0DuR9lBNNgx3cOJ4s3CAq775CsHGcbE\n\
RmQzcGrnZGd0cCav2j2pY0KOwgHSWeslL9YF9YsUvWUiUbqb81qHzp0WeRvpZJvf\n\
TIZFuucWLkF3qFzzheTqZIevuncerBmruJWJq5J0lwKBgQDJWc3M+PNtx/e6pqI/\n\
u0WyMsvG/wQEoTUCLrSmxlzVgtcIxm7AFWXNb/qFKupbzulWQX+I7In5a0GI3TMJ\n\
F3k70qxP1hA8HEOShpkF16g0DFRTmC8szTVsKr1rJXKyAKUnzxYs4XrYESwqsb1B\n\
iCbQ2NvgWgIR7AsVBUqPEcLd/wKBgQDAgSk9j0nCQzq5CPMkiU30cVULnQaVi3cK\n\
53zTcZ3Ji3H+NZTc4C0H950Mu0z2kuqeDDq0U9Tu6R3IBQNi3qBOiF2R/fYMyRAE\n\
3aVtGjIY0Qy6A82JDRtsqJP5vGm4MDfT8uuiBNCJkjsO/vTd0Qryje+wZhXe0WWO\n\
+erKWNk0KwKBgE0jBU+4M/HjwMkV22SVBv9crGqg+dYYOCmM4RJfPeNiC6VF99nI\n\
mMaaVpw7s6rWWtf7obNAAx+1clbPfyTFhGFhtTekp3WP0mdI/0iYtMyzyH559+Cv\n\
AP9eZFCu+QCudJcKo9Ev1JMACS6WBw4a7dvmqF2Lve1Fw30bGNQ0lrXbAoGAZbPX\n\
Ve6dyUQ7ggZ4bpUb2kkF5VzQXbGQ8vHZwNwFwLUrRGKKgybso4juMAvEXYDA8z8C\n\
xMKFfLMsSMBdSZ7Ukqj4xmil1ZtkGy3+d+WfEf1f++aGcCG8m2t0wjwInoba998f\n\
Hjns8sGO8h1AwO/wGLVkLI3CgDWgsijXIpBOMIECgYEApgPgsSn4FO/7J+KgCoMk\n\
/Xa17HUVhCEKzQDLXn9KokR+hgqMuDZKTOQKMmXe5cjigK+g6ANW2vgh8re8dsmD\n\
0FXNsMPx8s1AAO8neNeHJ6B5n642Yhb/XbXil9vMazEdsTBQGxfVaQJN09xTU0ai\n\
F02uIyigQlKsP6J8ECOLCig=\n\
-----END PRIVATE KEY-----\n\
";

const PEM_BOUNDARY: &str = "-----";

pub fn pkcs8(pem: &str) -> Vec<u8> {
    let body: String = pem
        .lines()
        .filter(|line| !line.starts_with(PEM_BOUNDARY))
        .collect();
    STANDARD
        .decode(body)
        .expect("a test private key is base64 PEM")
}

pub fn metadata(entity_id: &str, sso_url: &str, certificates: &[&str]) -> String {
    let mut keys = String::new();
    for certificate in certificates {
        keys.push_str(
            "    <md:KeyDescriptor use=\"signing\"><ds:KeyInfo><ds:X509Data><ds:X509Certificate>",
        );
        escape_text(certificate, &mut keys);
        keys.push_str("</ds:X509Certificate></ds:X509Data></ds:KeyInfo></md:KeyDescriptor>\n");
    }
    let mut entity = String::new();
    escape_attribute(entity_id, &mut entity);
    let mut location = String::new();
    escape_attribute(sso_url, &mut location);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<md:EntityDescriptor xmlns:md=\"{metadata}\" xmlns:ds=\"{dsig}\" entityID=\"{entity}\">\n\
  <md:IDPSSODescriptor WantAuthnRequestsSigned=\"false\" protocolSupportEnumeration=\"{protocol}\">\n\
{keys}\
    <md:NameIDFormat>{persistent}</md:NameIDFormat>\n\
    <md:NameIDFormat>{email}</md:NameIDFormat>\n\
    <md:SingleSignOnService Binding=\"{redirect}\" Location=\"{location}\"/>\n\
    <md:SingleSignOnService Binding=\"{post}\" Location=\"{location}\"/>\n\
  </md:IDPSSODescriptor>\n\
</md:EntityDescriptor>\n",
        metadata = uri::METADATA,
        dsig = uri::DSIG,
        protocol = uri::PROTOCOL,
        persistent = uri::PERSISTENT,
        email = uri::EMAIL_ADDRESS,
        redirect = uri::HTTP_REDIRECT,
        post = uri::HTTP_POST,
    )
}

pub fn public_key(certificate: &str) -> Vec<u8> {
    Certificate::parse(certificate)
        .expect("a test certificate parses")
        .public_key()
        .to_vec()
}

pub fn response(options: &Options) -> String {
    STANDARD.encode(response_xml(options))
}

pub fn response_xml(options: &Options) -> String {
    let signing = Signing {
        key: options.signing_key,
        reference_prefixes: Some(vec![SCHEMA_PREFIX.to_owned()]),
        ..Signing::default()
    };
    let mut xml = unsigned(options);
    if options.sign_assertion {
        xml = sign_with(&xml, &options.assertion_id, &signing);
    }
    if options.sign_response {
        xml = sign_with(&xml, &options.response_id, &signing);
    }
    xml
}

pub fn sign(xml: &str, element_id: &str, key: Key) -> String {
    sign_with(
        xml,
        element_id,
        &Signing {
            key,
            ..Signing::default()
        },
    )
}

pub fn sign_with(xml: &str, element_id: &str, signing: &Signing) -> String {
    let mut document = Document::parse(xml.as_bytes()).expect("the XML to sign parses");
    let element = *identifiers::index(&document)
        .expect("the XML to sign has unique IDs")
        .get(element_id)
        .expect("the XML to sign has the element");
    let reference_prefixes = signing.reference_prefixes.clone().unwrap_or_default();
    let content = canonical::unbounded(&document, element, None, &reference_prefixes);
    let digest_method = DigestMethod::parse(signing.digest_method).unwrap_or(DigestMethod::Sha256);
    let digest = STANDARD.encode(digest_method.digest(&content));
    let fragment = Document::parse(signature_xml(element_id, signing, &digest).as_bytes())
        .expect("the test signature parses");
    let signature = splice(&mut document, element, fragment);
    let signed_info = document
        .child_elements(signature)
        .next()
        .expect("the test signature has SignedInfo");
    let signed_info_prefixes = signing.signed_info_prefixes.clone().unwrap_or_default();
    let message = canonical::unbounded(&document, signed_info, None, &signed_info_prefixes);
    let value = document
        .children_named(signature, uri::DSIG, SIGNATURE_VALUE)
        .next()
        .and_then(|value| document.children(value).first().copied())
        .expect("the test signature has a SignatureValue");
    document.nodes[value.0] =
        Node::Text(STANDARD.encode(signed(signing.key, signing.method, &message)));
    serialize(&document)
}

fn signed(key: Key, method: &str, message: &[u8]) -> Vec<u8> {
    let pair = RsaKeyPair::from_pkcs8(&pkcs8(key.private_key())).expect("a test key is PKCS#8");
    let encoding: &'static dyn RsaEncoding = match SignatureMethod::parse(method) {
        Ok(SignatureMethod::RsaSha384) => &RSA_PKCS1_SHA384,
        Ok(SignatureMethod::RsaSha512) => &RSA_PKCS1_SHA512,
        Ok(SignatureMethod::RsaSha256) | Err(_) => &RSA_PKCS1_SHA256,
    };
    let mut signature = vec![0; pair.public_modulus_len()];
    pair.sign(encoding, &SystemRandom::new(), message, &mut signature)
        .expect("a test key signs");
    signature
}

fn signature_xml(element_id: &str, signing: &Signing, digest: &str) -> String {
    let target = signing
        .reference
        .clone()
        .unwrap_or_else(|| format!("#{element_id}"));
    let mut transforms = String::new();
    for algorithm in &signing.transforms {
        let inclusive = if *algorithm == uri::EXCLUSIVE_CANONICALIZATION {
            inclusive_namespaces(signing.reference_prefixes.as_deref())
        } else {
            String::new()
        };
        transforms.push_str(&format!(
            "<ds:Transform Algorithm=\"{}\">{inclusive}</ds:Transform>",
            attribute(algorithm)
        ));
    }
    let reference = format!(
        "<ds:Reference URI=\"{}\"><ds:Transforms>{transforms}</ds:Transforms>\
<ds:DigestMethod Algorithm=\"{}\"/><ds:DigestValue>{digest}</ds:DigestValue></ds:Reference>",
        attribute(&target),
        attribute(signing.digest_method),
    );
    let key_info = if signing.key_info {
        format!(
            "<ds:KeyInfo><ds:X509Data><ds:X509Certificate>{}</ds:X509Certificate>\
</ds:X509Data></ds:KeyInfo>",
            signing.key.certificate()
        )
    } else {
        String::new()
    };
    format!(
        "<ds:Signature xmlns:ds=\"{dsig}\"><ds:SignedInfo>\
<ds:CanonicalizationMethod Algorithm=\"{canonicalization}\">{inclusive}</ds:CanonicalizationMethod>\
<ds:SignatureMethod Algorithm=\"{method}\"/>{references}</ds:SignedInfo>\
<ds:SignatureValue>{PLACEHOLDER}</ds:SignatureValue>{key_info}</ds:Signature>",
        dsig = uri::DSIG,
        canonicalization = attribute(signing.canonicalization),
        inclusive = inclusive_namespaces(signing.signed_info_prefixes.as_deref()),
        method = attribute(signing.method),
        references = reference.repeat(signing.references),
    )
}

fn inclusive_namespaces(prefixes: Option<&[String]>) -> String {
    prefixes.map_or_else(String::new, |prefixes| {
        format!(
            "<ec:InclusiveNamespaces xmlns:ec=\"{}\" PrefixList=\"{}\"/>",
            uri::EXCLUSIVE_CANONICALIZATION,
            attribute(&prefixes.join(" "))
        )
    })
}

fn splice(document: &mut Document, parent: NodeId, fragment: Document) -> NodeId {
    let offset = document.nodes.len();
    let shift = |id: NodeId| NodeId(id.0 + offset);
    for (node, owner) in fragment.nodes.into_iter().zip(fragment.parents) {
        let node = match node {
            Node::Element(mut element) => {
                element.children = element.children.into_iter().map(shift).collect();
                Node::Element(element)
            }
            other => other,
        };
        document.nodes.push(node);
        document.parents.push(owner.map(shift));
    }
    let root = shift(fragment.root);
    document.parents[root.0] = Some(parent);
    let position = document
        .children(parent)
        .iter()
        .position(|child| document.is(*child, uri::ASSERTION, ISSUER))
        .map_or(0, |issuer| issuer + 1);
    if let Some(Node::Element(element)) = document.nodes.get_mut(parent.0) {
        let mut children = std::mem::take(&mut element.children).into_vec();
        children.insert(position, root);
        element.children = children.into_boxed_slice();
    }
    root
}

fn serialize(document: &Document) -> String {
    let mut prefixes: Vec<String> = document
        .nodes
        .iter()
        .filter_map(|node| match node {
            Node::Element(element) => Some(element.declarations()),
            _ => None,
        })
        .flatten()
        .map(|declaration| declaration.prefix().unwrap_or(DEFAULT_PREFIX).to_owned())
        .collect();
    prefixes.sort_unstable();
    prefixes.dedup();
    String::from_utf8(canonical::unbounded(
        document,
        document.root(),
        None,
        &prefixes,
    ))
    .expect("canonical XML is UTF-8")
}

fn unsigned(options: &Options) -> String {
    let mut statement = String::new();
    if let Some(email) = &options.email {
        statement.push_str(&attribute_element(uri::EMAIL_ATTRIBUTES[0], &[email]));
    }
    if let Some(name) = &options.display_name {
        statement.push_str(&attribute_element(uri::NAME_ATTRIBUTES[0], &[name]));
    }
    for claim in &options.claims {
        statement.push_str(&attribute_element(&claim.name, &claim.values));
    }
    if !statement.is_empty() {
        statement =
            format!("    <saml2:AttributeStatement>\n{statement}    </saml2:AttributeStatement>\n");
    }
    let mut audiences = String::new();
    for audience in &options.audiences {
        audiences.push_str(&format!(
            "        <saml2:Audience>{}</saml2:Audience>\n",
            text(audience)
        ));
    }
    if !audiences.is_empty() {
        audiences = format!(
            "      <saml2:AudienceRestriction>\n{audiences}      </saml2:AudienceRestriction>\n"
        );
    }
    let issue_instant = instant(options.issue_instant);
    let issuer = text(&options.issuer);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<saml2p:Response xmlns:saml2p=\"{protocol}\" xmlns:saml2=\"{assertion}\" xmlns:xs=\"{schema}\" \
xmlns:xsi=\"{instance}\" ID=\"{response_id}\" Version=\"{VERSION}\" IssueInstant=\"{issue_instant}\"\
{destination}{in_response_to}>\n  \
  <saml2:Issuer>{issuer}</saml2:Issuer>\n  \
  <saml2p:Status>\n    \
    <saml2p:StatusCode Value=\"{status}\"/>\n  \
  </saml2p:Status>\n  \
  <saml2:Assertion ID=\"{assertion_id}\" Version=\"{VERSION}\" IssueInstant=\"{issue_instant}\">\n    \
    <saml2:Issuer>{issuer}</saml2:Issuer>\n    \
    <saml2:Subject>\n      \
      <saml2:NameID Format=\"{name_id_format}\">{name_id}</saml2:NameID>\n      \
      <saml2:SubjectConfirmation Method=\"{method}\">\n        \
        <saml2:SubjectConfirmationData{confirmation_in_response_to}{confirmation_not_before} \
NotOnOrAfter=\"{confirmation}\" Recipient=\"{recipient}\"/>\n      \
      </saml2:SubjectConfirmation>\n    \
    </saml2:Subject>\n    \
    <saml2:Conditions NotBefore=\"{not_before}\" NotOnOrAfter=\"{not_on_or_after}\">\n\
{audiences}    \
    </saml2:Conditions>\n    \
    <saml2:AuthnStatement AuthnInstant=\"{issue_instant}\" SessionIndex=\"{assertion_id}\">\n      \
      <saml2:AuthnContext>\n        \
        <saml2:AuthnContextClassRef>{PASSWORD_PROTECTED_TRANSPORT}</saml2:AuthnContextClassRef>\n      \
      </saml2:AuthnContext>\n    \
    </saml2:AuthnStatement>\n\
{statement}  \
  </saml2:Assertion>\n\
</saml2p:Response>\n",
        protocol = uri::PROTOCOL,
        assertion = uri::ASSERTION,
        schema = uri::XML_SCHEMA,
        instance = uri::XML_SCHEMA_INSTANCE,
        response_id = attribute(&options.response_id),
        destination = optional("Destination", options.destination.as_deref()),
        in_response_to = optional("InResponseTo", options.in_response_to.as_deref()),
        status = attribute(&options.status),
        assertion_id = attribute(&options.assertion_id),
        name_id_format = attribute(&options.name_id_format),
        name_id = text(&options.name_id),
        method = attribute(&options.confirmation_method),
        confirmation_in_response_to = optional(
            "InResponseTo",
            options.confirmation_in_response_to.as_deref()
        ),
        confirmation_not_before = optional(
            "NotBefore",
            options.confirmation_not_before.map(instant).as_deref()
        ),
        confirmation = instant(options.confirmation_not_on_or_after),
        recipient = attribute(&options.recipient),
        not_before = instant(options.not_before),
        not_on_or_after = instant(options.not_on_or_after),
    )
}

fn attribute_element<S: AsRef<str>>(name: &str, values: &[S]) -> String {
    let mut elements = String::new();
    for value in values {
        elements.push_str(&format!(
            "        <saml2:AttributeValue xsi:type=\"{SCHEMA_PREFIX}:string\">{}</saml2:AttributeValue>\n",
            text(value.as_ref())
        ));
    }
    format!(
        "      <saml2:Attribute Name=\"{name}\" NameFormat=\"{BASIC}\">\n{elements}      </saml2:Attribute>\n",
        name = attribute(name),
    )
}

fn optional(name: &str, value: Option<&str>) -> String {
    value.map_or_else(String::new, |value| {
        format!(" {name}=\"{}\"", attribute(value))
    })
}

fn instant(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn text(value: &str) -> String {
    let mut escaped = String::new();
    escape_text(value, &mut escaped);
    escaped
}

fn attribute(value: &str) -> String {
    let mut escaped = String::new();
    escape_attribute(value, &mut escaped);
    escaped
}
