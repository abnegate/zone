use super::*;
use crate::audit::url_reason::UrlReason;

fn settings() -> S3Settings {
    S3Settings {
        bucket: "audit-logs".into(),
        region: "us-east-1".into(),
        endpoint: None,
        prefix: String::new(),
        addressing: Addressing::default(),
    }
}

#[test]
fn addressing_is_snake_case_and_defaults_to_virtual_host() {
    let values: Vec<String> = Addressing::ALL
        .iter()
        .map(|addressing| serde_json::to_string(addressing).unwrap())
        .collect();
    assert_eq!(values, [r#""path""#, r#""virtual_host""#]);
    for addressing in Addressing::ALL {
        assert_eq!(
            serde_json::to_value(addressing).unwrap(),
            addressing.as_str()
        );
    }
    assert_eq!(Addressing::default(), Addressing::VirtualHost);
    assert!(serde_json::from_str::<Addressing>(r#""virtualHost""#).is_err());
}

#[test]
fn bucket_names_follow_the_aws_rules() {
    for bucket in [
        "abc",
        "audit-logs",
        "audit.logs.2026",
        "1bucket",
        &"a".repeat(63),
    ] {
        assert!(valid_bucket(bucket), "{bucket:?}");
    }
    for bucket in [
        "",
        "ab",
        &"a".repeat(64),
        "Audit-Logs",
        "audit_logs",
        "audit logs",
        "-audit",
        "audit-",
        ".audit",
        "audit.",
        "audit..logs",
        "192.168.5.4",
        "xn--audit",
        "sthree-audit",
        "amzn-s3-demo-audit",
        "audit-s3alias",
        "audit--ol-s3",
        "audit.mrap",
        "audit--x-s3",
        "audit--table-s3",
        "bücket",
    ] {
        assert!(!valid_bucket(bucket), "{bucket:?}");
    }
}

#[test]
fn an_invalid_bucket_is_named() {
    let error = S3Settings {
        bucket: " Audit_Logs ".into(),
        ..settings()
    }
    .validated();
    assert_eq!(error, Err(Error::BucketInvalid("Audit_Logs".into())));
}

#[test]
fn regions_are_lowercase_letters_digits_and_hyphens() {
    for region in ["us-east-1", "eu-west-2", "auto", "garage"] {
        assert!(valid_region(region), "{region:?}");
    }
    for region in ["", "US-EAST-1", "us east 1", "us_east_1", "us-east-1/"] {
        assert!(!valid_region(region), "{region:?}");
        assert_eq!(
            S3Settings {
                region: region.into(),
                ..settings()
            }
            .validated(),
            Err(Error::RegionInvalid(region.trim().into())),
            "{region:?}"
        );
    }
}

#[test]
fn prefixes_are_normalised() {
    for (raw, expected) in [
        ("", ""),
        ("/", ""),
        ("  ", ""),
        ("zone", "zone/"),
        ("zone/", "zone/"),
        ("/zone", "zone/"),
        ("//zone/audit/", "zone/audit/"),
        (" audit/eu ", "audit/eu/"),
        ("logs/2026.v1", "logs/2026.v1/"),
    ] {
        assert_eq!(prefix(raw).as_deref(), Ok(expected), "{raw:?}");
        assert_eq!(prefix(expected).as_deref(), Ok(expected), "{expected:?}");
    }
}

#[test]
fn prefixes_refuse_empty_dot_and_control_segments() {
    for raw in ["a//b", "a/./b", "../a", "a/..", "a//", "a\nb", "a\u{0}b"] {
        assert_eq!(
            prefix(raw),
            Err(Error::PrefixInvalid(raw.trim().into())),
            "{raw:?}"
        );
    }
}

#[test]
fn a_prefix_leaves_room_for_the_object_name() {
    let longest = "a".repeat(PREFIX_MAXIMUM - 1);
    let normalised = prefix(&longest).unwrap();
    assert_eq!(
        normalised.len() + LONGEST_OBJECT_NAME.len(),
        OBJECT_KEY_MAXIMUM
    );
    assert!(prefix(&"a".repeat(PREFIX_MAXIMUM)).is_err());
}

#[test]
fn the_endpoint_follows_the_url_syntax() {
    let validated = S3Settings {
        endpoint: Some(" http://minio:9000 ".into()),
        ..settings()
    }
    .validated()
    .unwrap();
    assert_eq!(validated.endpoint.as_deref(), Some("http://minio:9000"));
    for (endpoint, reason) in [
        ("minio:9000", UrlReason::Scheme),
        ("https://key:secret@minio", UrlReason::Userinfo),
        ("https://minio?x-id=1", UrlReason::Query),
    ] {
        assert_eq!(
            S3Settings {
                endpoint: Some(endpoint.into()),
                ..settings()
            }
            .validated(),
            Err(Error::UrlInvalid(reason)),
            "{endpoint:?}"
        );
    }
}

#[test]
fn a_blank_endpoint_means_aws() {
    let validated = S3Settings {
        endpoint: Some("  ".into()),
        ..settings()
    }
    .validated()
    .unwrap();
    assert_eq!(validated.endpoint, None);
}

#[test]
fn the_endpoint_url_defaults_to_the_regions_aws_host() {
    assert_eq!(
        S3Settings {
            region: "eu-west-2".into(),
            ..settings()
        }
        .endpoint_url()
        .as_str(),
        "https://s3.eu-west-2.amazonaws.com/"
    );
    assert_eq!(
        S3Settings {
            endpoint: Some("http://10.0.0.7:9000".into()),
            ..settings()
        }
        .endpoint_url()
        .as_str(),
        "http://10.0.0.7:9000/"
    );
}

#[test]
fn the_request_base_puts_the_bucket_where_the_addressing_says() {
    for (endpoint, addressing, expected) in [
        (
            None,
            Addressing::VirtualHost,
            "https://audit-logs.s3.us-east-1.amazonaws.com/",
        ),
        (
            None,
            Addressing::Path,
            "https://s3.us-east-1.amazonaws.com/audit-logs",
        ),
        (
            Some("http://minio:9000"),
            Addressing::VirtualHost,
            "http://audit-logs.minio:9000/",
        ),
        (
            Some("http://minio:9000/storage/"),
            Addressing::Path,
            "http://minio:9000/storage/audit-logs",
        ),
        (
            Some("https://gateway.example/storage/"),
            Addressing::VirtualHost,
            "https://audit-logs.gateway.example/storage",
        ),
        (
            Some("http://10.0.0.5:9000"),
            Addressing::Path,
            "http://10.0.0.5:9000/audit-logs",
        ),
    ] {
        let validated = S3Settings {
            endpoint: endpoint.map(Into::into),
            addressing,
            ..settings()
        }
        .validated()
        .unwrap();
        assert_eq!(
            validated.request_base().unwrap().as_str(),
            expected,
            "{endpoint:?} {addressing:?}"
        );
    }
}

#[test]
fn virtual_host_addressing_refuses_an_ip_endpoint() {
    for endpoint in [
        "http://10.0.0.5:9000",
        "http://[fd00::5]:9000",
        "http://127.0.0.1",
    ] {
        assert_eq!(
            S3Settings {
                endpoint: Some(endpoint.into()),
                addressing: Addressing::VirtualHost,
                ..settings()
            }
            .validated(),
            Err(Error::VirtualHostUnaddressable),
            "{endpoint}"
        );
    }
}

#[test]
fn validation_is_idempotent() {
    let validated = S3Settings {
        bucket: " audit-logs ".into(),
        region: " us-east-1".into(),
        endpoint: Some("https://r2.example.com ".into()),
        prefix: "/zone".into(),
        addressing: Addressing::Path,
    }
    .validated()
    .unwrap();
    assert_eq!(validated.clone().validated(), Ok(validated));
}

#[test]
fn debug_never_shows_the_keys() {
    let secrets = S3Secrets {
        access_key_id: "AKIAEXAMPLE".into(),
        secret_access_key: "wJalrXUtnFEMI".into(),
    };
    let shown = format!("{secrets:?}");
    assert!(
        !shown.contains("AKIAEXAMPLE") && !shown.contains("wJalrXUtnFEMI"),
        "{shown}"
    );
}
