use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use md5::Md5;
use sha2::{Digest, Sha256};

pub const SHA256_HEADER: &str = "x-amz-checksum-sha256";
pub const ALGORITHM_HEADER: &str = "x-amz-sdk-checksum-algorithm";
pub const SHA256: &str = "SHA256";
pub const MD5_HEADER: &str = "content-md5";

/// The `x-amz-checksum-sha256` value: the base64 of the body's raw SHA-256 digest.
pub fn sha256(body: &[u8]) -> String {
    STANDARD.encode(Sha256::digest(body))
}

/// The `Content-MD5` value of RFC 1864: the base64 of the body's raw MD5 digest.
pub fn md5(body: &[u8]) -> String {
    STANDARD.encode(Md5::digest(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests_are_base64_of_the_raw_bytes() {
        assert_eq!(sha256(b""), "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=");
        assert_eq!(md5(b""), "1B2M2Y8AsgTpgAmY7PhCfg==");
        assert_eq!(
            sha256(b"hello world"),
            "uU0nuZNNPgilLlLX2n2r+sSE7+N6U4DukIj3rOLvzek="
        );
        assert_eq!(md5(b"hello world"), "XrY7u+Ae7tCTyyK7j1rNww==");
    }
}
