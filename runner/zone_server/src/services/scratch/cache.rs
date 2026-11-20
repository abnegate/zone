//! Cargo build directories an agent's commands left directly under the
//! host's temporary directory, by naming them there itself
//! (`CARGO_TARGET_DIR=/tmp/zone-build`) rather than using the scratch its run
//! was given. Nothing else removes them, and a few runs of a large workspace
//! fill the disk.
//!
//! Only a directory cargo tagged as its own cache is ever taken, and only
//! once nothing has written to it for [`IDLE`]: a cache directory tag says
//! the directory holds nothing that cannot be built again, and the idle
//! window keeps a build that is still using one from losing it. The sweep
//! looks at nothing but the temporary directory's own entries, so the roots
//! Zone keeps there — checkouts, clones, agent state — are never descended
//! into.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The file a cache directory is tagged with.
const TAG: &str = "CACHEDIR.TAG";

/// The line every cache directory tag starts with.
const SIGNATURE: &str = "Signature: 8a477f597d28d172789f06886806bc55";

/// The line cargo writes into the tag of a directory it builds in.
const CARGO: &str = "# This file is a cache directory tag created by cargo.";

/// How long a cache must go unwritten before it is taken.
pub const IDLE: Duration = Duration::from_secs(24 * 60 * 60);

pub struct Cache {
    path: PathBuf,
}

