use std::fs;
use std::os::unix::fs as unix;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::bounds::ClosureBounds;
use super::closure::{ClosureError, ClosureProof, prove};
use super::roots::{Roots, Tree};

const DIRECTORIES: [(Tree, &str); 4] = [
    (Tree::PredecessorSnapshot, "predecessor"),
    (Tree::TargetSnapshot, "target"),
    (Tree::PredecessorExecution, "predecessor-execution"),
    (Tree::TargetExecution, "target-execution"),
];

const PREDECESSOR: [Tree; 2] = [Tree::PredecessorSnapshot, Tree::PredecessorExecution];
const TARGET: [Tree; 2] = [Tree::TargetSnapshot, Tree::TargetExecution];

/// Four trees on disk to prove closures against.
///
/// The helpers name intent rather than directories: [`Self::unchanged`] puts a
/// file in all four trees, [`Self::target`] puts it only on the side the change
/// produced, and [`Self::only`] reaches a single tree so an execution root can
/// be made to disagree with its snapshot.
pub struct Workspace {
    directory: TempDir,
}

impl Workspace {
    pub fn new() -> Self {
        let directory = TempDir::new().expect("a temporary directory is available");
        for (_, name) in DIRECTORIES {
            fs::create_dir_all(directory.path().join(name)).expect("a tree directory is created");
        }
        Self { directory }
    }

    pub fn root(&self, tree: Tree) -> PathBuf {
        let name = DIRECTORIES
            .iter()
            .find_map(|(candidate, name)| (*candidate == tree).then_some(*name))
            .expect("every tree has a directory");
        self.directory.path().join(name)
    }

    pub fn roots(&self) -> Roots {
        Roots::open(
            self.root(Tree::PredecessorSnapshot),
            self.root(Tree::TargetSnapshot),
            self.root(Tree::PredecessorExecution),
            self.root(Tree::TargetExecution),
        )
        .expect("the scaffolded trees open as roots")
    }

    pub fn unchanged(&self, path: &str, content: &str) -> &Self {
        self.write(&Tree::ALL, path, content)
    }

    pub fn predecessor(&self, path: &str, content: &str) -> &Self {
        self.write(&PREDECESSOR, path, content)
    }

    pub fn target(&self, path: &str, content: &str) -> &Self {
        self.write(&TARGET, path, content)
    }

    pub fn only(&self, tree: Tree, path: &str, content: &str) -> &Self {
        self.write(&[tree], path, content)
    }

    pub fn symlink(&self, tree: Tree, path: &str, destination: &Path) -> &Self {
        let full = self.root(tree).join(path);
        Self::parent(&full);
        unix::symlink(destination, &full).expect("a symlink is created");
        self
    }

    pub fn hardlink(&self, tree: Tree, path: &str, destination: &Path) -> &Self {
        let full = self.root(tree).join(path);
        Self::parent(&full);
        fs::hard_link(destination, &full).expect("a hard link is created");
        self
    }

    pub fn remove(&self, tree: Tree, path: &str) -> &Self {
        fs::remove_file(self.root(tree).join(path)).expect("the file is removed");
        self
    }

    fn write(&self, trees: &[Tree], path: &str, content: &str) -> &Self {
        for tree in trees {
            let full = self.root(*tree).join(path);
            Self::parent(&full);
            fs::write(&full, content).expect("a scaffolded file is written");
        }
        self
    }

    fn parent(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("a parent directory is created");
        }
    }

    pub fn prove(&self, entrypoint: &str) -> Result<ClosureProof, ClosureError> {
        prove(&self.roots(), entrypoint, &ClosureBounds::DEFAULT)
    }

    pub fn prove_within(
        &self,
        entrypoint: &str,
        bounds: &ClosureBounds,
    ) -> Result<ClosureProof, ClosureError> {
        prove(&self.roots(), entrypoint, bounds)
    }
}

/// The smallest workspace whose closure holds: an untouched harness reaching a
/// product file the change actually altered.
pub fn holding() -> Workspace {
    let workspace = Workspace::new();
    workspace.unchanged(
        "tests/checkout.test.mjs",
        "import { total } from '../src/cart.mjs';\nexport const run = () => total();\n",
    );
    workspace.predecessor("src/cart.mjs", "export const total = () => 0;\n");
    workspace.target("src/cart.mjs", "export const total = () => 1;\n");
    workspace
}

/// A closure that held, detached from the trees that produced it.
pub fn proven() -> ClosureProof {
    let proof = holding()
        .prove("tests/checkout.test.mjs")
        .expect("the scaffolded closure proves");
    assert!(proof.identical(), "the scaffolded closure holds");
    proof
}
