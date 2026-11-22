use bytes::{Buf, BufMut, Bytes, BytesMut};
use serde::de::DeserializeOwned;
use uuid::Uuid;

use super::kind::Kind;
use super::rejection::Rejection;
use super::signer::{Signer, TAG_BYTES};
use super::topic::Topic;
use super::unverified::Unverified;

const VERSION: u8 = 1;
const UUID_BYTES: usize = 16;
const ABSENT: u8 = 0;
const PRESENT: u8 = 1;
const FIXED_BYTES: usize = 1 + 1 + UUID_BYTES + 1 + UUID_BYTES + 4;

/// One message on the bus: who sent it, the request it answers or opens, a
/// small header the receiver can read without touching the payload, and the
/// payload bytes. On the wire it carries an HMAC over all of them and the
/// channel it was published on.
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

    pub fn header<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.header)
    }

    pub fn payload<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.payload)
    }

    /// The payload exactly as it was published.
    pub fn bytes(&self) -> &Bytes {
        &self.payload
    }

    pub(super) fn encode(&self, channel: &str, signer: &Signer) -> Bytes {
        let mut buffer = BytesMut::with_capacity(
            FIXED_BYTES + self.header.len() + self.payload.len() + TAG_BYTES,
        );
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
        buffer.put_u32(u32::try_from(self.header.len()).expect("a header fits in four bytes"));
        buffer.put_slice(&self.header);
        buffer.put_slice(&self.payload);
        let tag = signer.sign(channel, &buffer);
        buffer.put_slice(&tag);
        buffer.freeze()
    }

    /// Reads the framing without checking the signature, so an envelope the
    /// receiver would drop anyway costs no HMAC.
    pub(super) fn decode(topic: Topic, bytes: Bytes) -> Result<Unverified, Rejection> {
        if bytes.len() < FIXED_BYTES + TAG_BYTES {
            return Err(Rejection::Truncated);
        }
        let body = bytes.len() - TAG_BYTES;
        let mut reader = bytes.slice(..body);
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
        let header_length = reader.get_u32() as usize;
        if reader.remaining() < header_length {
            return Err(Rejection::Truncated);
        }
        let header = reader.split_to(header_length);
        let envelope = Self::new(topic, kind, origin, correlation, header, reader);
        Ok(Unverified::new(envelope, bytes, body))
    }
}

fn read_uuid(reader: &mut Bytes) -> Uuid {
    let mut id = [0u8; UUID_BYTES];
    reader.copy_to_slice(&mut id);
    Uuid::from_bytes(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret;

    const PASSPHRASE: &str = "a-cluster-passphrase-of-at-least-32-bytes";
    const CHANNEL: &str = "zone:0123456789abcdef:titles";

    fn signer() -> Signer {
        Signer::derive(&secret::derive(PASSPHRASE).unwrap())
    }

    fn envelope() -> Envelope {
        Envelope::new(
            Topic::Titles,
            Kind::Request,
            Uuid::new_v4(),
            Some(Uuid::new_v4()),
            Bytes::from_static(br#"{"sequence":7}"#),
            Bytes::from_static(br#"{"title":"Hello"}"#),
        )
    }

    #[test]
    fn an_encoded_envelope_decodes_and_verifies_to_itself() {
        let signer = signer();
        for original in [
            envelope(),
            Envelope::new(
                Topic::Actions,
                Kind::Message,
                Uuid::new_v4(),
                None,
                Bytes::new(),
                Bytes::new(),
            ),
        ] {
            let encoded = original.encode(CHANNEL, &signer);
            let decoded = Envelope::decode(original.topic(), encoded)
                .unwrap()
                .verify(CHANNEL, &signer)
                .unwrap();

            assert_eq!(decoded, original);
        }
    }

    #[test]
    fn a_tampered_envelope_is_rejected_wherever_the_byte_flips() {
        let signer = signer();
        let encoded = envelope().encode(CHANNEL, &signer);

        for index in 0..encoded.len() {
            let mut tampered = encoded.to_vec();
            tampered[index] ^= 0x01;
            let verified = Envelope::decode(Topic::Titles, Bytes::from(tampered))
                .and_then(|unverified| unverified.verify(CHANNEL, &signer));
            assert!(verified.is_err(), "flipping byte {index} went unnoticed");
        }
    }

    #[test]
    fn an_envelope_replayed_onto_another_channel_is_rejected() {
        let signer = signer();
        let encoded = envelope().encode(CHANNEL, &signer);

        let replayed = Envelope::decode(Topic::Actions, encoded)
            .unwrap()
            .verify("zone:0123456789abcdef:actions", &signer);

        assert_eq!(replayed, Err(Rejection::Signature));
    }

    #[test]
    fn a_short_or_unknown_envelope_is_rejected_before_its_signature() {
        let signer = signer();
        let encoded = envelope().encode(CHANNEL, &signer);
        let mut future = encoded.to_vec();
        future[0] = VERSION + 1;

        assert_eq!(
            Envelope::decode(Topic::Titles, encoded.slice(..FIXED_BYTES)).err(),
            Some(Rejection::Truncated)
        );
        assert_eq!(
            Envelope::decode(Topic::Titles, Bytes::from(future)).err(),
            Some(Rejection::Version)
        );
    }

    #[test]
    fn a_header_longer_than_the_body_is_truncated() {
        let signer = signer();
        let mut encoded = envelope().encode(CHANNEL, &signer).to_vec();
        let length_offset = FIXED_BYTES - 4;
        encoded[length_offset..FIXED_BYTES].copy_from_slice(&u32::MAX.to_be_bytes());

        assert_eq!(
            Envelope::decode(Topic::Titles, Bytes::from(encoded)).err(),
            Some(Rejection::Truncated)
        );
    }
}
