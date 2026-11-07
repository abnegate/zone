use super::transfer_lock::TransferLock;
use std::io;
use std::path::{Path, PathBuf};

const PART_EXTENSION: &str = "part";
const VALIDATOR_SUFFIX: &str = ".validator";

/// The files abnegate-llm keeps beside a destination while downloading to it:
/// the bytes received so far, and the validator naming the upstream file they
/// are the start of.
#[derive(Debug)]
pub(super) struct Partial {
    part: PathBuf,
}

impl Partial {
    /// Named exactly as the crate names them for `destination`.
    pub(super) fn of(destination: &Path) -> Self {
        let extension = destination.extension().map_or_else(
            || PART_EXTENSION.to_string(),
            |extension| format!("{}.{PART_EXTENSION}", extension.to_string_lossy()),
        );
        Self {
            part: destination.with_extension(extension),
        }
    }

    pub(super) fn part(&self) -> &Path {
        &self.part
    }

    pub(super) fn validator(&self) -> PathBuf {
        let mut name = self.part.as_os_str().to_owned();
        name.push(VALIDATOR_SUFFIX);
        PathBuf::from(name)
    }

    /// Hand this interrupted download to `successor`, so a transfer to the
    /// successor's destination resumes it, or remove it when it cannot be
    /// resumed there.
    ///
    /// It can only be resumed with its validator, and only where the
    /// successor has no bytes of its own. Nothing moves while a transfer
    /// holds either side.
    pub(super) fn move_to(&self, successor: &Self) -> io::Result<()> {
        if !self.exists() {
            return Ok(());
        }
        let Some(_own) = TransferLock::try_acquire(&self.part)? else {
            return Ok(());
        };
        let theirs = TransferLock::try_acquire(&successor.part)?;
        let resumable = theirs.is_some()
            && self.validator().is_file()
            && self.part.is_file()
            && !successor.part.exists();
        if !resumable {
            return self.remove();
        }
        remove(&successor.validator())?;
        std::fs::rename(&self.part, &successor.part)?;
        std::fs::rename(self.validator(), successor.validator())
    }

    fn exists(&self) -> bool {
        [
            self.part.clone(),
            self.validator(),
            TransferLock::path(&self.part),
        ]
        .iter()
        .any(|path| path.exists())
    }

    fn remove(&self) -> io::Result<()> {
        remove(&self.part)?;
        remove(&self.validator())
    }
}

fn remove(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::transfer_lock::TransferLock;
    use super::Partial;
    use std::fs::File;
    use std::path::Path;

    const BYTES: &[u8] = b"bytes received before the interruption";
    const VALIDATOR: &[u8] = br#"{"url":"http://origin/file.bin","entity_tag":"\"v1\""}"#;

    fn seed(partial: &Partial) {
        std::fs::write(partial.part(), BYTES).unwrap();
        std::fs::write(partial.validator(), VALIDATOR).unwrap();
    }

    #[test]
    fn named_as_abnegate_llm_names_them() {
        let partial = Partial::of(Path::new("/models/model.gguf"));
        assert_eq!(partial.part(), Path::new("/models/model.gguf.part"));
        assert_eq!(
            partial.validator(),
            Path::new("/models/model.gguf.part.validator")
        );
        assert_eq!(
            Partial::of(Path::new("/models/model")).part(),
            Path::new("/models/model.part")
        );
    }

    #[test]
    fn a_resumable_download_moves_to_its_successor() {
        let directory = tempfile::tempdir().unwrap();
        let legacy = Partial::of(&directory.path().join("model.gguf"));
        let successor = Partial::of(&directory.path().join("model.gguf.download"));
        seed(&legacy);

        legacy.move_to(&successor).unwrap();

        assert_eq!(std::fs::read(successor.part()).unwrap(), BYTES);
        assert_eq!(std::fs::read(successor.validator()).unwrap(), VALIDATOR);
        assert!(!legacy.part().exists());
        assert!(!legacy.validator().exists());
        assert!(!TransferLock::path(legacy.part()).exists());
        assert!(!TransferLock::path(successor.part()).exists());
    }

    #[test]
    fn a_download_held_by_a_transfer_is_left_alone() {
        let directory = tempfile::tempdir().unwrap();
        let legacy = Partial::of(&directory.path().join("model.gguf"));
        let successor = Partial::of(&directory.path().join("model.gguf.download"));
        seed(&legacy);
        let held = File::create(TransferLock::path(legacy.part())).unwrap();
        held.lock().unwrap();

        legacy.move_to(&successor).unwrap();

        assert_eq!(std::fs::read(legacy.part()).unwrap(), BYTES);
        assert!(legacy.validator().exists());
        assert!(!successor.part().exists());
    }

    #[test]
    fn a_successor_held_by_a_transfer_is_not_written() {
        let directory = tempfile::tempdir().unwrap();
        let legacy = Partial::of(&directory.path().join("model.gguf"));
        let successor = Partial::of(&directory.path().join("model.gguf.download"));
        seed(&legacy);
        let held = File::create(TransferLock::path(successor.part())).unwrap();
        held.lock().unwrap();

        legacy.move_to(&successor).unwrap();

        assert!(!successor.part().exists());
        assert!(!legacy.part().exists());
        assert!(!legacy.validator().exists());
    }

    #[test]
    fn a_stale_lock_alone_is_removed() {
        let directory = tempfile::tempdir().unwrap();
        let legacy = Partial::of(&directory.path().join("model.gguf"));
        let successor = Partial::of(&directory.path().join("model.gguf.download"));
        std::fs::write(TransferLock::path(legacy.part()), b"").unwrap();

        legacy.move_to(&successor).unwrap();

        assert!(!TransferLock::path(legacy.part()).exists());
        assert!(!successor.part().exists());
    }

    #[test]
    fn nothing_to_move_creates_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let legacy = Partial::of(&directory.path().join("model.gguf"));
        let successor = Partial::of(&directory.path().join("model.gguf.download"));

        legacy.move_to(&successor).unwrap();

        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}
