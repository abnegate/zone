use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use thiserror::Error;

use super::bounds::ClosureBounds;
use super::proof::{FileProof, ProofError};
use super::references::{Reference, references};
use super::roots::{Root, Roots, Tree};
use super::surface::Surface;
use super::validation::relative_path;

const WALKED: [Tree; 2] = [Tree::TargetSnapshot, Tree::PredecessorSnapshot];
const EXECUTED: [Tree; 2] = [Tree::TargetExecution, Tree::PredecessorExecution];

#[derive(Debug, Clone, Eq, PartialEq, Error)]
pub enum ClosureError {
    #[error("A nominated entrypoint must be workspace-relative with no traversal: {path}")]
    UnsafeEntrypoint { path: String },
    #[error("A nominated entrypoint must be a harness path: {path}")]
    NotHarness { path: String },
    #[error("A nominated entrypoint does not exist: {path}")]
    EntrypointMissing { path: String },
    #[error("{referrer} references {specifier}, which resolves to nothing in the {tree}.")]
    Unresolved {
        referrer: String,
        specifier: String,
        tree: Tree,
    },
    #[error(
        "The {tree} does not match its snapshot for {path}, so the closure is not what would run."
    )]
    Unfaithful { path: String, tree: Tree },
    #[error("The closure reaches more than {limit} files.")]
    TooManyFiles { limit: usize },
    #[error("The closure reaches more than {limit} bytes.")]
    TooManyBytes { limit: u64 },
    #[error("A file in the closure changed while it was being proven: {path}")]
    Unstable { path: String },
    #[error("The closure contains no changed product file, so it proves nothing about the change.")]
    NoProduct,
    #[error(transparent)]
    Proof(#[from] ProofError),
}

/// A harness file that is not the same on both sides of the change.
///
/// Naming it is the whole point of the refusal: the nomination is rejected
/// because *this* file could have been edited to make the check pass.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Divergence {
    path: String,
    absent: Vec<Tree>,
}

impl Divergence {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn absent(&self) -> &[Tree] {
        &self.absent
    }

    pub fn message(&self) -> String {
        match self.absent.first() {
            Some(tree) => format!(
                "The harness file {} is absent from the {}, so the change brought its own check and cannot be proven by it.",
                self.path, tree
            ),
            None => format!(
                "The harness file {} differs between the predecessor and the target, so the change could have edited its own check.",
                self.path
            ),
        }
    }
}

/// Evidence that a harness closure held.
///
/// It has no public constructor and borrows the proof it came from, so an
/// authoritative verification outcome cannot be minted without a
/// [`ClosureProof`] that actually held.
#[derive(Debug, Clone, Copy)]
pub struct ProvenClosure<'proof> {
    proof: &'proof ClosureProof,
}

impl<'proof> ProvenClosure<'proof> {
    pub const fn proof(self) -> &'proof ClosureProof {
        self.proof
    }
}

/// The transitive file closure of a nominated entrypoint, proven across every
/// tree it would run in.
///
/// Every harness file in the closure has to be byte-identical between the
/// predecessor and the target. Only files in [`Self::product`] — the ones that
/// actually differ — may change. That is what makes it impossible for a change
/// to make its own test pass.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ClosureProof {
    entrypoint: String,
    harness: Vec<FileProof>,
    product: BTreeSet<PathBuf>,
    divergent: Option<Divergence>,
}

impl ClosureProof {
    pub fn entrypoint(&self) -> &str {
        &self.entrypoint
    }

    pub fn harness(&self) -> &[FileProof] {
        &self.harness
    }

    pub const fn product(&self) -> &BTreeSet<PathBuf> {
        &self.product
    }

    /// Whether every harness file in the closure is the same on both sides.
    ///
    /// A derived answer rather than a stored flag, so a proof cannot be built
    /// claiming to have held when it did not.
    pub const fn identical(&self) -> bool {
        self.divergent.is_none()
    }

    pub const fn divergent(&self) -> Option<&Divergence> {
        self.divergent.as_ref()
    }

    pub fn refusal(&self) -> Option<String> {
        self.divergent.as_ref().map(Divergence::message)
    }

    /// The witness that lets an authoritative outcome be minted, available only
    /// when the closure actually held.
    pub const fn witness(&self) -> Option<ProvenClosure<'_>> {
        if self.divergent.is_some() {
            return None;
        }
        Some(ProvenClosure { proof: self })
    }
}

