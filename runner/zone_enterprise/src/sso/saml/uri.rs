pub const PROTOCOL: &str = "urn:oasis:names:tc:SAML:2.0:protocol";
pub const ASSERTION: &str = "urn:oasis:names:tc:SAML:2.0:assertion";
pub const METADATA: &str = "urn:oasis:names:tc:SAML:2.0:metadata";
pub const DSIG: &str = "http://www.w3.org/2000/09/xmldsig#";
pub const XML: &str = "http://www.w3.org/XML/1998/namespace";
pub const XMLNS: &str = "http://www.w3.org/2000/xmlns/";
pub const XML_SCHEMA: &str = "http://www.w3.org/2001/XMLSchema";
pub const XML_SCHEMA_INSTANCE: &str = "http://www.w3.org/2001/XMLSchema-instance";

pub const EXCLUSIVE_CANONICALIZATION: &str = "http://www.w3.org/2001/10/xml-exc-c14n#";
pub const EXCLUSIVE_CANONICALIZATION_WITH_COMMENTS: &str =
    "http://www.w3.org/2001/10/xml-exc-c14n#WithComments";
pub const INCLUSIVE_CANONICALIZATION: &str = "http://www.w3.org/TR/2001/REC-xml-c14n-20010315";
pub const ENVELOPED_SIGNATURE: &str = "http://www.w3.org/2000/09/xmldsig#enveloped-signature";

pub const RSA_SHA256: &str = "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256";
pub const RSA_SHA384: &str = "http://www.w3.org/2001/04/xmldsig-more#rsa-sha384";
pub const RSA_SHA512: &str = "http://www.w3.org/2001/04/xmldsig-more#rsa-sha512";
pub const RSA_SHA1: &str = "http://www.w3.org/2000/09/xmldsig#rsa-sha1";
pub const SHA256: &str = "http://www.w3.org/2001/04/xmlenc#sha256";
pub const SHA384: &str = "http://www.w3.org/2001/04/xmldsig-more#sha384";
pub const SHA512: &str = "http://www.w3.org/2001/04/xmlenc#sha512";
pub const SHA1: &str = "http://www.w3.org/2000/09/xmldsig#sha1";

pub const HTTP_REDIRECT: &str = "urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect";
pub const HTTP_POST: &str = "urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST";

pub const SUCCESS: &str = "urn:oasis:names:tc:SAML:2.0:status:Success";

pub const TRANSIENT: &str = "urn:oasis:names:tc:SAML:2.0:nameid-format:transient";
pub const PERSISTENT: &str = "urn:oasis:names:tc:SAML:2.0:nameid-format:persistent";
pub const EMAIL_ADDRESS: &str = "urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress";

pub const BEARER: &str = "urn:oasis:names:tc:SAML:2.0:cm:bearer";

pub const EMAIL_ATTRIBUTES: [&str; 4] = [
    "email",
    "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/emailaddress",
    "urn:oid:0.9.2342.19200300.100.1.3",
    "mail",
];
pub const NAME_ATTRIBUTES: [&str; 4] = [
    "displayName",
    "http://schemas.microsoft.com/identity/claims/displayname",
    "urn:oid:2.16.840.1.113730.3.1.241",
    "name",
];
pub const GIVEN_NAME_ATTRIBUTES: [&str; 4] = [
    "firstName",
    "givenName",
    "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/givenname",
    "urn:oid:2.5.4.42",
];
pub const SURNAME_ATTRIBUTES: [&str; 5] = [
    "lastName",
    "surname",
    "sn",
    "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/surname",
    "urn:oid:2.5.4.4",
];
