//! A path no file tool reaches, as the filesystem knows it.

use std::ffi::OsStr;
use std::path::Path;

use super::file::resolve;
use super::identity::Identity;

/// A path no file tool reaches, as it stands on disk: the identity of the
/// deepest part of it that exists, and the names below that part that do not
/// exist yet.
///
/// Until a name exists it has no identity to compare, so those names are
/// compared instead, folded the way a filesystem that ignores case compares
/// them. Refusing a name that differs only in case costs nothing on a
/// filesystem that tells the two apart.
pub(super) struct Denied {
    existing: Identity,
    missing: Vec<String>,
}

impl Denied {
    pub(super) fn of(path: &Path) -> Option<Self> {
        if let Some(existing) = Identity::of(path) {
            return Some(Self {
                existing,
                missing: Vec::new(),
            });
        }
        let resolved = resolve(path);
        let mut missing = Vec::new();
        for ancestor in resolved.ancestors() {
            if let Some(existing) = Identity::of(ancestor) {
                missing.reverse();
                return Some(Self { existing, missing });
            }
            missing.extend(ancestor.file_name().map(folded));
        }
        None
    }

    /// Whether the names `below` a directory whose identity is `directory`
    /// lead into this path.
    pub(super) fn covers(&self, directory: Identity, below: &Path) -> bool {
        let mut names = below
            .components()
            .map(|component| folded(component.as_os_str()));
        directory == self.existing
            && self
                .missing
                .iter()
                .all(|missing| names.next().is_some_and(|name| name == *missing))
    }
}

fn folded(name: &OsStr) -> String {
    name.to_string_lossy().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn a_path_that_exists_covers_everything_below_it_and_nothing_beside_it() {
        let directory = tempdir().unwrap();
        let state = directory.path().join("agent-state");
        std::fs::create_dir(&state).unwrap();
        let denied = Denied::of(&state).expect("a directory that exists");
        let identity = Identity::of(&state).unwrap();

        assert!(denied.covers(identity, Path::new("")));
        assert!(denied.covers(identity, Path::new("organization/codex/auth.json")));
        assert!(!denied.covers(Identity::of(directory.path()).unwrap(), Path::new("")));
    }

    #[test]
    fn a_path_yet_to_be_made_covers_only_what_lies_under_the_names_it_will_have() {
        let directory = tempdir().unwrap();
        let denied =
            Denied::of(&directory.path().join("zone/agents")).expect("a directory above it");
        let above = Identity::of(directory.path()).unwrap();

        assert!(denied.covers(above, Path::new("zone/agents")));
        assert!(denied.covers(above, Path::new("ZONE/Agents/organization/codex/auth.json")));
        assert!(
            !denied.covers(above, Path::new("zone")),
            "the directory it will be made in is not under it"
        );
        assert!(!denied.covers(above, Path::new("zone/agents-notes")));
        assert!(
            !denied.covers(
                Identity::of(Path::new("/")).unwrap(),
                Path::new("zone/agents")
            ),
            "the same names under another directory are another path"
        );
    }
}
