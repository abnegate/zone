use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use thiserror::Error;

use super::bounds::ClosureBounds;
use super::content::Content;
use super::roots::{Root, Roots, Tree};
use super::validation::relative_path;

const EXPECTED_LINKS: u64 = 1;

#[derive(Debug, Clone, Eq, PartialEq, Error)]
pub enum ProofError {
    #[error("A proven path must be workspace-relative with no traversal: {path}")]
    UnsafePath { path: String },
    #[error("A proven path may not cross a symlink: {path} in the {tree}")]
    Symlink { path: String, tree: Tree },
    #[error("A proven path must resolve inside its root: {path} in the {tree}")]
    Escaping { path: String, tree: Tree },
    #[error("A proven path must be a regular file: {path} in the {tree}")]
    NotFile { path: String, tree: Tree },
    #[error("A proven path must have exactly one link: {path} in the {tree}")]
    Aliased { path: String, tree: Tree },
    #[error("A proven path could not be read: {path} in the {tree}")]
    Unreadable { path: String, tree: Tree },
}

/// One workspace-relative path, digested in every tree that holds it.
///
/// A tree with no entry means the file is absent there, which is legal: a file
/// the change added is absent from the predecessor. What is never legal is a
/// path that leaves its root, and [`Self::read`] refuses that before it hashes
/// anything.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FileProof {
    path: String,
    contents: BTreeMap<Tree, Content>,
}

impl FileProof {
    pub fn read(roots: &Roots, path: &str, bounds: &ClosureBounds) -> Result<Self, ProofError> {
        if !relative_path(path, bounds.path_bytes) {
            return Err(ProofError::UnsafePath {
                path: path.to_string(),
            });
        }
        let mut contents = BTreeMap::new();
        for root in roots.all() {
            if let Some(content) = Self::within(root, path, bounds)? {
                contents.insert(root.tree(), content);
            }
        }
        Ok(Self {
            path: path.to_string(),
            contents,
        })
    }

