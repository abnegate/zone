use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct TreeKey {
    pub(super) instance: String,
    pub(super) project: String,
    pub(super) branch: String,
    pub(super) path: String,
    pub(super) fingerprint: Option<[u8; 32]>,
}

impl fmt::Debug for TreeKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TreeKey")
            .field("instance", &self.instance)
            .field("project", &self.project)
            .field("branch", &self.branch)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_names_its_tree_but_never_its_fingerprint() {
        let key = TreeKey {
            instance: "https://gitlab.example.com".to_string(),
            project: "group/project".to_string(),
            branch: "main".to_string(),
            path: "src".to_string(),
            fingerprint: Some([0xab; 32]),
        };

        let logged = format!("{key:?}");

        for part in ["https://gitlab.example.com", "group/project", "main", "src"] {
            assert!(logged.contains(part), "{logged}");
        }
        assert!(!logged.contains("fingerprint"), "{logged}");
        assert!(!logged.contains("171"), "{logged}");
    }
}