impl Cache {
    /// Remove every cargo cache directly under `directory` that nothing has
    /// written to in [`IDLE`] as of `now`, and say how many went. One that
    /// cannot be read or removed is logged and left for the next sweep.
    pub fn sweep(directory: &Path, now: SystemTime) -> std::io::Result<u64> {
        let mut removed = 0;
        for cache in Self::find(directory)? {
            match cache.idle(now) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) => {
                    tracing::warn!(path = %cache.path.display(), %error, "Cannot tell when a build cache was last used; leaving it");
                    continue;
                }
            }
            match std::fs::remove_dir_all(&cache.path) {
                Ok(()) => {
                    removed += 1;
                    tracing::info!(path = %cache.path.display(), "Removed a build cache an agent left in the temporary directory");
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    tracing::warn!(path = %cache.path.display(), %error, "Could not remove an idle build cache; will retry")
                }
            }
        }
        Ok(removed)
    }

    /// The directories directly under `directory` that are this user's own,
    /// not links, and tagged as cargo's cache.
    fn find(directory: &Path) -> std::io::Result<Vec<Self>> {
        let mut caches = Vec::new();
        for entry in std::fs::read_dir(directory)? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.is_dir() && Self::owned(&metadata) && Self::tagged(&path) {
                caches.push(Self { path });
            }
        }
        Ok(caches)
    }

    #[cfg(unix)]
    fn owned(metadata: &std::fs::Metadata) -> bool {
        use std::os::unix::fs::MetadataExt;
        metadata.uid() == nix::unistd::geteuid().as_raw()
    }

    #[cfg(not(unix))]
    fn owned(_: &std::fs::Metadata) -> bool {
        true
    }

    fn tagged(path: &Path) -> bool {
        let tag = path.join(TAG);
        if !std::fs::symlink_metadata(&tag).is_ok_and(|metadata| metadata.is_file()) {
            return false;
        }
        std::fs::read_to_string(tag).is_ok_and(|tag| {
            let mut lines = tag.lines();
            lines.next() == Some(SIGNATURE) && lines.any(|line| line == CARGO)
        })
    }

    /// Whether nothing has been written in the cache for [`IDLE`]. A build
    /// writes into the profile directories (`debug/deps`,
    /// `release/.fingerprint`) and into `build/` beside them, so the newest
    /// change among the cache, its entries and theirs is when it was last
    /// used.
    fn idle(&self, now: SystemTime) -> std::io::Result<bool> {
        let mut used = std::fs::symlink_metadata(&self.path)?.modified()?;
        for entry in std::fs::read_dir(&self.path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            used = used.max(metadata.modified()?);
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                continue;
            }
            for inner in std::fs::read_dir(entry.path())? {
                used = used.max(inner?.metadata()?.modified()?);
            }
        }
        Ok(now
            .duration_since(used)
            .is_ok_and(|elapsed| elapsed >= IDLE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARGO_TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n\
        # This file is a cache directory tag created by cargo.\n\
        # For information about cache directory tags see https://bford.info/cachedir/\n";

    /// A target directory the way cargo leaves one after a debug build.
    fn target(path: &Path) -> PathBuf {
        let deps = path.join("debug").join("deps");
        std::fs::create_dir_all(&deps).unwrap();
        std::fs::write(deps.join("libzone.rlib"), "artifact").unwrap();
        std::fs::write(path.join(TAG), CARGO_TAG).unwrap();
        path.to_path_buf()
    }

    /// A day and a minute from now, when everything written just now is idle.
    fn later() -> SystemTime {
        SystemTime::now() + IDLE + Duration::from_secs(60)
    }

    #[test]
    fn an_idle_cargo_cache_is_removed_and_a_busy_one_kept() {
        let temporary = tempfile::tempdir().unwrap();
        let build = target(&temporary.path().join("zone-build"));

        assert_eq!(
            Cache::sweep(temporary.path(), SystemTime::now()).unwrap(),
            0
        );
        assert!(build.exists(), "a cache written just now is in use");

        assert_eq!(Cache::sweep(temporary.path(), later()).unwrap(), 1);
        assert!(!build.exists(), "an idle cache was left to fill the disk");
    }

    #[test]
    fn a_write_deep_in_the_profile_keeps_the_cache() {
        let temporary = tempfile::tempdir().unwrap();
        let build = target(&temporary.path().join("zone-target2"));
        let deps = build.join("debug").join("deps");
        let fresh = SystemTime::now() + IDLE;
        std::fs::File::options()
            .write(true)
            .open(deps.join("libzone.rlib"))
            .unwrap()
            .set_modified(fresh)
            .unwrap();
        std::fs::File::open(&deps)
            .unwrap()
            .set_modified(fresh)
            .unwrap();

        assert_eq!(Cache::sweep(temporary.path(), later()).unwrap(), 0);
        assert!(build.exists());
    }

    #[test]
    fn nothing_but_cargo_s_own_tagged_caches_is_taken() {
        let temporary = tempfile::tempdir().unwrap();
        let untagged = temporary.path().join("notes");
        std::fs::create_dir(&untagged).unwrap();
        std::fs::write(untagged.join("draft.md"), "kept").unwrap();
        let foreign = temporary.path().join("other-cache");
        std::fs::create_dir(&foreign).unwrap();
        std::fs::write(
            foreign.join(TAG),
            "Signature: 8a477f597d28d172789f06886806bc55\n# created by another tool\n",
        )
        .unwrap();
        let forged = temporary.path().join("forged");
        std::fs::create_dir(&forged).unwrap();
        std::fs::write(
            forged.join(TAG),
            "# This file is a cache directory tag created by cargo.\n",
        )
        .unwrap();
        std::fs::write(temporary.path().join("loose.txt"), "kept").unwrap();

        assert_eq!(Cache::sweep(temporary.path(), later()).unwrap(), 0);
        for path in [&untagged, &foreign, &forged] {
            assert!(path.exists(), "{} was removed", path.display());
        }
    }

    /// The roots Zone keeps in the temporary directory hold cargo output a
    /// run made inside them; the sweep looks only at the temporary
    /// directory's own entries and never descends into them.
    #[test]
    fn zone_s_own_roots_are_never_touched() {
        let temporary = tempfile::tempdir().unwrap();
        let mut kept = Vec::new();
        for root in [
            "zone-checkouts-v1-0123456789abcdef",
            "repos",
            "zone-clones",
            "zone-agents",
        ] {
            let root = temporary.path().join(root);
            let nested = target(&root.join("run").join("target"));
            std::fs::write(root.join("state"), "kept").unwrap();
            kept.push(root);
            kept.push(nested);
        }

        assert_eq!(Cache::sweep(temporary.path(), later()).unwrap(), 0);
        for path in &kept {
            assert!(path.exists(), "{} was removed", path.display());
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_cache_is_never_followed() {
        let temporary = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let build = target(&elsewhere.path().join("target"));
        std::os::unix::fs::symlink(&build, temporary.path().join("zone-build")).unwrap();

        assert_eq!(Cache::sweep(temporary.path(), later()).unwrap(), 0);
        assert!(
            build
                .join("debug")
                .join("deps")
                .join("libzone.rlib")
                .exists()
        );
    }
}