    fn within(
        root: &Root,
        path: &str,
        bounds: &ClosureBounds,
    ) -> Result<Option<Content>, ProofError> {
        let mut current = root.path().to_path_buf();
        let mut leaf = None;
        for segment in path.split('/') {
            current.push(segment);
            let Ok(details) = fs::symlink_metadata(&current) else {
                return Ok(None);
            };
            if details.file_type().is_symlink() {
                return Err(ProofError::Symlink {
                    path: path.to_string(),
                    tree: root.tree(),
                });
            }
            leaf = Some(details);
        }
        let details = leaf.ok_or_else(|| ProofError::UnsafePath {
            path: path.to_string(),
        })?;
        if !details.is_file() {
            return Err(ProofError::NotFile {
                path: path.to_string(),
                tree: root.tree(),
            });
        }
        if details.nlink() != EXPECTED_LINKS {
            return Err(ProofError::Aliased {
                path: path.to_string(),
                tree: root.tree(),
            });
        }
        let resolved: PathBuf = fs::canonicalize(&current).map_err(|_| ProofError::Unreadable {
            path: path.to_string(),
            tree: root.tree(),
        })?;
        if !resolved.starts_with(root.path()) {
            return Err(ProofError::Escaping {
                path: path.to_string(),
                tree: root.tree(),
            });
        }
        Content::read(&resolved, bounds.source_bytes)
            .map(Some)
            .map_err(|_| ProofError::Unreadable {
                path: path.to_string(),
                tree: root.tree(),
            })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn content(&self, tree: Tree) -> Option<&Content> {
        self.contents.get(&tree)
    }

    pub fn digest(&self, tree: Tree) -> Option<String> {
        self.contents.get(&tree).map(Content::hex)
    }

    pub fn present(&self, tree: Tree) -> bool {
        self.contents.contains_key(&tree)
    }

    /// Whether the file is present in both trees and byte-for-byte the same.
    pub fn equal(&self, left: Tree, right: Tree) -> bool {
        match (self.contents.get(&left), self.contents.get(&right)) {
            (Some(left), Some(right)) => left == right,
            _ => false,
        }
    }

    /// Whether the file is present in every tree and the same in all of them.
    ///
    /// This is the property a harness file has to hold. Anything less means the
    /// apparatus doing the checking is not the same apparatus on both sides.
    pub fn identical(&self) -> bool {
        Tree::ALL
            .iter()
            .all(|tree| self.equal(Tree::TargetSnapshot, *tree))
    }

    /// Whether one execution root matches the snapshot it was built from.
    ///
    /// A file absent from a snapshot must also be absent from its execution
    /// root; a runner that materialises something the snapshot does not describe
    /// is not running the change under review.
    pub fn materialised(&self, execution: Tree) -> bool {
        let snapshot = match execution {
            Tree::TargetExecution => Tree::TargetSnapshot,
            Tree::PredecessorExecution => Tree::PredecessorSnapshot,
            other => other,
        };
        self.agrees(snapshot, execution)
    }

    pub fn faithful(&self) -> bool {
        self.materialised(Tree::TargetExecution) && self.materialised(Tree::PredecessorExecution)
    }

    /// Whether the file differs at all between predecessor and target, counting
    /// an addition or a removal as a difference.
    pub fn changed(&self) -> bool {
        !self.agrees(Tree::PredecessorSnapshot, Tree::TargetSnapshot)
    }

    fn agrees(&self, snapshot: Tree, execution: Tree) -> bool {
        match (self.contents.get(&snapshot), self.contents.get(&execution)) {
            (None, None) => true,
            (Some(left), Some(right)) => left == right,
            _ => false,
        }
    }

    /// The trees this path is missing from, named for a refusal message.
    pub fn absent(&self) -> Vec<Tree> {
        Tree::ALL
            .into_iter()
            .filter(|tree| !self.contents.contains_key(tree))
            .collect()
    }

    pub fn size(&self) -> u64 {
        Tree::ALL
            .iter()
            .filter_map(|tree| self.contents.get(tree))
            .map(Content::size)
            .max()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::super::scaffold::Workspace;
    use super::*;

    const PATH: &str = "src/cart.mjs";
    const BOUNDS: ClosureBounds = ClosureBounds::DEFAULT;

    fn read(workspace: &Workspace, path: &str) -> Result<FileProof, ProofError> {
        FileProof::read(&workspace.roots(), path, &BOUNDS)
    }

    #[test]
    fn a_file_the_change_left_alone_is_identical_everywhere() {
        let workspace = Workspace::new();
        workspace.unchanged(PATH, "export const total = () => 0;\n");

        let proof = read(&workspace, PATH).expect("the file proves");

        assert!(proof.identical());
        assert!(proof.faithful());
        assert!(!proof.changed());
        assert!(proof.absent().is_empty());
        assert_eq!(proof.path(), PATH);
        assert_eq!(
            proof.digest(Tree::TargetSnapshot),
            proof.digest(Tree::PredecessorSnapshot)
        );
    }

    #[test]
    fn a_file_the_change_edited_is_changed_but_still_faithful() {
        let workspace = Workspace::new();
        workspace.predecessor(PATH, "export const total = () => 0;\n");
        workspace.target(PATH, "export const total = () => 1;\n");

        let proof = read(&workspace, PATH).expect("the file proves");

        assert!(!proof.identical());
        assert!(proof.faithful());
        assert!(proof.changed());
        assert!(!proof.equal(Tree::PredecessorSnapshot, Tree::TargetSnapshot));
        assert!(proof.equal(Tree::TargetSnapshot, Tree::TargetExecution));
    }

    #[test]
    fn a_file_the_change_added_is_absent_from_the_predecessor() {
        let workspace = Workspace::new();
        workspace.target(PATH, "export const total = () => 1;\n");

        let proof = read(&workspace, PATH).expect("the file proves");

        assert!(!proof.identical());
        assert!(proof.changed());
        assert!(proof.faithful());
        assert!(proof.present(Tree::TargetSnapshot));
        assert!(!proof.present(Tree::PredecessorSnapshot));
        assert_eq!(
            proof.absent(),
            [Tree::PredecessorSnapshot, Tree::PredecessorExecution]
        );
    }

    #[test]
    fn an_execution_root_that_invents_a_file_is_not_materialised() {
        let workspace = Workspace::new();
        workspace.only(
            Tree::TargetExecution,
            PATH,
            "export const total = () => 9;\n",
        );

        let proof = read(&workspace, PATH).expect("the file proves");

        assert!(!proof.materialised(Tree::TargetExecution));
        assert!(proof.materialised(Tree::PredecessorExecution));
        assert!(!proof.faithful());
    }

    #[test]
    fn a_file_absent_everywhere_proves_nothing_rather_than_failing() {
        let workspace = Workspace::new();

        let proof = read(&workspace, PATH).expect("an absent file is not an error");

        assert_eq!(proof.absent().len(), Tree::ALL.len());
        assert!(!proof.identical());
        assert!(!proof.changed());
        assert_eq!(proof.size(), 0);
    }

    #[test]
    fn a_path_that_escapes_its_root_never_reaches_the_filesystem() {
        let workspace = Workspace::new();

        for path in [
            "../cart.mjs",
            "/etc/passwd",
            "src/../../cart.mjs",
            "src/./cart.mjs",
            "",
        ] {
            assert_eq!(
                read(&workspace, path),
                Err(ProofError::UnsafePath {
                    path: path.to_string()
                }),
                "{path} is refused"
            );
        }
    }

    #[test]
    fn a_path_longer_than_its_budget_is_refused() {
        let workspace = Workspace::new();
        let bounds = ClosureBounds {
            path_bytes: 4,
            ..ClosureBounds::DEFAULT
        };

        assert_eq!(
            FileProof::read(&workspace.roots(), PATH, &bounds),
            Err(ProofError::UnsafePath {
                path: PATH.to_string()
            })
        );
    }

    #[test]
    fn a_directory_where_a_file_was_expected_is_refused() {
        let workspace = Workspace::new();
        workspace.unchanged("src/cart.mjs/inner.mjs", "export const inner = 1;\n");

        assert_eq!(
            read(&workspace, PATH),
            Err(ProofError::NotFile {
                path: PATH.to_string(),
                tree: Tree::PredecessorSnapshot,
            })
        );
    }
}
