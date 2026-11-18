use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

const BYTES: usize = 32;

pub(crate) fn token() -> String {
    let mut bytes = [0u8; BYTES];
    rand::fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_thirty_two_bytes_of_unpadded_base64url() {
        let token = token();
        assert_eq!(token.len(), 43);
        assert_eq!(URL_SAFE_NO_PAD.decode(&token).unwrap().len(), BYTES);
    }

    #[test]
    fn differs_every_time() {
        assert_ne!(token(), token());
    }
}
