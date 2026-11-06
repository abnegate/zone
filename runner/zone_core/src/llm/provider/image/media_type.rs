use super::signature::Signature;

pub struct MediaType {
    pub name: &'static str,
    pub extension: &'static str,
    signatures: &'static [Signature],
}

impl MediaType {
    pub const fn new(
        name: &'static str,
        extension: &'static str,
        signatures: &'static [Signature],
    ) -> Self {
        Self {
            name,
            extension,
            signatures,
        }
    }

    pub fn holds(&self, bytes: &[u8]) -> bool {
        self.signatures
            .iter()
            .any(|signature| signature.starts(bytes))
    }
}
