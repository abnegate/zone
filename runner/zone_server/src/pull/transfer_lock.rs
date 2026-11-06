use std::fs::{File, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

const SUFFIX: &str = ".lock";

/// Sole use of a `.part` file and its validator, taken the way abnegate-llm
/// takes it for a transfer, so the two keep each other off the same files.
///
/// The crate does not export its lock, so this mirrors its protocol: an
/// advisory lock on the part's `.lock` sibling, whose file is removed while
/// still held, and a lock that only counts if the path still names the file
/// it was taken on.
#[derive(Debug)]
pub(super) struct TransferLock {
    path: PathBuf,
    _file: File,
}

impl TransferLock {
    pub(super) fn path(part: &Path) -> PathBuf {
        let mut name = part.as_os_str().to_owned();
        name.push(SUFFIX);
        PathBuf::from(name)
    }

    /// The lock for `part`, or `None` while a transfer holds it.
    pub(super) fn try_acquire(part: &Path) -> io::Result<Option<Self>> {
        let path = Self::path(part);
        loop {
            let file = File::options()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)?;
            match file.try_lock() {
                Ok(()) => {}
                Err(TryLockError::WouldBlock) => return Ok(None),
                Err(TryLockError::Error(error)) => return Err(error),
            }
            if names(&file, &path) {
                return Ok(Some(Self { path, _file: file }));
            }
        }
    }
}

impl Drop for TransferLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Whether `path` still names `file`: a holder removes the lock file before
/// unlocking it, so a lock won on a file opened just before that removal is
/// a lock no other transfer will ever see.
#[cfg(unix)]
fn names(file: &File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    match (file.metadata(), std::fs::metadata(path)) {
        (Ok(held), Ok(named)) => held.dev() == named.dev() && held.ino() == named.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn names(_file: &File, _path: &Path) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::TransferLock;
    use std::fs::File;
    use std::path::Path;

    #[test]
    fn the_lock_sits_beside_the_part_file() {
        assert_eq!(
            TransferLock::path(Path::new("/models/model.gguf.part")),
            Path::new("/models/model.gguf.part.lock")
        );
    }

    #[test]
    fn a_held_lock_is_refused_and_a_released_one_is_removed() {
        let directory = tempfile::tempdir().unwrap();
        let part = directory.path().join("model.gguf.part");
        let held = File::create(TransferLock::path(&part)).unwrap();
        held.lock().unwrap();

        assert!(TransferLock::try_acquire(&part).unwrap().is_none());

        held.unlock().unwrap();
        let lock = TransferLock::try_acquire(&part).unwrap();
        assert!(lock.is_some(), "an unheld lock file is taken over");
        drop(lock);
        assert!(!TransferLock::path(&part).exists());
    }
}
