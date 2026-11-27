use bytes::{Buf, BufMut, Bytes, BytesMut};
use serde::de::DeserializeOwned;
use uuid::Uuid;

use super::kind::Kind;
use super::malformed::Malformed;
use super::rejection::Rejection;
use super::sealer::{NONCE_BYTES, Sealer, TAG_BYTES};
use super::topic::Topic;
use super::unverified::Unverified;

const VERSION: u8 = 2;
const UUID_BYTES: usize = 16;
const ABSENT: u8 = 0;
const PRESENT: u8 = 1;
const ISSUED_AT_BYTES: usize = 8;
const LENGTH_BYTES: usize = 4;
pub(super) const FRAMING_BYTES: usize =
    1 + 1 + UUID_BYTES + 1 + UUID_BYTES + ISSUED_AT_BYTES + NONCE_BYTES;
const MINIMUM_BYTES: usize = FRAMING_BYTES + LENGTH_BYTES + TAG_BYTES;

/// One message on the bus: who sent it, the request it answers or opens, a
/// small header the receiver can read without touching the payload, and the
/// payload bytes. On the wire the header and payload are sealed, and the
/// framing around them, the time it was issued and the channel it was
/// published on are authenticated with them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    topic: Topic,
    kind: Kind,
    origin: Uuid,
    correlation: Option<Uuid>,
    header: Bytes,
    payload: Bytes,
}

impl Envelope {
    pub(super) fn new(
        topic: Topic,
        kind: Kind,
        origin: Uuid,
        correlation: Option<Uuid>,
        header: Bytes,
        payload: Bytes,
    ) -> Self {
        Self {
            topic,
            kind,
            origin,
            correlation,
            header,
            payload,
        }
    }

