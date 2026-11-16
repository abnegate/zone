use std::str::FromStr;

use jsonwebtoken::Algorithm;

pub(crate) const ALLOWED: [Algorithm; 9] = [
    Algorithm::RS256,
    Algorithm::RS384,
    Algorithm::RS512,
    Algorithm::PS256,
    Algorithm::PS384,
    Algorithm::PS512,
    Algorithm::ES256,
    Algorithm::ES384,
    Algorithm::EdDSA,
];

pub(crate) fn parse(name: &str) -> Option<Algorithm> {
    Algorithm::from_str(name)
        .ok()
        .filter(|algorithm| ALLOWED.contains(algorithm))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_asymmetric_algorithms() {
        for name in [
            "RS256", "RS384", "RS512", "PS256", "PS384", "PS512", "ES256", "ES384", "EdDSA",
        ] {
            assert_eq!(
                parse(name),
                Some(Algorithm::from_str(name).unwrap()),
                "{name}"
            );
        }
    }

    #[test]
    fn refuses_none_hmac_and_unknown_names() {
        for name in [
            "none", "None", "HS256", "HS384", "HS512", "rs256", "ES512", "",
        ] {
            assert_eq!(parse(name), None, "{name}");
        }
    }
}
