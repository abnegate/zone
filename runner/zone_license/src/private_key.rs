use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use zone_enterprise::SigningKey;

use crate::private_key_error::PrivateKeyError;
use crate::variable::Variable;

pub const SEED_LENGTH: usize = 32;
#[cfg(unix)]
const SHARED_MODE_BITS: u32 = 0o077;

pub struct PrivateKey {
    key: SigningKey,
}

impl PrivateKey {
    pub fn new(seed: [u8; SEED_LENGTH]) -> Self {
        Self {
            key: SigningKey::from_bytes(&seed),
        }
    }

    pub fn parse(text: &str) -> Result<Self, PrivateKeyError> {
        let bytes = STANDARD
            .decode(text.trim())
            .map_err(|_| PrivateKeyError::Encoding)?;
        let length = bytes.len();
        let seed: [u8; SEED_LENGTH] = bytes
            .try_into()
            .map_err(|_| PrivateKeyError::Length { length })?;
        Ok(Self::new(seed))
    }

    pub fn load(file: Option<PathBuf>) -> Result<Self, PrivateKeyError> {
        let file = file.or_else(|| Variable::PrivateKeyFile.read().map(PathBuf::from));
        Self::resolve(file.as_deref(), Variable::PrivateKey.read().as_deref())
    }

    pub fn resolve(file: Option<&Path>, value: Option<&str>) -> Result<Self, PrivateKeyError> {
        match (file, value) {
            (Some(_), Some(_)) => Err(PrivateKeyError::Ambiguous),
            (None, None) => Err(PrivateKeyError::Missing),
            (None, Some(value)) => Self::parse(value),
            (Some(path), None) => {
                let path = secured(path)?;
                let text = fs::read_to_string(&path)
                    .map_err(|source| PrivateKeyError::Read { path, source })?;
                Self::parse(&text)
            }
        }
    }

    pub fn encode(&self) -> String {
        STANDARD.encode(self.key.to_bytes())
    }

    pub fn public(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    pub(crate) fn into_signing_key(self) -> SigningKey {
        self.key
    }
}

impl fmt::Debug for PrivateKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PrivateKey")
            .field("public", &STANDARD.encode(self.public()))
            .finish_non_exhaustive()
    }
}

/// Returns the path it checked so CodeQL's barrier model can bind the guard.
pub fn secured(path: &Path) -> Result<PathBuf, PrivateKeyError> {
    let metadata = fs::metadata(path).map_err(|source| PrivateKeyError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(PrivateKeyError::NotAFile {
            path: path.to_path_buf(),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode() & 0o777;
        if mode & SHARED_MODE_BITS != 0 {
            return Err(PrivateKeyError::Permissions {
                path: path.to_path_buf(),
                mode,
            });
        }
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    const SEED: [u8; SEED_LENGTH] = [7; SEED_LENGTH];

    fn encoded() -> String {
        STANDARD.encode(SEED)
    }

    fn key_file(contents: &str, mode: u32) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(file.path(), fs::Permissions::from_mode(mode)).unwrap();
        }
        #[cfg(not(unix))]
        let _ = mode;
        file
    }

    #[test]
    fn parses_a_standard_base64_seed() {
        let key = PrivateKey::parse(&encoded()).unwrap();
        assert_eq!(key.encode(), encoded());
        assert_eq!(
            key.public(),
            SigningKey::from_bytes(&SEED).verifying_key().to_bytes()
        );
    }

    #[test]
    fn trims_surrounding_whitespace() {
        let key = PrivateKey::parse(&format!("  {}\n", encoded())).unwrap();
        assert_eq!(key.encode(), encoded());
    }

    #[test]
    fn refuses_bad_base64() {
        for text in ["not base64!", "", "AAAA\nAAAA"] {
            assert!(
                matches!(
                    PrivateKey::parse(text),
                    Err(PrivateKeyError::Encoding | PrivateKeyError::Length { .. })
                ),
                "accepted {text:?}"
            );
        }
        assert!(matches!(
            PrivateKey::parse("not base64!"),
            Err(PrivateKeyError::Encoding)
        ));
    }

    #[test]
    fn refuses_the_url_safe_alphabet() {
        let seed = [0xfb; SEED_LENGTH];
        let url_safe = base64::engine::general_purpose::URL_SAFE.encode(seed);
        assert!(url_safe.contains(['-', '_']));
        assert!(matches!(
            PrivateKey::parse(&url_safe),
            Err(PrivateKeyError::Encoding)
        ));
    }

    #[test]
    fn refuses_the_wrong_length() {
        for length in [0, 31, 33, 64] {
            let text = STANDARD.encode(vec![1; length]);
            assert!(
                matches!(PrivateKey::parse(&text), Err(PrivateKeyError::Length { length: found }) if found == length),
                "accepted {length} bytes"
            );
        }
    }

    #[test]
    fn resolves_a_value() {
        let key = PrivateKey::resolve(None, Some(&encoded())).unwrap();
        assert_eq!(key.encode(), encoded());
    }

    #[test]
    fn resolves_a_private_file() {
        let file = key_file(&format!("{}\n", encoded()), 0o600);
        let key = PrivateKey::resolve(Some(file.path()), None).unwrap();
        assert_eq!(key.encode(), encoded());
    }

    #[test]
    fn refuses_both_sources() {
        let file = key_file(&encoded(), 0o600);
        assert!(matches!(
            PrivateKey::resolve(Some(file.path()), Some(&encoded())),
            Err(PrivateKeyError::Ambiguous)
        ));
    }

    #[test]
    fn refuses_neither_source() {
        assert!(matches!(
            PrivateKey::resolve(None, None),
            Err(PrivateKeyError::Missing)
        ));
    }

    #[test]
    fn refuses_a_missing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("absent.key");
        assert!(matches!(
            PrivateKey::resolve(Some(&path), None),
            Err(PrivateKeyError::Read { path: found, .. }) if found == path
        ));
    }

    #[test]
    fn refuses_a_directory() {
        let directory = tempfile::tempdir().unwrap();
        assert!(matches!(
            PrivateKey::resolve(Some(directory.path()), None),
            Err(PrivateKeyError::NotAFile { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_file_others_can_access() {
        for mode in [0o644, 0o640, 0o604, 0o620, 0o601, 0o610] {
            let file = key_file(&encoded(), mode);
            assert!(
                matches!(
                    PrivateKey::resolve(Some(file.path()), None),
                    Err(PrivateKeyError::Permissions { mode: found, .. }) if found == mode
                ),
                "accepted mode {mode:o}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn accepts_an_owner_read_only_file() {
        let file = key_file(&encoded(), 0o400);
        assert!(PrivateKey::resolve(Some(file.path()), None).is_ok());
    }

    #[test]
    fn debug_redacts_the_seed() {
        let key = PrivateKey::new(SEED);
        let shown = format!("{key:?} {key:#?}");
        assert!(!shown.contains(&encoded()), "{shown}");
        assert!(!shown.contains(&format!("{:?}", SEED)), "{shown}");
        assert!(!shown.contains("7, 7, 7"), "{shown}");
        assert!(shown.contains(&STANDARD.encode(key.public())), "{shown}");
    }

    #[test]
    fn errors_never_show_the_value() {
        let text = format!("{}{}", encoded(), encoded());
        let error = PrivateKey::parse(&text).unwrap_err();
        assert!(!error.to_string().contains(&encoded()));
    }
}
