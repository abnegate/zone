use std::io::Write;

use clap::Subcommand;
use zone_enterprise::Keyring;

use crate::private_key::PrivateKey;
use crate::registry::Jwks;
use crate::registry_arguments::RegistryArguments;
use crate::registry_error::RegistryError;
use crate::signer::Signer;
use crate::variable::Variable;

#[derive(Debug, Clone, Subcommand)]
pub enum RegistryCommand {
    /// Print the JSON Web Key Set the registry verifies tokens with.
    ///
    /// It holds the public half of each generation's token key, derived from the license signing key.
    Jwks(RegistryArguments),
}

impl RegistryCommand {
    pub fn run(self, keyring: &Keyring, stdout: &mut dyn Write) -> Result<(), RegistryError> {
        match self {
            Self::Jwks(arguments) => jwks(arguments, keyring, stdout),
        }
    }
}

fn jwks(
    arguments: RegistryArguments,
    keyring: &Keyring,
    stdout: &mut dyn Write,
) -> Result<(), RegistryError> {
    let key_id = arguments
        .key_id
        .or_else(|| Variable::KeyId.read())
        .ok_or(RegistryError::MissingKeyId)?;
    let signer = Signer::new(PrivateKey::load(arguments.private_key_file)?, key_id);
    signer.trusted_by(keyring)?;
    let set = Jwks::derive(&signer, &arguments.generations);
    let text = serde_json::to_string_pretty(&set).expect("a key set has only string keys");
    writeln!(stdout, "{text}")?;
    let token_key_ids: Vec<&str> = set.keys().iter().map(|key| key.key_id()).collect();
    tracing::info!(
        key_id = signer.key_id(),
        token_key_ids = %token_key_ids.join(" "),
        "printed the registry key set"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::num::NonZeroU32;

    use serde_json::Value;
    use zone_enterprise::TrustedKey;

    use super::*;
    use crate::registry::TokenKey;
    use crate::untrusted::Untrusted;

    const KEY_ID: &str = "test-1";
    const SEED: [u8; 32] = [11; 32];

    struct Fixture {
        _directory: tempfile::TempDir,
        arguments: RegistryArguments,
    }

    fn generation(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).unwrap()
    }

    fn fixture(generations: &[u32]) -> Fixture {
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
            arguments: RegistryArguments {
                generations: generations.iter().copied().map(generation).collect(),
                key_id: Some(KEY_ID.into()),
                private_key_file: Some(path),
            },
        }
    }

    fn keyring() -> Keyring {
        Keyring::new([TrustedKey::new(KEY_ID, PrivateKey::new(SEED).public()).unwrap()]).unwrap()
    }

    fn run_to_string(generations: &[u32]) -> String {
        let fixture = fixture(generations);
        let mut stdout = Vec::new();
        RegistryCommand::Jwks(fixture.arguments)
            .run(&keyring(), &mut stdout)
            .unwrap();
        String::from_utf8(stdout).unwrap()
    }

    fn printed(generations: &[u32]) -> Value {
        serde_json::from_str(&run_to_string(generations)).unwrap()
    }

    fn key_ids(printed: &Value) -> Vec<&str> {
        printed["keys"]
            .as_array()
            .unwrap()
            .iter()
            .map(|key| key["kid"].as_str().unwrap())
            .collect()
    }

    fn signer() -> Signer {
        Signer::new(PrivateKey::new(SEED), KEY_ID)
    }

    #[test]
    fn prints_one_key_per_generation_in_the_go_jose_okp_shape() {
        let printed = printed(&[1, 2]);
        assert_eq!(printed.as_object().unwrap().len(), 1, "{printed}");
        let keys = printed["keys"].as_array().unwrap();
        assert_eq!(keys.len(), 2, "{printed}");
        for (key, generation) in keys.iter().zip([generation(1), generation(2)]) {
            let token_key = TokenKey::derive(&signer(), generation);
            assert_eq!(
                key,
                &serde_json::json!({
                    "kty": "OKP",
                    "crv": "Ed25519",
                    "alg": "EdDSA",
                    "use": "sig",
                    "kid": format!("registry-test-1-{generation}"),
                    "x": token_key.jwk().x(),
                })
            );
        }
        assert_eq!(
            key_ids(&printed),
            ["registry-test-1-1", "registry-test-1-2"]
        );
    }

    #[test]
    fn prints_a_single_generation() {
        assert_eq!(key_ids(&printed(&[1])), ["registry-test-1-1"]);
    }

    #[test]
    fn a_repeated_generation_is_printed_once() {
        assert_eq!(
            key_ids(&printed(&[2, 1, 2])),
            ["registry-test-1-2", "registry-test-1-1"]
        );
    }

    #[test]
    fn never_prints_the_seed_or_a_private_component() {
        let printed = run_to_string(&[1]);
        assert!(
            !printed.contains(&PrivateKey::new(SEED).encode()),
            "{printed}"
        );
        assert!(!printed.contains("\"d\""), "{printed}");
    }

    #[test]
    fn refuses_a_key_the_keyring_does_not_hold() {
        let fixture = fixture(&[1]);
        let mut stdout = Vec::new();
        let error = RegistryCommand::Jwks(fixture.arguments)
            .run(&Keyring::production(), &mut stdout)
            .unwrap_err();
        assert!(
            matches!(
                error,
                RegistryError::Untrusted(Untrusted::UnknownKeyId { .. })
            ),
            "got {error:?}"
        );
        assert!(stdout.is_empty());
    }

    #[test]
    fn refuses_a_key_that_does_not_match_its_id() {
        let fixture = fixture(&[1]);
        let other =
            Keyring::new([TrustedKey::new(KEY_ID, PrivateKey::new([12; 32]).public()).unwrap()])
                .unwrap();
        let mut stdout = Vec::new();
        let error = RegistryCommand::Jwks(fixture.arguments)
            .run(&other, &mut stdout)
            .unwrap_err();
        assert!(
            matches!(
                error,
                RegistryError::Untrusted(Untrusted::PublicKeyMismatch { .. })
            ),
            "got {error:?}"
        );
        assert!(stdout.is_empty());
    }
}
