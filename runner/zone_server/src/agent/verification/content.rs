use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use sha2::{Digest, Sha256};

const DIGEST_BYTES: usize = 32;
const MODE_MASK: u32 = 0o7777;
const READ_CHUNK: usize = 64 * 1024;

/// What a file is, at one instant, in one tree.
///
/// Two files are the same file when their digest, size and mode all agree. Mode
/// rides along with the digest because an executable bit is part of what a
/// harness does, not incidental metadata.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct Content {
    digest: [u8; DIGEST_BYTES],
    mode: u32,
    size: u64,
}

impl Content {
    pub fn read(path: &Path, max_bytes: u64) -> io::Result<Self> {
        let details = path.metadata()?;
        if details.len() > max_bytes {
            return Err(io::Error::other(
                "The file is larger than the closure byte budget.",
            ));
        }
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; READ_CHUNK];
        let mut read = 0_u64;
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            read += count as u64;
            if read > max_bytes {
                return Err(io::Error::other(
                    "The file grew past the closure byte budget while it was read.",
                ));
            }
            hasher.update(&buffer[..count]);
        }
        if read != details.len() {
            return Err(io::Error::other("The file changed size while it was read."));
        }
        Ok(Self {
            digest: hasher.finalize().into(),
            mode: details.permissions().mode() & MODE_MASK,
            size: details.size(),
        })
    }

    pub fn hex(&self) -> String {
        hex::encode(self.digest)
    }

    /// Whether bytes read afterwards are still the bytes that were proven.
    ///
    /// The closure walk reads a file a second time to find its references, so
    /// it re-checks the digest rather than trusting the file to have held still.
    pub fn digests(&self, bytes: &[u8]) -> bool {
        let digest: [u8; DIGEST_BYTES] = Sha256::digest(bytes).into();
        digest == self.digest
    }

    pub const fn mode(&self) -> u32 {
        self.mode
    }

    pub const fn size(&self) -> u64 {
        self.size
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    const KNOWN: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const BUDGET: u64 = 1024 * 1024;

    fn written(bytes: &[u8]) -> (TempDir, std::path::PathBuf) {
        let directory = TempDir::new().expect("a temporary directory is available");
        let path = directory.path().join("subject");
        fs::write(&path, bytes).expect("the file is written");
        (directory, path)
    }

    #[test]
    fn the_digest_is_sha256_of_the_bytes() {
        let (_directory, path) = written(b"");

        let content = Content::read(&path, BUDGET).expect("the file reads");

        assert_eq!(content.hex(), KNOWN);
        assert_eq!(content.size(), 0);
        assert!(content.digests(b""));
        assert!(!content.digests(b"other"));
    }

    #[test]
    fn a_file_larger_than_its_budget_is_never_read() {
        let (_directory, path) = written(b"a much longer body than the budget allows");

        assert!(Content::read(&path, 4).is_err());
    }

    #[test]
    fn contents_differing_only_in_mode_are_not_the_same_file() {
        let (_directory, path) = written(b"export const seed = 0;\n");
        let before = Content::read(&path, BUDGET).expect("the file reads");

        let mut permissions = path.metadata().expect("the file exists").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("the mode is set");
        let after = Content::read(&path, BUDGET).expect("the file reads again");

        assert_eq!(before.hex(), after.hex());
        assert_ne!(before.mode(), after.mode());
        assert_ne!(before, after);
    }

    #[test]
    fn a_larger_body_than_one_chunk_still_digests_whole() {
        let body = vec![b'z'; READ_CHUNK * 2 + 7];
        let (_directory, path) = written(&body);

        let content = Content::read(&path, BUDGET).expect("the file reads");

        assert_eq!(content.size(), body.len() as u64);
        assert!(content.digests(&body));
    }
}
