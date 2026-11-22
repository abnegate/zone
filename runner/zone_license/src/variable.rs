#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Variable {
    PrivateKeyFile,
    PrivateKey,
    KeyId,
    WebhookSecret,
    Products,
    AccessToken,
    Listen,
    SmtpHost,
    SmtpPort,
    SmtpUser,
    SmtpPassword,
    SmtpFrom,
    SmtpFromName,
    BundleBucket,
    BundleRegion,
    BundleEndpoint,
    BundlePrefix,
    BundleAccessKeyId,
    BundleSecretAccessKey,
}

impl Variable {
    #[cfg(test)]
    pub const ALL: [Self; 19] = [
        Self::PrivateKeyFile,
        Self::PrivateKey,
        Self::KeyId,
        Self::WebhookSecret,
        Self::Products,
        Self::AccessToken,
        Self::Listen,
        Self::SmtpHost,
        Self::SmtpPort,
        Self::SmtpUser,
        Self::SmtpPassword,
        Self::SmtpFrom,
        Self::SmtpFromName,
        Self::BundleBucket,
        Self::BundleRegion,
        Self::BundleEndpoint,
        Self::BundlePrefix,
        Self::BundleAccessKeyId,
        Self::BundleSecretAccessKey,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::PrivateKeyFile => "ZONE_LICENSE_PRIVATE_KEY_FILE",
            Self::PrivateKey => "ZONE_LICENSE_PRIVATE_KEY",
            Self::KeyId => "ZONE_LICENSE_KEY_ID",
            Self::WebhookSecret => "POLAR_WEBHOOK_SECRET",
            Self::Products => "POLAR_PRODUCT_IDS",
            Self::AccessToken => "POLAR_ACCESS_TOKEN",
            Self::Listen => "ZONE_LICENSE_LISTEN",
            Self::SmtpHost => "SMTP_HOST",
            Self::SmtpPort => "SMTP_PORT",
            Self::SmtpUser => "SMTP_USER",
            Self::SmtpPassword => "SMTP_PASSWORD",
            Self::SmtpFrom => "SMTP_FROM",
            Self::SmtpFromName => "SMTP_FROM_NAME",
            Self::BundleBucket => "ZONE_BUNDLE_BUCKET",
            Self::BundleRegion => "ZONE_BUNDLE_REGION",
            Self::BundleEndpoint => "ZONE_BUNDLE_ENDPOINT",
            Self::BundlePrefix => "ZONE_BUNDLE_PREFIX",
            Self::BundleAccessKeyId => "ZONE_BUNDLE_ACCESS_KEY_ID",
            Self::BundleSecretAccessKey => "ZONE_BUNDLE_SECRET_ACCESS_KEY",
        }
    }

    /// Blank values count as unset, so an empty export cannot shadow the other key source.
    pub fn read(self) -> Option<String> {
        self.exact().filter(|value| !value.trim().is_empty())
    }

    /// The value exactly as set, blank included, for `SMTP_PASSWORD`, which a
    /// relay may accept empty.
    pub fn exact(self) -> Option<String> {
        std::env::var(self.name()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variable_keeps_its_documented_name() {
        assert_eq!(
            Variable::ALL.map(Variable::name),
            [
                "ZONE_LICENSE_PRIVATE_KEY_FILE",
                "ZONE_LICENSE_PRIVATE_KEY",
                "ZONE_LICENSE_KEY_ID",
                "POLAR_WEBHOOK_SECRET",
                "POLAR_PRODUCT_IDS",
                "POLAR_ACCESS_TOKEN",
                "ZONE_LICENSE_LISTEN",
                "SMTP_HOST",
                "SMTP_PORT",
                "SMTP_USER",
                "SMTP_PASSWORD",
                "SMTP_FROM",
                "SMTP_FROM_NAME",
                "ZONE_BUNDLE_BUCKET",
                "ZONE_BUNDLE_REGION",
                "ZONE_BUNDLE_ENDPOINT",
                "ZONE_BUNDLE_PREFIX",
                "ZONE_BUNDLE_ACCESS_KEY_ID",
                "ZONE_BUNDLE_SECRET_ACCESS_KEY",
            ]
        );
    }
}
