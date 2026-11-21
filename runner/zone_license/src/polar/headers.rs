use super::signature_error::SignatureError;

pub const ID_HEADER: &str = "webhook-id";
pub const TIMESTAMP_HEADER: &str = "webhook-timestamp";
pub const SIGNATURE_HEADER: &str = "webhook-signature";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers {
    pub id: Option<String>,
    pub timestamp: Option<String>,
    pub signature: Option<String>,
}

impl Headers {
    pub fn read<'a>(lookup: impl Fn(&'static str) -> Option<&'a str>) -> Self {
        Self {
            id: lookup(ID_HEADER).map(str::to_owned),
            timestamp: lookup(TIMESTAMP_HEADER).map(str::to_owned),
            signature: lookup(SIGNATURE_HEADER).map(str::to_owned),
        }
    }

    pub(crate) fn id(&self) -> Result<&str, SignatureError> {
        present(self.id.as_deref(), ID_HEADER)
    }

    pub(crate) fn timestamp(&self) -> Result<i64, SignatureError> {
        present(self.timestamp.as_deref(), TIMESTAMP_HEADER)?
            .parse()
            .map_err(|_| SignatureError::Timestamp)
    }

    pub(crate) fn signature(&self) -> Result<&str, SignatureError> {
        present(self.signature.as_deref(), SIGNATURE_HEADER)
    }
}

fn present<'a>(value: Option<&'a str>, name: &'static str) -> Result<&'a str, SignatureError> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(SignatureError::Missing(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete() -> Headers {
        Headers {
            id: Some("msg_1".to_owned()),
            timestamp: Some("1700000000".to_owned()),
            signature: Some("v1,abc".to_owned()),
        }
    }

    #[test]
    fn reads_each_header_by_its_standard_webhooks_name() {
        let headers = Headers::read(|name| match name {
            ID_HEADER => Some("msg_1"),
            TIMESTAMP_HEADER => Some("1700000000"),
            SIGNATURE_HEADER => Some("v1,abc"),
            _ => None,
        });
        assert_eq!(headers, complete());
    }

    #[test]
    fn blank_header_counts_as_missing() {
        let headers = Headers {
            id: Some("  ".to_owned()),
            ..complete()
        };
        assert_eq!(headers.id(), Err(SignatureError::Missing(ID_HEADER)));
    }

    #[test]
    fn timestamp_must_be_an_integer() {
        let headers = Headers {
            timestamp: Some("1700000000.5".to_owned()),
            ..complete()
        };
        assert_eq!(headers.timestamp(), Err(SignatureError::Timestamp));
    }

    #[test]
    fn parses_an_integer_timestamp() {
        assert_eq!(complete().timestamp(), Ok(1_700_000_000));
    }
}
