use crate::audit::destination::SecretName;
use crate::audit::url_reason::UrlReason;

pub const DESTINATION_NOT_FOUND: &str = "audit_destination_not_found";
pub const DESTINATION_DUPLICATE: &str = "audit_destination_duplicate";
pub const NAME_INVALID: &str = "audit_destination_name_invalid";
pub const KIND_INVALID: &str = "audit_destination_kind_invalid";
pub const URL_INVALID: &str = "audit_destination_url_invalid";
pub const BUCKET_INVALID: &str = "audit_destination_bucket_invalid";
pub const REGION_INVALID: &str = "audit_destination_region_invalid";
pub const PREFIX_INVALID: &str = "audit_destination_prefix_invalid";
pub const HOST_INVALID: &str = "audit_destination_host_invalid";
pub const PORT_INVALID: &str = "audit_destination_port_invalid";
pub const FACILITY_INVALID: &str = "audit_destination_facility_invalid";
pub const HOSTNAME_INVALID: &str = "audit_destination_hostname_invalid";
pub const SECRET_INVALID: &str = "audit_destination_secret_invalid";
pub const CERTIFICATE_INVALID: &str = "audit_destination_certificate_invalid";
pub const RETENTION_INVALID: &str = "audit_retention_invalid";
pub const EXPORT_RANGE_INVALID: &str = "audit_export_range_invalid";
pub const EXPORT_FORMAT_INVALID: &str = "audit_export_format_invalid";

