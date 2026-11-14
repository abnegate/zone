use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct TreeKey {
    pub(super) owner: String,
    pub(super) repo: String,
    pub(super) branch: String,
    pub(super) fingerprint: Option<[u8; 32]>,
}

impl fmt::Debug for TreeKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TreeKey")
            .field("owner", &self.owner)
            .field("repo", &self.repo)
            .field("branch", &self.branch)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_names_its_tree_but_never_its_fingerprint() {
        let key = TreeKey {
            owner: "acme".to_string(),
            repo: "widgets".to_string(),
            branch: "main".to_string(),
            fingerprint: Some([0xab; 32]),
        };

        let logged = format!("{key:?}");

        for part in ["acme", "widgets", "main"] {
            assert!(logged.contains(part), "{logged}");
        }
        assert!(!logged.contains("fingerprint"), "{logged}");
        assert!(!logged.contains("171"), "{logged}");
    }
}