    pub fn topic(&self) -> Topic {
        self.topic
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The instance that published the envelope.
    pub fn origin(&self) -> Uuid {
        self.origin
    }

    /// The id a request and its reply share.
    pub fn correlation(&self) -> Option<Uuid> {
        self.correlation
    }

    pub fn header<T: DeserializeOwned>(&self) -> Result<T, Malformed> {
        Ok(serde_json::from_slice(&self.header)?)
    }

    pub fn payload<T: DeserializeOwned>(&self) -> Result<T, Malformed> {
        Ok(serde_json::from_slice(&self.payload)?)
    }

    /// The payload exactly as it was published.
    pub fn bytes(&self) -> &Bytes {
        &self.payload
    }

    /// The envelope sealed for `channel`, stamped as issued at `issued_at`
    /// milliseconds since the Unix epoch.
    pub(super) fn encode(&self, channel: &str, sealer: &Sealer, issued_at: u64) -> Bytes {
        let nonce = Sealer::nonce();
        let mut buffer =
            BytesMut::with_capacity(MINIMUM_BYTES + self.header.len() + self.payload.len());
        buffer.put_u8(VERSION);
        buffer.put_u8(self.kind.as_byte());
        buffer.put_slice(self.origin.as_bytes());
        match self.correlation {
            Some(correlation) => {
                buffer.put_u8(PRESENT);
                buffer.put_slice(correlation.as_bytes());
            }
            None => {
                buffer.put_u8(ABSENT);
                buffer.put_slice(Uuid::nil().as_bytes());
            }
        }
        buffer.put_u64(issued_at);
        buffer.put_slice(&nonce);
        buffer.put_u32(u32::try_from(self.header.len()).expect("a header fits in four bytes"));
        buffer.put_slice(&self.header);
        buffer.put_slice(&self.payload);
        let (framing, body) = buffer.split_at_mut(FRAMING_BYTES);
        let tag = sealer.seal(&nonce, channel, framing, body);
        buffer.put_slice(&tag);
        buffer.freeze()
    }

    /// Reads the framing without opening the body, so an envelope the
    /// receiver would drop anyway costs no decryption.
    pub(super) fn decode(topic: Topic, bytes: Bytes) -> Result<Unverified, Rejection> {
        if bytes.len() < MINIMUM_BYTES {
            return Err(Rejection::Truncated);
        }
        let mut reader = &bytes[..FRAMING_BYTES];
        if reader.get_u8() != VERSION {
            return Err(Rejection::Version);
        }
        let kind = Kind::from_byte(reader.get_u8()).ok_or(Rejection::Kind)?;
        let origin = read_uuid(&mut reader);
        let present = reader.get_u8();
        let correlation = read_uuid(&mut reader);
        let correlation = match present {
            PRESENT => Some(correlation),
            ABSENT => None,
            _ => return Err(Rejection::Truncated),
        };
        let issued_at = reader.get_u64();
        let nonce = <[u8; NONCE_BYTES]>::try_from(reader).expect("the framing ends with the nonce");
        Ok(Unverified::new(
            topic,
            kind,
            origin,
            correlation,
            issued_at,
            nonce,
            bytes,
        ))
    }
}

fn read_uuid(reader: &mut &[u8]) -> Uuid {
    let mut id = [0u8; UUID_BYTES];
    reader.copy_to_slice(&mut id);
    Uuid::from_bytes(id)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::cluster::clock::{SKEW, milliseconds};
    use crate::secret;

    const PASSPHRASE: &str = "a-cluster-passphrase-of-at-least-32-bytes";
    const OTHER_PASSPHRASE: &str = "another-cluster-passphrase-of-32-bytes!";
    const CHANNEL: &str = "zone:0123456789abcdef:titles";
    const ISSUED_AT: u64 = 1_800_000_000_000;
    const HEADER: &str = r#"{"sequence":7}"#;
    const PAYLOAD: &str = r#"{"title":"Hello"}"#;

    fn sealer() -> Sealer {
        Sealer::derive(&secret::derive(PASSPHRASE).unwrap())
    }

    fn envelope(kind: Kind) -> Envelope {
        Envelope::new(
            Topic::Titles,
            kind,
            Uuid::new_v4(),
            Some(Uuid::new_v4()),
            Bytes::from_static(HEADER.as_bytes()),
            Bytes::from_static(PAYLOAD.as_bytes()),
        )
    }

    fn shows(bytes: &[u8], text: &str) -> bool {
        bytes
            .windows(text.len())
            .any(|window| window == text.as_bytes())
    }

    fn opened(bytes: Bytes, channel: &str, sealer: &Sealer) -> Result<Envelope, Rejection> {
        Envelope::decode(Topic::Titles, bytes)
            .and_then(|unverified| unverified.open(channel, sealer))
    }

    fn issued_after(kind: Kind, elapsed: Duration) -> bool {
        let encoded = envelope(kind).encode(CHANNEL, &sealer(), ISSUED_AT);
        Envelope::decode(Topic::Titles, encoded)
            .unwrap()
            .fresh(ISSUED_AT + milliseconds(elapsed))
    }

    fn issued_ahead_by(ahead: Duration) -> bool {
        let encoded = envelope(Kind::Request).encode(CHANNEL, &sealer(), ISSUED_AT);
        Envelope::decode(Topic::Titles, encoded)
            .unwrap()
            .fresh(ISSUED_AT - milliseconds(ahead))
    }

    #[test]
    fn an_encoded_envelope_decodes_and_opens_to_itself() {
        let sealer = sealer();
        for original in [
            envelope(Kind::Request),
            Envelope::new(
                Topic::Actions,
                Kind::Message,
                Uuid::new_v4(),
                None,
                Bytes::new(),
                Bytes::new(),
            ),
        ] {
            let encoded = original.encode(CHANNEL, &sealer, ISSUED_AT);
            let unverified = Envelope::decode(original.topic(), encoded).unwrap();

            assert_eq!(unverified.origin(), original.origin());
            assert_eq!(unverified.issued_at(), ISSUED_AT);
            assert_eq!(unverified.open(CHANNEL, &sealer).unwrap(), original);
        }
    }

    #[test]
    fn the_wire_shows_neither_the_header_nor_the_payload() {
        let encoded = envelope(Kind::Message).encode(CHANNEL, &sealer(), ISSUED_AT);

        assert!(!shows(&encoded, HEADER), "{encoded:?}");
        assert!(!shows(&encoded, PAYLOAD), "{encoded:?}");
        assert!(!shows(&encoded, "Hello"), "{encoded:?}");
    }

    #[test]
    fn the_same_envelope_seals_differently_each_time() {
        let sealer = sealer();
        let original = envelope(Kind::Message);
        let first = original.encode(CHANNEL, &sealer, ISSUED_AT);
        let second = original.encode(CHANNEL, &sealer, ISSUED_AT);
        let nonce = FRAMING_BYTES - NONCE_BYTES..FRAMING_BYTES;

        assert_ne!(first[nonce.clone()], second[nonce]);
        assert_ne!(first[FRAMING_BYTES..], second[FRAMING_BYTES..]);
        assert_eq!(opened(first, CHANNEL, &sealer).unwrap(), original);
        assert_eq!(opened(second, CHANNEL, &sealer).unwrap(), original);
    }

    #[test]
    fn a_tampered_envelope_is_rejected_wherever_the_byte_flips() {
        let sealer = sealer();
        let encoded = envelope(Kind::Request).encode(CHANNEL, &sealer, ISSUED_AT);

        for index in 0..encoded.len() {
            let mut tampered = encoded.to_vec();
            tampered[index] ^= 0x01;
            assert!(
                opened(Bytes::from(tampered), CHANNEL, &sealer).is_err(),
                "flipping byte {index} went unnoticed"
            );
        }
    }

    #[test]
    fn an_envelope_replayed_onto_another_channel_is_rejected() {
        let sealer = sealer();
        let encoded = envelope(Kind::Request).encode(CHANNEL, &sealer, ISSUED_AT);

        assert_eq!(
            opened(encoded, "zone:0123456789abcdef:actions", &sealer),
            Err(Rejection::Seal)
        );
    }

    #[test]
    fn an_envelope_sealed_under_another_master_key_is_rejected() {
        let other = Sealer::derive(&secret::derive(OTHER_PASSPHRASE).unwrap());
        let encoded = envelope(Kind::Message).encode(CHANNEL, &other, ISSUED_AT);

        assert_eq!(opened(encoded, CHANNEL, &sealer()), Err(Rejection::Seal));
    }

    #[test]
    fn a_short_or_unknown_envelope_is_rejected_before_it_is_opened() {
        let encoded = envelope(Kind::Message).encode(CHANNEL, &sealer(), ISSUED_AT);
        let mut future = encoded.to_vec();
        future[0] = VERSION + 1;

        assert_eq!(
            Envelope::decode(Topic::Titles, encoded.slice(..MINIMUM_BYTES - 1)).err(),
            Some(Rejection::Truncated)
        );
        assert_eq!(
            Envelope::decode(Topic::Titles, Bytes::from(future)).err(),
            Some(Rejection::Version)
        );
    }

    #[test]
    fn a_sealed_header_longer_than_the_body_is_truncated() {
        let sealer = sealer();
        let genuine = envelope(Kind::Message).encode(CHANNEL, &sealer, ISSUED_AT);
        let framing = &genuine[..FRAMING_BYTES];
        let nonce: [u8; NONCE_BYTES] = framing[FRAMING_BYTES - NONCE_BYTES..].try_into().unwrap();
        let mut body = u32::MAX.to_be_bytes().to_vec();
        let tag = sealer.seal(&nonce, CHANNEL, framing, &mut body);
        let forged = Bytes::from([framing, &body, &tag].concat());

        assert_eq!(opened(forged, CHANNEL, &sealer), Err(Rejection::Truncated));
    }

    #[test]
    fn a_request_is_fresh_for_its_lifetime_only() {
        let lifetime = Kind::Request.lifetime();

        assert!(issued_after(Kind::Request, Duration::ZERO));
        assert!(issued_after(Kind::Request, lifetime));
        assert!(!issued_after(
            Kind::Request,
            lifetime + Duration::from_millis(1)
        ));
    }

    #[test]
    fn messages_and_replies_stay_fresh_longer_than_requests() {
        let past_a_request = Kind::Request.lifetime() + Duration::from_secs(1);
        let lifetime = Kind::Message.lifetime();

        for kind in [Kind::Message, Kind::Reply] {
            assert!(issued_after(kind, past_a_request));
            assert!(issued_after(kind, lifetime));
            assert!(!issued_after(kind, lifetime + Duration::from_millis(1)));
        }
    }

    #[test]
    fn an_envelope_from_a_clock_ahead_is_fresh_only_within_the_skew() {
        assert!(issued_ahead_by(SKEW));
        assert!(!issued_ahead_by(SKEW + Duration::from_millis(1)));
    }
}
