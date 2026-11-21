use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::sso::saml::canonical::{escape_attribute, escape_text};
use crate::sso::saml::uri;

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
