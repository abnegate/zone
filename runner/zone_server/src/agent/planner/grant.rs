//! Proof, carried by the model from `create_repository` to `finalize_project`, that a source's
//! credential created a repository, so the project may keep that credential for it.

use abnegate_secret::MasterKey;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use uuid::Uuid;
use zeroize::Zeroizing;

const PURPOSE: &[u8] = b"zone planner repository grant";
const SEPARATOR: &[u8] = &[0];

type Signer = Hmac<Sha256>;

pub(super) struct Grant {
    key: Zeroizing<[u8; 32]>,
    workspace: Uuid,
    source: Uuid,
}

impl Grant {
    pub(super) fn new(master_key: &MasterKey, workspace: Uuid, source: Uuid) -> Self {
        let mut purpose = Signer::new_from_slice(master_key.to_hexadecimal().as_bytes())
            .expect("HMAC takes any key");
        purpose.update(PURPOSE);
        Self {
            key: Zeroizing::new(purpose.finalize().into_bytes().into()),
            workspace,
            source,
        }
    }

    /// The grant for `repository`, a normalised repository identity.
    pub(super) fn issue(&self, repository: &str) -> String {
        hex::encode(self.signer(repository).finalize().into_bytes())
    }

    /// Whether `presented` is the grant this workspace's source was issued for `repository`.
    pub(super) fn admits(&self, repository: &str, presented: &str) -> bool {
        hex::decode(presented.trim())
            .is_ok_and(|tag| self.signer(repository).verify_slice(&tag).is_ok())
    }

    fn signer(&self, repository: &str) -> Signer {
        let mut signer = Signer::new_from_slice(self.key.as_slice()).expect("HMAC takes any key");
        signer.update(self.workspace.as_bytes());
        signer.update(SEPARATOR);
        signer.update(self.source.as_bytes());
        signer.update(SEPARATOR);
        signer.update(repository.as_bytes());
        signer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPOSITORY: &str = "https://github.com/acme/shop.git";

    fn key() -> MasterKey {
        MasterKey::new([7; 32])
    }

    #[test]
    fn a_grant_admits_only_the_repository_workspace_and_source_it_was_issued_for() {
        let workspace = Uuid::new_v4();
        let source = Uuid::new_v4();
        let grant = Grant::new(&key(), workspace, source).issue(REPOSITORY);

        assert!(Grant::new(&key(), workspace, source).admits(REPOSITORY, &grant));
        assert!(
            !Grant::new(&key(), workspace, source)
                .admits("https://github.com/acme/other.git", &grant)
        );
        assert!(!Grant::new(&key(), Uuid::new_v4(), source).admits(REPOSITORY, &grant));
        assert!(!Grant::new(&key(), workspace, Uuid::new_v4()).admits(REPOSITORY, &grant));
        assert!(
            !Grant::new(&MasterKey::new([8; 32]), workspace, source).admits(REPOSITORY, &grant)
        );
    }

    #[test]
    fn a_malformed_grant_admits_nothing() {
        let grant = Grant::new(&key(), Uuid::new_v4(), Uuid::new_v4());

        assert!(!grant.admits(REPOSITORY, ""));
        assert!(!grant.admits(REPOSITORY, "not hex"));
        assert!(!grant.admits(REPOSITORY, "00"));
    }
}
