use abnegate_secret::REDACTED;
use chrono::Utc;
use uuid::Uuid;
use zone_enterprise::sso::Protocol;

use super::providers::Row;
use super::{Asserted, Connection, ProviderError};

const SEALED: &str = "ENC[v1:sealed]";
const SSO_URL: &str = "https://acme.okta.com/app/zone/sso/saml";
const CERTIFICATE: &str = "MIIC";

fn oidc_row() -> Row {
    Row {
        id: Uuid::nil(),
        protocol: Protocol::Oidc.as_str().into(),
        display_name: "Okta".into(),
        issuer: "https://acme.okta.com".into(),
        client_id: Some("0oa1".into()),
        client_secret: Some(SEALED.into()),
        sso_url: None,
        certificates: None,
        metadata_url: None,
        allowed_domains: vec!["acme.com".into()],
        require_verified_email: true,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn saml_columns(row: Row) -> Row {
    Row {
        client_id: None,
        client_secret: None,
        sso_url: Some(SSO_URL.into()),
        certificates: Some(vec![CERTIFICATE.into()]),
        metadata_url: Some("https://acme.okta.com/app/zone/sso/saml/metadata".into()),
        ..row
    }
}

fn refused(row: Row) {
    assert!(
        matches!(row.record(), Err(ProviderError::Protocol(id, _)) if id == Uuid::nil()),
        "an inconsistent row must not become a record"
    );
}

#[test]
fn debug_never_shows_the_code_hash() {
    let shown = format!(
        "{:?}",
        Asserted {
            code_hash: "code-hash-secret".into(),
            subject: "subject-1".into(),
            email: "ada@acme.com".into(),
            display_name: Some("Ada".into()),
        }
    );
    assert!(!shown.contains("secret"), "{shown}");
    assert!(shown.contains(REDACTED), "{shown}");
    assert!(shown.contains("subject-1"), "{shown}");
}

#[test]
fn an_oidc_row_becomes_an_oidc_connection() {
    let record = oidc_row().record().unwrap();
    assert_eq!(record.protocol, Protocol::Oidc);
    assert_eq!(
        record.connection,
        Connection::Oidc {
            client_id: "0oa1".into(),
            sealed_secret: SEALED.into(),
        }
    );
    assert_eq!(record.client_id(), Some("0oa1"));
}

#[test]
fn an_oidc_row_with_saml_columns_is_refused() {
    refused(saml_columns(oidc_row()));
    refused(Row {
        sso_url: Some(SSO_URL.into()),
        ..oidc_row()
    });
    refused(Row {
        certificates: Some(vec![CERTIFICATE.into()]),
        ..oidc_row()
    });
    refused(Row {
        metadata_url: Some(SSO_URL.into()),
        ..oidc_row()
    });
}

#[test]
fn a_row_with_half_a_connection_is_refused() {
    refused(Row {
        client_secret: None,
        ..oidc_row()
    });
    refused(Row {
        client_id: None,
        ..oidc_row()
    });
    refused(Row {
        certificates: None,
        ..saml_columns(oidc_row())
    });
    refused(Row {
        client_id: None,
        client_secret: None,
        ..oidc_row()
    });
}

#[test]
fn an_unknown_protocol_is_refused() {
    refused(Row {
        protocol: "ws-federation".into(),
        ..oidc_row()
    });
}
