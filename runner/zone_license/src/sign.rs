use std::io::Write;

use chrono::{SubsecRound, Utc};
use uuid::Uuid;
use zone_enterprise::Keyring;

use crate::private_key::PrivateKey;
use crate::sign_arguments::SignArguments;
use crate::sign_error::SignError;
use crate::signer::Signer;
use crate::terms::Terms;
use crate::variable::Variable;

pub fn run(
    arguments: SignArguments,
    keyring: &Keyring,
    stdout: &mut dyn Write,
) -> Result<(), SignError> {
    let key_id = arguments
        .key_id
        .or_else(|| Variable::KeyId.read())
        .ok_or(SignError::MissingKeyId)?;
    let signer = Signer::new(PrivateKey::load(arguments.private_key_file)?, key_id);
    signer.trusted_by(keyring)?;
    let terms = Terms {
        id: arguments.id.unwrap_or_else(Uuid::new_v4),
        customer: arguments.customer,
        seats: arguments.seats,
        issued_at: arguments
            .issued_at
            .unwrap_or_else(|| Utc::now().trunc_subsecs(0)),
        expires_at: arguments.expires_at,
    };
    let license = signer.sign(&terms)?;
    writeln!(stdout, "{license}")?;
    tracing::info!(
        id = %terms.id,
        key_id = signer.key_id(),
        seats = terms.seats,
        expires_at = %terms.expires_at,
        "signed a license"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::{DateTime, TimeZone};
    use zone_enterprise::TrustedKey;

    use super::*;
    use crate::untrusted::Untrusted;

    const KEY_ID: &str = "test-1";
    const SEED: [u8; 32] = [5; 32];

    struct Fixture {
        _directory: tempfile::TempDir,
        arguments: SignArguments,
    }

    fn expires_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2027, 11, 1, 0, 0, 0).unwrap()
    }

    fn fixture() -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("signing.key");
        fs::write(&path, PrivateKey::new(SEED).encode()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        Fixture {
            _directory: directory,
            arguments: SignArguments {
                customer: "Acme Ltd".into(),
                seats: 25,
                expires_at: expires_at(),
                key_id: Some(KEY_ID.into()),
                private_key_file: Some(path),
                issued_at: None,
                id: None,
            },
        }
    }

    fn keyring() -> Keyring {
        Keyring::new([TrustedKey::new(KEY_ID, PrivateKey::new(SEED).public()).unwrap()]).unwrap()
    }

    #[test]
    fn prints_only_a_verifiable_license() {
        let fixture = fixture();
        let id = Uuid::from_u128(42);
        let issued_at = Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap();
        let mut stdout = Vec::new();
        run(
            SignArguments {
                id: Some(id),
                issued_at: Some(issued_at),
                ..fixture.arguments
            },
            &keyring(),
            &mut stdout,
        )
        .unwrap();
        let printed = String::from_utf8(stdout).unwrap();
        assert_eq!(printed.lines().count(), 1, "{printed}");
        let license = keyring().verify(&printed).unwrap();
        let payload = license.payload();
        assert_eq!(payload.id, id);
        assert_eq!(payload.customer, "Acme Ltd");
        assert_eq!(payload.seats, 25);
        assert_eq!(payload.issued_at, issued_at);
        assert_eq!(payload.expires_at, expires_at());
        assert_eq!(payload.key_id, KEY_ID);
    }

    #[test]
    fn defaults_the_issue_time_to_now_in_whole_seconds() {
        let fixture = fixture();
        let before = Utc::now().trunc_subsecs(0);
        let mut stdout = Vec::new();
        run(fixture.arguments, &keyring(), &mut stdout).unwrap();
        let license = keyring()
            .verify(&String::from_utf8(stdout).unwrap())
            .unwrap();
        let issued_at = license.payload().issued_at;
        assert!(
            issued_at >= before && issued_at <= Utc::now(),
            "{issued_at}"
        );
        assert_eq!(issued_at, issued_at.trunc_subsecs(0));
    }

    #[test]
    fn refuses_a_key_the_keyring_does_not_hold() {
        let fixture = fixture();
        let mut stdout = Vec::new();
        let error = run(fixture.arguments, &Keyring::production(), &mut stdout).unwrap_err();
        assert!(
            matches!(error, SignError::Untrusted(Untrusted::UnknownKeyId { .. })),
            "got {error:?}"
        );
        assert!(stdout.is_empty());
    }

    #[test]
    fn refuses_a_key_that_does_not_match_its_id() {
        let fixture = fixture();
        let other =
            Keyring::new([TrustedKey::new(KEY_ID, PrivateKey::new([6; 32]).public()).unwrap()])
                .unwrap();
        let error = run(fixture.arguments, &other, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(
                error,
                SignError::Untrusted(Untrusted::PublicKeyMismatch { .. })
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn refuses_invalid_terms() {
        let fixture = fixture();
        let mut stdout = Vec::new();
        let error = run(
            SignArguments {
                seats: 0,
                ..fixture.arguments
            },
            &keyring(),
            &mut stdout,
        )
        .unwrap_err();
        assert!(
            matches!(
                error,
                SignError::License(zone_enterprise::Error::Payload(_))
            ),
            "got {error:?}"
        );
        assert!(stdout.is_empty());
    }
}
