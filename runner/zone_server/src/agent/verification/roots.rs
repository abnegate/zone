use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// One of the four trees a harness closure is proven across.
///
/// A snapshot is what the change says the source is. An execution root is what
/// a runner would actually mount. Proving a path in both is what stops a
/// closure being argued from a snapshot that the runner never sees.
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub enum Tree {
    PredecessorSnapshot,
    TargetSnapshot,
    PredecessorExecution,
    TargetExecution,
}

impl Tree {
    pub const ALL: [Self; 4] = [
        Self::PredecessorSnapshot,
        Self::TargetSnapshot,
        Self::PredecessorExecution,
        Self::TargetExecution,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::PredecessorSnapshot => "predecessor snapshot",
            Self::TargetSnapshot => "target snapshot",
            Self::PredecessorExecution => "predecessor execution root",
            Self::TargetExecution => "target execution root",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Error)]
pub enum RootError {
    #[error("The {tree} must be an absolute path: {path}")]
    Relative { tree: Tree, path: PathBuf },
    #[error("The {tree} must not itself be a symlink: {path}")]
    Indirect { tree: Tree, path: PathBuf },
    #[error("The {tree} must be an existing directory: {path}")]
    Missing { tree: Tree, path: PathBuf },
}

impl std::fmt::Display for Tree {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

/// A canonical directory that file proofs are taken against.
///
/// Canonicalising once, up front, is what lets every later containment check be
/// a prefix comparison against a path that cannot move underneath it.
#[derive(Debug, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct Root {
    tree: Tree,
    path: PathBuf,
}

impl Root {
    pub fn open(tree: Tree, path: impl AsRef<Path>) -> Result<Self, RootError> {
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(RootError::Relative {
                tree,
                path: path.to_path_buf(),
            });
        }
        let details = fs::symlink_metadata(path).map_err(|_| RootError::Missing {
            tree,
            path: path.to_path_buf(),
        })?;
        if details.file_type().is_symlink() {
            return Err(RootError::Indirect {
                tree,
                path: path.to_path_buf(),
            });
        }
        if !details.is_dir() {
            return Err(RootError::Missing {
                tree,
                path: path.to_path_buf(),
            });
        }
        let canonical = fs::canonicalize(path).map_err(|_| RootError::Missing {
            tree,
            path: path.to_path_buf(),
        })?;
        Ok(Self {
            tree,
            path: canonical,
        })
    }

    pub const fn tree(&self) -> Tree {
        self.tree
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The four trees a harness closure is proven across.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Roots {
    predecessor_snapshot: Root,
    target_snapshot: Root,
    predecessor_execution: Root,
    target_execution: Root,
}

impl Roots {
    pub fn open(
        predecessor_snapshot: impl AsRef<Path>,
        target_snapshot: impl AsRef<Path>,
        predecessor_execution: impl AsRef<Path>,
        target_execution: impl AsRef<Path>,
    ) -> Result<Self, RootError> {
        Ok(Self {
            predecessor_snapshot: Root::open(Tree::PredecessorSnapshot, predecessor_snapshot)?,
            target_snapshot: Root::open(Tree::TargetSnapshot, target_snapshot)?,
            predecessor_execution: Root::open(Tree::PredecessorExecution, predecessor_execution)?,
            target_execution: Root::open(Tree::TargetExecution, target_execution)?,
        })
    }

    pub const fn all(&self) -> [&Root; 4] {
        [
            &self.predecessor_snapshot,
            &self.target_snapshot,
            &self.predecessor_execution,
            &self.target_execution,
        ]
    }

    pub const fn root(&self, tree: Tree) -> &Root {
        match tree {
            Tree::PredecessorSnapshot => &self.predecessor_snapshot,
            Tree::TargetSnapshot => &self.target_snapshot,
            Tree::PredecessorExecution => &self.predecessor_execution,
            Tree::TargetExecution => &self.target_execution,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs as unix;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn a_root_is_stored_canonical_however_the_caller_spelled_it() {
        let directory = TempDir::new().expect("a temporary directory is available");
        let nested = directory.path().join("snapshot");
        fs::create_dir(&nested).expect("the directory is created");

        let root = Root::open(Tree::TargetSnapshot, &nested).expect("the root opens");

        assert_eq!(root.tree(), Tree::TargetSnapshot);
        assert!(root.path().is_absolute());
        assert_eq!(
            root.path(),
            fs::canonicalize(&nested).expect("the path canonicalizes")
        );
    }

    #[test]
    fn a_relative_root_is_refused() {
        assert_eq!(
            Root::open(Tree::TargetSnapshot, "snapshot"),
            Err(RootError::Relative {
                tree: Tree::TargetSnapshot,
                path: PathBuf::from("snapshot"),
            })
        );
    }

    #[test]
    fn a_symlinked_root_is_refused() {
        let directory = TempDir::new().expect("a temporary directory is available");
        let real = directory.path().join("snapshot");
        let link = directory.path().join("alias");
        fs::create_dir(&real).expect("the directory is created");
        unix::symlink(&real, &link).expect("the symlink is created");

        assert_eq!(
            Root::open(Tree::TargetSnapshot, &link),
            Err(RootError::Indirect {
                tree: Tree::TargetSnapshot,
                path: link,
            })
        );
    }

    #[test]
    fn a_root_that_is_a_file_or_missing_is_refused() {
        let directory = TempDir::new().expect("a temporary directory is available");
        let file = directory.path().join("snapshot");
        fs::write(&file, "not a tree").expect("the file is written");
        let missing = directory.path().join("absent");

        for path in [file, missing] {
            assert_eq!(
                Root::open(Tree::PredecessorSnapshot, &path),
                Err(RootError::Missing {
                    tree: Tree::PredecessorSnapshot,
                    path,
                })
            );
        }
    }

    #[test]
    fn every_tree_resolves_to_its_own_root() {
        let directory = TempDir::new().expect("a temporary directory is available");
        let mut paths = Vec::new();
        for name in ["a", "b", "c", "d"] {
            let path = directory.path().join(name);
            fs::create_dir(&path).expect("the directory is created");
            paths.push(path);
        }
        let roots = Roots::open(&paths[0], &paths[1], &paths[2], &paths[3]).expect("roots open");

        for tree in Tree::ALL {
            assert_eq!(roots.root(tree).tree(), tree);
        }
        assert_eq!(roots.all().len(), Tree::ALL.len());
    }
}