/// Walk the closure reachable from a nominated entrypoint and apply the rule.
pub fn prove(
    roots: &Roots,
    entrypoint: &str,
    bounds: &ClosureBounds,
) -> Result<ClosureProof, ClosureError> {
    if !relative_path(entrypoint, bounds.path_bytes) {
        return Err(ClosureError::UnsafeEntrypoint {
            path: entrypoint.to_string(),
        });
    }
    if !Surface::of(entrypoint).harness() {
        return Err(ClosureError::NotHarness {
            path: entrypoint.to_string(),
        });
    }

    let root = FileProof::read(roots, entrypoint, bounds)?;
    if root.absent().len() == Tree::ALL.len() {
        return Err(ClosureError::EntrypointMissing {
            path: entrypoint.to_string(),
        });
    }
    if !root.identical() {
        return Ok(refused(entrypoint, divergence(&root), vec![root]));
    }

    let mut proofs: BTreeMap<String, FileProof> = BTreeMap::new();
    proofs.insert(entrypoint.to_string(), root);
    walk(roots, entrypoint, bounds, &mut proofs)?;
    partition(entrypoint, proofs)
}

fn walk(
    roots: &Roots,
    entrypoint: &str,
    bounds: &ClosureBounds,
    proofs: &mut BTreeMap<String, FileProof>,
) -> Result<(), ClosureError> {
    let mut total: u64 = proofs.values().map(FileProof::size).sum();
    let mut pending: Vec<(Tree, String)> = WALKED
        .into_iter()
        .map(|tree| (tree, entrypoint.to_string()))
        .collect();
    let mut seen: BTreeSet<(Tree, String)> = BTreeSet::new();

    while let Some((tree, path)) = pending.pop() {
        if !seen.insert((tree, path.clone())) {
            continue;
        }
        let Some(source) = read(roots.root(tree), &path, bounds, proofs.get(&path))? else {
            continue;
        };
        for reference in references(&path, &source) {
            match resolve(roots, tree, &reference, bounds, proofs, &mut total)? {
                Resolution::Walked(resolved) => pending.push((tree, resolved)),
                Resolution::Recorded => {}
                Resolution::Missing => {
                    return Err(ClosureError::Unresolved {
                        referrer: path.clone(),
                        specifier: reference.specifier().to_string(),
                        tree,
                    });
                }
            }
        }
    }
    Ok(())
}

/// What became of one static reference while walking one tree.
enum Resolution {
    /// Present in the tree being walked, so its own references are next.
    Walked(String),
    /// Present in another tree but not this one. It is proven and recorded, and
    /// the rule decides what its absence means — there is nothing here to read.
    Recorded,
    /// Present in no tree at all, which leaves a hole in the closure.
    Missing,
}

/// Take the first candidate for a reference that exists in any tree, proving it
/// across every root on the way in.
fn resolve(
    roots: &Roots,
    tree: Tree,
    reference: &Reference,
    bounds: &ClosureBounds,
    proofs: &mut BTreeMap<String, FileProof>,
    total: &mut u64,
) -> Result<Resolution, ClosureError> {
    for candidate in reference.candidates() {
        if !relative_path(candidate, bounds.path_bytes) {
            continue;
        }
        if let Some(known) = proofs.get(candidate) {
            return Ok(settled(known, tree, candidate));
        }
        let proof = FileProof::read(roots, candidate, bounds)?;
        if proof.absent().len() == Tree::ALL.len() {
            continue;
        }
        if proofs.len() >= bounds.files {
            return Err(ClosureError::TooManyFiles {
                limit: bounds.files,
            });
        }
        *total = total.saturating_add(proof.size());
        if *total > bounds.total_bytes {
            return Err(ClosureError::TooManyBytes {
                limit: bounds.total_bytes,
            });
        }
        let resolution = settled(&proof, tree, candidate);
        proofs.insert(candidate.clone(), proof);
        return Ok(resolution);
    }
    Ok(Resolution::Missing)
}

fn settled(proof: &FileProof, tree: Tree, candidate: &str) -> Resolution {
    if proof.present(tree) {
        return Resolution::Walked(candidate.to_string());
    }
    Resolution::Recorded
}