const NOT_FOUND: u16 = 404;
const CONFLICT: u16 = 409;
const BAD_REQUEST: u16 = 400;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("This audit destination doesn't exist.")]
    DestinationNotFound,
    #[error("An audit destination with this name already exists.")]
    DestinationDuplicate,
    #[error("Enter a destination name without control characters.")]
    NameInvalid,
    #[error(
        "Choose webhook, s3 or syslog as the destination kind, with settings for that kind. A destination's kind can't change."
    )]
    KindInvalid,
    #[error("{0}")]
    UrlInvalid(UrlReason),
    #[error(
        "\"{0}\" is not a valid bucket name. Use 3 to 63 lowercase letters, digits, dots or hyphens, starting and ending with a letter or digit."
    )]
    BucketInvalid(String),
    #[error(
        "\"{0}\" is not a valid region. Use lowercase letters, digits and hyphens, such as us-east-1."
    )]
    RegionInvalid(String),
    #[error(
        "\"{0}\" is not a valid key prefix. Use path segments separated by single slashes, without . or .. segments or control characters, short enough to leave room for the object name."
    )]
    PrefixInvalid(String),
    #[error(
        "\"{0}\" is not a valid syslog host. Use a hostname or IP address, with no scheme, port or path."
    )]
    HostInvalid(String),
    #[error("The port must be between 1 and 65535.")]
    PortInvalid,
    #[error("{0} is not a syslog facility. Use a number from 0 to 23.")]
    FacilityInvalid(u8),
    #[error(
        "\"{0}\" is not a valid syslog hostname. Use 1 to 255 printable ASCII characters with no spaces."
    )]
    HostnameInvalid(String),
    #[error("Secrets must be an object of named text values.")]
    SecretsMalformed,
    #[error("\"{0}\" is not a secret this destination takes.")]
    SecretUnknown(String),
    #[error("Enter the {}.", .0.label())]
    SecretInvalid(SecretName),
    #[error("The {} must be in PEM form.", .0.label())]
    CertificateInvalid(SecretName),
    #[error("A client certificate and a client key go together: set both or neither.")]
    CertificateUnpaired,
    #[error("Certificates need the tls protocol: UDP syslog is unencrypted.")]
    CertificateWithoutTls,
    #[error(
        "The retention period must be a whole number of days, at least one, or empty to keep audit events forever."
    )]
    RetentionInvalid,
    #[error(
        "The export range is invalid. Use ISO 8601 dates, with the start on or before the end."
    )]
    ExportRangeInvalid,
    #[error("Choose ndjson or csv as the export format.")]
    ExportFormatInvalid,
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::DestinationNotFound => DESTINATION_NOT_FOUND,
            Self::DestinationDuplicate => DESTINATION_DUPLICATE,
            Self::NameInvalid => NAME_INVALID,
            Self::KindInvalid => KIND_INVALID,
            Self::UrlInvalid(_) => URL_INVALID,
            Self::BucketInvalid(_) => BUCKET_INVALID,
            Self::RegionInvalid(_) => REGION_INVALID,
            Self::PrefixInvalid(_) => PREFIX_INVALID,
            Self::HostInvalid(_) => HOST_INVALID,
            Self::PortInvalid => PORT_INVALID,
            Self::FacilityInvalid(_) => FACILITY_INVALID,
            Self::HostnameInvalid(_) => HOSTNAME_INVALID,
            Self::SecretsMalformed | Self::SecretUnknown(_) | Self::SecretInvalid(_) => {
                SECRET_INVALID
            }
            Self::CertificateInvalid(_)
            | Self::CertificateUnpaired
            | Self::CertificateWithoutTls => CERTIFICATE_INVALID,
            Self::RetentionInvalid => RETENTION_INVALID,
            Self::ExportRangeInvalid => EXPORT_RANGE_INVALID,
            Self::ExportFormatInvalid => EXPORT_FORMAT_INVALID,
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            Self::DestinationNotFound => NOT_FOUND,
            Self::DestinationDuplicate => CONFLICT,
            _ => BAD_REQUEST,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every() -> [Error; 22] {
        [
            Error::DestinationNotFound,
            Error::DestinationDuplicate,
            Error::NameInvalid,
            Error::KindInvalid,
            Error::UrlInvalid(UrlReason::Scheme),
            Error::BucketInvalid("My_Bucket".into()),
            Error::RegionInvalid("US East".into()),
            Error::PrefixInvalid("a//b".into()),
            Error::HostInvalid("https://siem".into()),
            Error::PortInvalid,
            Error::FacilityInvalid(24),
            Error::HostnameInvalid("zone host".into()),
            Error::SecretsMalformed,
            Error::SecretUnknown("password".into()),
            Error::SecretInvalid(SecretName::SigningSecret),
            Error::CertificateInvalid(SecretName::CaCertificate),
            Error::CertificateUnpaired,
            Error::CertificateWithoutTls,
            Error::RetentionInvalid,
            Error::ExportRangeInvalid,
            Error::ExportFormatInvalid,
            Error::UrlInvalid(UrlReason::Metadata),
        ]
    }

    #[test]
    fn codes_are_the_wire_contract() {
        let codes: Vec<&str> = every().iter().map(Error::code).collect();
        assert_eq!(
            codes,
            [
                "audit_destination_not_found",
                "audit_destination_duplicate",
                "audit_destination_name_invalid",
                "audit_destination_kind_invalid",
                "audit_destination_url_invalid",
                "audit_destination_bucket_invalid",
                "audit_destination_region_invalid",
                "audit_destination_prefix_invalid",
                "audit_destination_host_invalid",
                "audit_destination_port_invalid",
                "audit_destination_facility_invalid",
                "audit_destination_hostname_invalid",
                "audit_destination_secret_invalid",
                "audit_destination_secret_invalid",
                "audit_destination_secret_invalid",
                "audit_destination_certificate_invalid",
                "audit_destination_certificate_invalid",
                "audit_destination_certificate_invalid",
                "audit_retention_invalid",
                "audit_export_range_invalid",
                "audit_export_format_invalid",
                "audit_destination_url_invalid",
            ]
        );
    }

    #[test]
    fn constants_are_the_wire_contract() {
        assert_eq!(DESTINATION_NOT_FOUND, "audit_destination_not_found");
        assert_eq!(DESTINATION_DUPLICATE, "audit_destination_duplicate");
        assert_eq!(NAME_INVALID, "audit_destination_name_invalid");
        assert_eq!(KIND_INVALID, "audit_destination_kind_invalid");
        assert_eq!(URL_INVALID, "audit_destination_url_invalid");
        assert_eq!(BUCKET_INVALID, "audit_destination_bucket_invalid");
        assert_eq!(REGION_INVALID, "audit_destination_region_invalid");
        assert_eq!(PREFIX_INVALID, "audit_destination_prefix_invalid");
        assert_eq!(HOST_INVALID, "audit_destination_host_invalid");
        assert_eq!(PORT_INVALID, "audit_destination_port_invalid");
        assert_eq!(FACILITY_INVALID, "audit_destination_facility_invalid");
        assert_eq!(HOSTNAME_INVALID, "audit_destination_hostname_invalid");
        assert_eq!(SECRET_INVALID, "audit_destination_secret_invalid");
        assert_eq!(CERTIFICATE_INVALID, "audit_destination_certificate_invalid");
        assert_eq!(RETENTION_INVALID, "audit_retention_invalid");
        assert_eq!(EXPORT_RANGE_INVALID, "audit_export_range_invalid");
        assert_eq!(EXPORT_FORMAT_INVALID, "audit_export_format_invalid");
    }

    #[test]
    fn statuses_follow_the_code() {
        for error in every() {
            let expected = match error.code() {
                DESTINATION_NOT_FOUND => 404,
                DESTINATION_DUPLICATE => 409,
                _ => 400,
            };
            assert_eq!(error.status(), expected, "{error:?}");
        }
    }

    #[test]
    fn every_message_is_one_line() {
        for error in every() {
            let message = error.to_string();
            assert!(!message.is_empty() && !message.contains('\n'), "{error:?}");
        }
    }

    #[test]
    fn messages_name_the_rejected_value() {
        assert!(
            Error::BucketInvalid("My_Bucket".into())
                .to_string()
                .contains("\"My_Bucket\"")
        );
        assert!(Error::FacilityInvalid(24).to_string().starts_with("24 "));
        assert_eq!(
            Error::SecretInvalid(SecretName::SecretAccessKey).to_string(),
            "Enter the secret access key."
        );
        assert_eq!(
            Error::CertificateInvalid(SecretName::ClientKey).to_string(),
            "The client key must be in PEM form."
        );
        assert_eq!(
            Error::UrlInvalid(UrlReason::Query).to_string(),
            "The URL must not include a query."
        );
    }
}
