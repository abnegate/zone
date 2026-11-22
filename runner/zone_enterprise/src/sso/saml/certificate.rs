use std::fmt::{self, Write};

use aws_lc_rs::digest::{SHA256, digest};
use aws_lc_rs::rsa::PublicEncryptingKey;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rustls_pki_types::CertificateDer;
use webpki::EndEntityCert;

use crate::sso::error::Error;

const MINIMUM_BITS: usize = 2048;
const SEPARATOR: char = ':';

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Certificate {
    der: Vec<u8>,
    public_key: Vec<u8>,
    fingerprint: String,
}

impl Certificate {
    pub fn parse(encoded: &str) -> Result<Self, Error> {
        let compact: String = encoded
            .chars()
            .filter(|character| !character.is_ascii_whitespace())
            .collect();
        let der = STANDARD
            .decode(compact)
            .map_err(|_| invalid("A signing certificate isn't valid base64."))?;
        let public_key = public_key(&der)?;
        let fingerprint = fingerprint(&der);
        Ok(Self {
            der,
            public_key,
            fingerprint,
        })
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    pub fn encoded(&self) -> String {
        STANDARD.encode(&self.der)
    }
}

impl fmt::Debug for Certificate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Certificate")
            .field("fingerprint", &self.fingerprint)
            .finish()
    }
}

fn public_key(der: &[u8]) -> Result<Vec<u8>, Error> {
    let certificate = CertificateDer::from(der);
    let parsed = EndEntityCert::try_from(&certificate)
        .map_err(|_| invalid("A signing certificate isn't a valid X.509 certificate."))?;
    let public_key = parsed.subject_public_key_info().as_ref().to_vec();
    let refused =
        || invalid("A signing certificate doesn't hold an RSA key of at least 2048 bits.");
    let key = PublicEncryptingKey::from_der(&public_key).map_err(|_| refused())?;
    if key.key_size_bits() < MINIMUM_BITS {
        return Err(refused());
    }
    Ok(public_key)
}

fn fingerprint(der: &[u8]) -> String {
    let hash = digest(&SHA256, der);
    let mut fingerprint = String::with_capacity(hash.as_ref().len() * 3);
    for (index, byte) in hash.as_ref().iter().enumerate() {
        if index > 0 {
            fingerprint.push(SEPARATOR);
        }
        write!(fingerprint, "{byte:02X}").expect("writing to a String never fails");
    }
    fingerprint
}

fn invalid(reason: &str) -> Error {
    Error::MetadataInvalid(reason.to_owned())
}