/// Read a file back for reference extraction, re-checking it against the digest
/// that was already proven.
fn read(
    root: &Root,
    path: &str,
    bounds: &ClosureBounds,
    proof: Option<&FileProof>,
) -> Result<Option<String>, ClosureError> {
    let Some(content) = proof.and_then(|proof| proof.content(root.tree())) else {
        return Ok(None);
    };
    if content.size() > bounds.source_bytes {
        return Ok(None);
    }
    let mut full = root.path().to_path_buf();
    for segment in path.split('/') {
        full.push(segment);
    }
    let Ok(bytes) = fs::read(&full) else {
        return Err(ClosureError::Unstable {
            path: path.to_string(),
        });
    };
    if !content.digests(&bytes) {
        return Err(ClosureError::Unstable {
            path: path.to_string(),
        });
    }
    Ok(String::from_utf8(bytes).ok())
}

fn partition(
    entrypoint: &str,
    proofs: BTreeMap<String, FileProof>,
) -> Result<ClosureProof, ClosureError> {
    let mut harness: Vec<FileProof> = Vec::new();
    let mut product: BTreeSet<PathBuf> = BTreeSet::new();

    for (path, proof) in proofs {
        if Surface::of(&path).harness() {
            let divergent = (!proof.identical()).then(|| divergence(&proof));
            harness.push(proof);
            if let Some(divergent) = divergent {
                return Ok(refused(entrypoint, divergent, harness));
            }
            continue;
        }
        for execution in EXECUTED {
            if !proof.materialised(execution) {
                return Err(ClosureError::Unfaithful {
                    path,
                    tree: execution,
                });
            }
        }
        if proof.changed() {
            product.insert(PathBuf::from(path));
        }
    }

    if product.is_empty() {
        return Err(ClosureError::NoProduct);
    }
    Ok(ClosureProof {
        entrypoint: entrypoint.to_string(),
        harness,
        product,
        divergent: None,
    })
}

fn refused(entrypoint: &str, divergent: Divergence, harness: Vec<FileProof>) -> ClosureProof {
    ClosureProof {
        entrypoint: entrypoint.to_string(),
        harness,
        product: BTreeSet::new(),
        divergent: Some(divergent),
    }
}

