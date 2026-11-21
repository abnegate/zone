use super::*;

fn every() -> [Error; 28] {
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
        Error::VirtualHostUnaddressable,
        Error::HostRefused(UrlReason::Metadata),
        Error::HostRefused(UrlReason::Unlisted),
        Error::HostRefused(UrlReason::Host),
        Error::CertificateMismatched,
        Error::Rebasing,
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
            "audit_destination_url_invalid",
            "audit_destination_host_invalid",
            "audit_destination_host_invalid",
            "audit_destination_host_invalid",
            "audit_destination_certificate_invalid",
            "audit_rebasing",
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
    assert_eq!(REBASING, "audit_rebasing");
}

#[test]
fn statuses_follow_the_code() {
    for error in every() {
        let expected = match error.code() {
            DESTINATION_NOT_FOUND => 404,
            DESTINATION_DUPLICATE => 409,
            REBASING => 503,
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

#[test]
fn a_refused_syslog_host_says_why_without_calling_it_malformed() {
    assert_eq!(
        Error::HostRefused(UrlReason::Metadata).to_string(),
        "The syslog host must not be a link-local or cloud metadata address."
    );
    let unlisted = Error::HostRefused(UrlReason::Unlisted).to_string();
    assert!(unlisted.contains("ZONE_ENDPOINT_HOSTS"), "{unlisted}");
    for reason in UrlReason::ALL {
        let message = Error::HostRefused(reason).to_string();
        assert!(!message.contains("not a valid"), "{message}");
        assert!(message.starts_with("The syslog host"), "{message}");
    }
}

#[test]
fn a_mismatched_key_is_not_called_malformed() {
    let message = Error::CertificateMismatched.to_string();
    assert_eq!(
        message,
        "The client key doesn't belong to the client certificate."
    );
    assert!(!message.contains("PEM"));
}
