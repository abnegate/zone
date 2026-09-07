//! Time-limited signatures so a media element can load an artifact without a bearer header.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use uuid::Uuid;

/// Long enough to outlive a scrubbing session, short enough that a leaked URL stops working.
pub const LIFETIME_SECONDS: i64 = 3600;

#[derive(Clone, Copy, Debug)]
pub struct Location<'a> {
    pub workspace_id: Uuid,
    pub chat_id: Uuid,
    pub owner_id: Uuid,
    pub filename: &'a str,
}

pub fn signed_url(secret: &str, location: Location<'_>, expires: i64) -> String {
    let signature = sign(secret, location, expires);
    format!("{}?expires={expires}&signature={signature}", path(location))
}

pub fn sign(secret: &str, location: Location<'_>, expires: i64) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts a key of any length");
    mac.update(message(location, expires).as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub fn verify(
    secret: &str,
    location: Location<'_>,
    expires: i64,
    signature: &str,
    now: i64,
) -> bool {
    if expires < now {
        return false;
    }
    sign(secret, location, expires)
        .as_bytes()
        .ct_eq(signature.as_bytes())
        .into()
}

fn path(location: Location<'_>) -> String {
    format!(
        "/api/artifacts/{}/{}/{}/{}",
        location.workspace_id, location.chat_id, location.owner_id, location.filename
    )
}

fn message(location: Location<'_>, expires: i64) -> String {
    // Domain separation keeps this MAC from colliding with any other use of the secret.
    format!("zone-artifact-v1:{}:{expires}", path(location))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "test-secret-key-must-be-at-least-32-chars-long";

    fn location(filename: &str) -> Location<'_> {
        Location {
            workspace_id: Uuid::from_u128(1),
            chat_id: Uuid::from_u128(2),
            owner_id: Uuid::from_u128(3),
            filename,
        }
    }

    #[test]
    fn a_fresh_signature_verifies() {
        let signature = sign(SECRET, location("clip.webm"), 2_000);
        assert!(verify(
            SECRET,
            location("clip.webm"),
            2_000,
            &signature,
            1_000
        ));
    }

    #[test]
    fn a_signature_stops_working_once_it_expires() {
        let signature = sign(SECRET, location("clip.webm"), 2_000);
        assert!(verify(
            SECRET,
            location("clip.webm"),
            2_000,
            &signature,
            2_000
        ));
        assert!(!verify(
            SECRET,
            location("clip.webm"),
            2_000,
            &signature,
            2_001
        ));
    }

    #[test]
    fn a_signature_only_covers_the_artifact_it_was_minted_for() {
        let signature = sign(SECRET, location("clip.webm"), 2_000);
        assert!(
            !verify(SECRET, location("other.webm"), 2_000, &signature, 1_000),
            "a signature must not travel to another filename"
        );

        let elsewhere = Location {
            workspace_id: Uuid::from_u128(9),
            ..location("clip.webm")
        };
        assert!(
            !verify(SECRET, elsewhere, 2_000, &signature, 1_000),
            "a signature must not travel to another workspace"
        );
    }

    #[test]
    fn a_tampered_signature_or_deadline_is_rejected() {
        let signature = sign(SECRET, location("clip.webm"), 2_000);
        assert!(!verify(
            SECRET,
            location("clip.webm"),
            9_999,
            &signature,
            1_000
        ));
        assert!(!verify(SECRET, location("clip.webm"), 2_000, "", 1_000));
        assert!(!verify(
            SECRET,
            location("clip.webm"),
            2_000,
            &"0".repeat(64),
            1_000
        ));
        assert!(!verify(
            "another-secret-key-that-is-also-long-enough",
            location("clip.webm"),
            2_000,
            &signature,
            1_000
        ));
    }

    #[test]
    fn a_signed_url_carries_the_deadline_and_signature() {
        let url = signed_url(SECRET, location("clip.webm"), 2_000);
        let expected = sign(SECRET, location("clip.webm"), 2_000);
        assert_eq!(
            url,
            format!(
                "/api/artifacts/{}/{}/{}/clip.webm?expires=2000&signature={expected}",
                Uuid::from_u128(1),
                Uuid::from_u128(2),
                Uuid::from_u128(3)
            )
        );
    }
}
