use bytes::Bytes;

use super::envelope::Envelope;
use super::rejection::Rejection;
use super::signer::Signer;

/// A decoded envelope whose signature has not been checked yet.
#[derive(Debug)]
pub struct Unverified {
    envelope: Envelope,
    bytes: Bytes,
    body: usize,
}

impl Unverified {
    pub(super) fn new(envelope: Envelope, bytes: Bytes, body: usize) -> Self {
        Self {
            envelope,
            bytes,
            body,
        }
    }

    /// Who the envelope claims sent it. Only good for deciding to drop it.
    pub fn claimed(&self) -> &Envelope {
        &self.envelope
    }

    pub fn verify(self, channel: &str, signer: &Signer) -> Result<Envelope, Rejection> {
        let (body, tag) = self.bytes.split_at(self.body);
        if signer.verify(channel, body, tag) {
            Ok(self.envelope)
        } else {
            Err(Rejection::Signature)
        }
    }
}
