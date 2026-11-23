use bytes::{Buf, Bytes, BytesMut};
use uuid::Uuid;

use super::clock::{SKEW, milliseconds};
use super::envelope::{Envelope, FRAMING_BYTES};
use super::kind::Kind;
use super::rejection::Rejection;
use super::sealer::{NONCE_BYTES, Sealer, TAG_BYTES};
use super::topic::Topic;

/// An envelope whose framing was read but whose body is still sealed. What
/// the framing claims is only good for deciding to drop it unopened.
#[derive(Debug)]
pub struct Unverified {
    topic: Topic,
    kind: Kind,
    origin: Uuid,
    correlation: Option<Uuid>,
    issued_at: u64,
    nonce: [u8; NONCE_BYTES],
    bytes: Bytes,
}

impl Unverified {
    pub(super) fn new(
        topic: Topic,
        kind: Kind,
        origin: Uuid,
        correlation: Option<Uuid>,
        issued_at: u64,
        nonce: [u8; NONCE_BYTES],
        bytes: Bytes,
    ) -> Self {
        Self {
            topic,
            kind,
            origin,
            correlation,
            issued_at,
            nonce,
            bytes,
        }
    }

    pub fn origin(&self) -> Uuid {
        self.origin
    }

    /// Milliseconds since the Unix epoch when the sender sealed it.
    pub fn issued_at(&self) -> u64 {
        self.issued_at
    }

    /// Whether, at `now`, it was issued within its kind's lifetime and not
    /// further in the future than replicas' clocks may drift apart.
    pub fn fresh(&self, now: u64) -> bool {
        self.issued_at <= now.saturating_add(milliseconds(SKEW))
            && now
                <= self
                    .issued_at
                    .saturating_add(milliseconds(self.kind.lifetime()))
    }

    pub fn open(self, channel: &str, sealer: &Sealer) -> Result<Envelope, Rejection> {
        let sealed = self.bytes.len() - TAG_BYTES;
        let framing = &self.bytes[..FRAMING_BYTES];
        let tag: &[u8; TAG_BYTES] = self.bytes[sealed..]
            .try_into()
            .expect("the tag is the last bytes of a decoded envelope");
        let mut body = BytesMut::from(&self.bytes[FRAMING_BYTES..sealed]);
        if !sealer.open(&self.nonce, channel, framing, &mut body, tag) {
            return Err(Rejection::Seal);
        }
        let mut body = body.freeze();
        let header_length = body.get_u32() as usize;
        if body.remaining() < header_length {
            return Err(Rejection::Truncated);
        }
        let header = body.split_to(header_length);
        Ok(Envelope::new(
            self.topic,
            self.kind,
            self.origin,
            self.correlation,
            header,
            body,
        ))
    }
}
