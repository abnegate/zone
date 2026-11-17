#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum UrlReason {
    #[error("The URL must be absolute.")]
    Unparseable,
    #[error("The URL must use http or https.")]
    Scheme,
    #[error("The URL must include a host.")]
    Host,
    #[error("The URL must not include a username or password.")]
    Userinfo,
    #[error("The URL must not include a query.")]
    Query,
    #[error("The URL must not include a fragment.")]
    Fragment,
    #[error("The URL must not point at a link-local or cloud metadata address.")]
    Metadata,
    #[error("The URL's host is not one this instance allows endpoints on.")]
    Unlisted,
}

impl UrlReason {
    pub const ALL: [Self; 8] = [
        Self::Unparseable,
        Self::Scheme,
        Self::Host,
        Self::Userinfo,
        Self::Query,
        Self::Fragment,
        Self::Metadata,
        Self::Unlisted,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reason_is_one_sentence_naming_the_url() {
        for reason in UrlReason::ALL {
            let message = reason.to_string();
            assert!(message.starts_with("The URL"), "{message}");
            assert!(
                message.ends_with('.') && !message.contains('\n'),
                "{message}"
            );
        }
    }
}