fn divergence(proof: &FileProof) -> Divergence {
    Divergence {
        path: proof.path().to_string(),
        absent: proof.absent(),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::super::scaffold::{Workspace, holding};
    use super::*;

    const ENTRYPOINT: &str = "tests/checkout.test.mjs";

    #[test]
    fn a_closure_over_identical_harness_trees_proves() {
        let workspace = holding();
        let proof = workspace.prove(ENTRYPOINT).expect("the closure proves");

        assert!(proof.identical(), "{:?}", proof.refusal());
        assert!(proof.refusal().is_none());
        assert_eq!(proof.entrypoint(), ENTRYPOINT);
        assert_eq!(paths(&proof), vec![ENTRYPOINT]);
        assert_eq!(
            proof.product(),
            &BTreeSet::from([PathBuf::from("src/cart.mjs")])
        );
        assert!(proof.witness().is_some());
    }

    #[test]
    fn a_harness_file_that_differs_refuses_the_nomination_and_names_it() {
        let workspace = Workspace::new();
        workspace.unchanged(
            ENTRYPOINT,
            "import { seed } from './helper.mjs';\nexport const run = () => seed();\n",
        );
        workspace.predecessor("tests/helper.mjs", "export const seed = () => 0;\n");
        workspace.target("tests/helper.mjs", "export const seed = () => 1;\n");

        let proof = workspace
            .prove(ENTRYPOINT)
            .expect("the closure is evaluated");

        assert!(!proof.identical());
        assert!(proof.witness().is_none());
        let divergence = proof.divergent().expect("the refusal names a file");
        assert_eq!(divergence.path(), "tests/helper.mjs");
        assert!(divergence.absent().is_empty());
        let refusal = proof.refusal().expect("the refusal has a message");
        assert!(
            refusal.contains("tests/helper.mjs"),
            "the refusal names the divergent harness file: {refusal}"
        );
        assert!(proof.product().is_empty());
    }

    #[test]
    fn a_test_the_change_brought_with_it_cannot_prove_the_change() {
        let workspace = Workspace::new();
        workspace.target(ENTRYPOINT, "import '../src/cart.mjs';\n");
        workspace.predecessor("src/cart.mjs", "export const total = () => 0;\n");
        workspace.target("src/cart.mjs", "export const total = () => 1;\n");

        let proof = workspace
            .prove(ENTRYPOINT)
            .expect("the closure is evaluated");

        assert!(!proof.identical());
        assert!(proof.witness().is_none());
        let divergence = proof.divergent().expect("the refusal names a file");
        assert_eq!(divergence.path(), ENTRYPOINT);
        assert_eq!(
            divergence.absent(),
            [Tree::PredecessorSnapshot, Tree::PredecessorExecution]
        );
        let refusal = proof.refusal().expect("the refusal has a message");
        assert!(refusal.contains(ENTRYPOINT), "{refusal}");
        assert!(refusal.contains("brought its own check"), "{refusal}");
    }

    #[test]
    fn a_harness_file_that_differs_only_in_mode_still_refuses() {
        let workspace = holding();
        workspace.unchanged(
            ENTRYPOINT,
            "import './helper.mjs';\nimport '../src/cart.mjs';\n",
        );
        workspace.unchanged("tests/helper.mjs", "export const seed = () => 0;\n");
        executable(
            &workspace
                .root(Tree::TargetSnapshot)
                .join("tests/helper.mjs"),
        );

        let proof = workspace
            .prove(ENTRYPOINT)
            .expect("the closure is evaluated");

        assert!(!proof.identical());
        assert_eq!(
            proof.divergent().map(Divergence::path),
            Some("tests/helper.mjs")
        );
    }

    #[test]
    fn a_product_file_that_differs_does_not_refuse() {
        let workspace = holding();
        workspace.predecessor("src/pricing.mjs", "export const rate = 1;\n");
        workspace.target("src/pricing.mjs", "export const rate = 2;\n");
        workspace.unchanged(
            ENTRYPOINT,
            "import '../src/cart.mjs';\nimport '../src/pricing.mjs';\n",
        );

        let proof = workspace.prove(ENTRYPOINT).expect("the closure proves");

        assert!(proof.identical(), "{:?}", proof.refusal());
        assert_eq!(
            proof.product(),
            &BTreeSet::from([
                PathBuf::from("src/cart.mjs"),
                PathBuf::from("src/pricing.mjs")
            ])
        );
    }

    #[test]
    fn a_fixture_reached_by_the_harness_must_be_identical_too() {
        let workspace = holding();
        workspace.unchanged(
            ENTRYPOINT,
            "import '../src/cart.mjs';\nimport body from './fixtures/order.json';\n",
        );
        workspace.predecessor("tests/fixtures/order.json", "{\"total\":0}\n");
        workspace.target("tests/fixtures/order.json", "{\"total\":1}\n");

        let proof = workspace
            .prove(ENTRYPOINT)
            .expect("the closure is evaluated");

        assert!(!proof.identical());
        assert_eq!(
            proof.divergent().map(Divergence::path),
            Some("tests/fixtures/order.json")
        );
    }

    #[test]
    fn a_symlink_on_the_path_is_refused() {
        let workspace = holding();
        let outside = workspace
            .root(Tree::PredecessorSnapshot)
            .join("src/cart.mjs");
        workspace.unchanged(
            ENTRYPOINT,
            "import '../src/cart.mjs';\nimport './leak.mjs';\n",
        );
        workspace.symlink(Tree::TargetSnapshot, "tests/leak.mjs", &outside);

        assert_eq!(
            workspace.prove(ENTRYPOINT),
            Err(ClosureError::Proof(ProofError::Symlink {
                path: "tests/leak.mjs".to_string(),
                tree: Tree::TargetSnapshot,
            }))
        );
    }

    #[test]
    fn a_symlinked_directory_on_the_path_is_refused() {
        let workspace = holding();
        let outside = workspace.root(Tree::PredecessorSnapshot).join("src");
        workspace.unchanged(
            ENTRYPOINT,
            "import '../src/cart.mjs';\nimport './linked/leak.mjs';\n",
        );
        workspace.symlink(Tree::TargetSnapshot, "tests/linked", &outside);

        assert_eq!(
            workspace.prove(ENTRYPOINT),
            Err(ClosureError::Proof(ProofError::Symlink {
                path: "tests/linked/leak.mjs".to_string(),
                tree: Tree::TargetSnapshot,
            }))
        );
    }

    #[test]
    fn a_symlinked_entrypoint_is_refused_before_anything_is_hashed() {
        let workspace = holding();
        let outside = workspace
            .root(Tree::PredecessorSnapshot)
            .join("src/cart.mjs");
        workspace.symlink(Tree::TargetSnapshot, "tests/alias.test.mjs", &outside);

        assert_eq!(
            workspace.prove("tests/alias.test.mjs"),
            Err(ClosureError::Proof(ProofError::Symlink {
                path: "tests/alias.test.mjs".to_string(),
                tree: Tree::TargetSnapshot,
            }))
        );
    }

    #[test]
    fn a_hard_link_where_a_file_was_expected_is_refused() {
        let workspace = holding();
        let aliased = workspace
            .root(Tree::TargetSnapshot)
            .join("tests/checkout.test.mjs");
        workspace.hardlink(Tree::TargetSnapshot, "tests/alias.mjs", &aliased);

        assert_eq!(
            workspace.prove(ENTRYPOINT),
            Err(ClosureError::Proof(ProofError::Aliased {
                path: ENTRYPOINT.to_string(),
                tree: Tree::TargetSnapshot,
            }))
        );
    }

    #[test]
    fn a_harness_file_the_change_deleted_refuses_the_nomination() {
        let workspace = chain();
        workspace.remove(Tree::TargetSnapshot, "tests/support/two.mjs");
        workspace.remove(Tree::TargetExecution, "tests/support/two.mjs");

        let proof = workspace
            .prove(ENTRYPOINT)
            .expect("the closure is evaluated");

        assert!(!proof.identical());
        let divergence = proof.divergent().expect("the refusal names a file");
        assert_eq!(divergence.path(), "tests/support/two.mjs");
        assert_eq!(
            divergence.absent(),
            [Tree::TargetSnapshot, Tree::TargetExecution]
        );
    }

    #[test]
    fn an_entrypoint_that_escapes_the_root_is_refused() {
        let workspace = holding();

        for entrypoint in [
            "../tests/escape.test.mjs",
            "/etc/passwd",
            "tests/../../escape.test.mjs",
            "tests/./escape.test.mjs",
            "tests\\escape.test.mjs",
        ] {
            assert_eq!(
                workspace.prove(entrypoint),
                Err(ClosureError::UnsafeEntrypoint {
                    path: entrypoint.to_string()
                }),
                "{entrypoint} is refused"
            );
        }
    }

    #[test]
    fn an_entrypoint_that_does_not_exist_is_refused() {
        let workspace = holding();

        assert_eq!(
            workspace.prove("tests/absent.test.mjs"),
            Err(ClosureError::EntrypointMissing {
                path: "tests/absent.test.mjs".to_string()
            })
        );
    }

    #[test]
    fn an_entrypoint_that_is_not_a_harness_path_is_refused() {
        let workspace = holding();

        assert_eq!(
            workspace.prove("src/cart.mjs"),
            Err(ClosureError::NotHarness {
                path: "src/cart.mjs".to_string()
            })
        );
    }

    #[test]
    fn the_file_bound_is_enforced() {
        let workspace = chain();

        assert_eq!(
            workspace.prove_within(
                ENTRYPOINT,
                &ClosureBounds {
                    files: 2,
                    ..ClosureBounds::DEFAULT
                }
            ),
            Err(ClosureError::TooManyFiles { limit: 2 })
        );
        assert!(workspace.prove(ENTRYPOINT).is_ok());
    }

    #[test]
    fn the_byte_bound_is_enforced() {
        let workspace = chain();

        assert_eq!(
            workspace.prove_within(
                ENTRYPOINT,
                &ClosureBounds {
                    total_bytes: 8,
                    ..ClosureBounds::DEFAULT
                }
            ),
            Err(ClosureError::TooManyBytes { limit: 8 })
        );
    }

    #[test]
    fn a_file_over_the_source_bound_is_refused_rather_than_skipped() {
        let workspace = holding();

        assert_eq!(
            workspace.prove_within(
                ENTRYPOINT,
                &ClosureBounds {
                    source_bytes: 4,
                    ..ClosureBounds::DEFAULT
                }
            ),
            Err(ClosureError::Proof(ProofError::Unreadable {
                path: ENTRYPOINT.to_string(),
                tree: Tree::PredecessorSnapshot,
            }))
        );
    }

    #[test]
    fn a_reference_that_resolves_to_nothing_refuses_the_closure() {
        let workspace = holding();
        workspace.unchanged(
            ENTRYPOINT,
            "import '../src/cart.mjs';\nimport './gone.mjs';\n",
        );

        assert_eq!(
            workspace.prove(ENTRYPOINT),
            Err(ClosureError::Unresolved {
                referrer: ENTRYPOINT.to_string(),
                specifier: "./gone.mjs".to_string(),
                tree: Tree::PredecessorSnapshot,
            })
        );
    }

    #[test]
    fn an_execution_root_that_does_not_match_its_snapshot_refuses_the_closure() {
        let workspace = holding();
        workspace.only(
            Tree::TargetExecution,
            "src/cart.mjs",
            "export const total = () => 9;\n",
        );

        assert_eq!(
            workspace.prove(ENTRYPOINT),
            Err(ClosureError::Unfaithful {
                path: "src/cart.mjs".to_string(),
                tree: Tree::TargetExecution,
            })
        );
    }

    #[test]
    fn a_closure_with_nothing_changed_proves_nothing() {
        let workspace = Workspace::new();
        workspace.unchanged(ENTRYPOINT, "import '../src/cart.mjs';\n");
        workspace.unchanged("src/cart.mjs", "export const total = () => 0;\n");

        assert_eq!(workspace.prove(ENTRYPOINT), Err(ClosureError::NoProduct));
    }

    #[test]
    fn the_closure_follows_references_transitively() {
        let workspace = chain();

        let proof = workspace.prove(ENTRYPOINT).expect("the closure proves");

        assert_eq!(
            paths(&proof),
            vec![ENTRYPOINT, "tests/support/one.mjs", "tests/support/two.mjs"]
        );
        assert_eq!(
            proof.product(),
            &BTreeSet::from([PathBuf::from("src/cart.mjs")])
        );
    }

    #[test]
    fn a_harness_file_deep_in_the_chain_still_refuses() {
        let workspace = chain();
        workspace.target("tests/support/two.mjs", "export const two = () => 99;\n");

        let proof = workspace
            .prove(ENTRYPOINT)
            .expect("the closure is evaluated");

        assert!(!proof.identical());
        assert_eq!(
            proof.divergent().map(Divergence::path),
            Some("tests/support/two.mjs")
        );
    }

    #[test]
    fn a_rust_module_declaration_is_a_closure_edge() {
        let workspace = Workspace::new();
        workspace.unchanged("tests/suite.rs", "mod helper;\n");
        workspace.unchanged("tests/suite/helper/mod.rs", "pub mod inner;\n");
        workspace.predecessor("tests/suite/helper/inner.rs", "pub const SEED: u8 = 0;\n");
        workspace.target("tests/suite/helper/inner.rs", "pub const SEED: u8 = 1;\n");

        let proof = workspace
            .prove("tests/suite.rs")
            .expect("the closure is evaluated");

        assert!(!proof.identical());
        assert_eq!(
            proof.divergent().map(Divergence::path),
            Some("tests/suite/helper/inner.rs")
        );
    }

    #[test]
    fn a_php_include_is_a_closure_edge() {
        let workspace = Workspace::new();
        workspace.unchanged(
            "tests/CheckoutTest.php",
            "<?php\nrequire_once __DIR__ . '/../src/Cart.php';\n",
        );
        workspace.predecessor("src/Cart.php", "<?php\nreturn 0;\n");
        workspace.target("src/Cart.php", "<?php\nreturn 1;\n");

        let proof = workspace
            .prove("tests/CheckoutTest.php")
            .expect("the closure proves");

        assert!(proof.identical(), "{:?}", proof.refusal());
        assert_eq!(
            proof.product(),
            &BTreeSet::from([PathBuf::from("src/Cart.php")])
        );
    }

    #[test]
    fn a_binary_file_in_the_closure_is_a_leaf_rather_than_a_hole() {
        let workspace = holding();
        workspace.unchanged(
            ENTRYPOINT,
            "import '../src/cart.mjs';\nimport './fixtures/body.bin';\n",
        );
        workspace.unchanged("tests/fixtures/body.bin", "\u{0}\u{1}\u{2}");

        let proof = workspace.prove(ENTRYPOINT).expect("the closure proves");

        assert!(proof.identical(), "{:?}", proof.refusal());
        assert!(paths(&proof).contains(&"tests/fixtures/body.bin"));
    }

    fn chain() -> Workspace {
        let workspace = holding();
        workspace.unchanged(
            ENTRYPOINT,
            "import '../src/cart.mjs';\nimport './support/one.mjs';\n",
        );
        workspace.unchanged("tests/support/one.mjs", "import './two.mjs';\n");
        workspace.unchanged("tests/support/two.mjs", "export const two = () => 2;\n");
        workspace
    }

    fn paths(proof: &ClosureProof) -> Vec<&str> {
        proof.harness().iter().map(FileProof::path).collect()
    }

    fn executable(path: &Path) {
        let mut permissions = path.metadata().expect("the file exists").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("the mode is set");
    }
}
