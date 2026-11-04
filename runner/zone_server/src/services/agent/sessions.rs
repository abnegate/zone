//! A CLI session's file, found in one login's home and carried into another, so the next turn
//! resumes the conversation on whichever login it lands on.
//!
//! Claude keeps a session at `projects/<sanitized work>/<id>.jsonl` in its config directory;
//! Codex at `sessions/<yyyy>/<mm>/<dd>/rollout-<stamp>-<id>.jsonl` in its home. Both resume a
//! session by reading that file, so a carried copy under the same relative path is all the other
//! home needs. Claude finds a session by the directory it runs in, so a session whose chat now
//! starts somewhere else is moved to that directory's folder within its home.

mod carried;
mod error;

pub use carried::Carried;
pub use error::Error;

use std::fs::{self, DirBuilder, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use nix::fcntl::OFlag;
use uuid::Uuid;
use zone_core::llm::AgentKind;

const PROJECTS: &str = "projects";
const SESSIONS: &str = "sessions";
const ROLLOUT: &str = "rollout-";
const EXTENSION: &str = ".jsonl";
const STAGING: &str = ".carrying";

/// Claude cuts a longer project folder name to this many UTF-16 units and appends a hash.
const FOLDER_LIMIT: usize = 200;
const SEPARATOR: char = '-';
const HASH_RADIX: u32 = 36;

const FILE_MODE: u32 = 0o600;
const DIRECTORY_MODE: u32 = 0o700;

/// Whether `agent` resumes a session whose file was carried in from another login's home. Set
/// from `agent_session_resume_tests`, which runs each CLI across two homes.
pub fn portable(agent: AgentKind) -> bool {
    match agent {
        AgentKind::Claude | AgentKind::Codex => true,
    }
}

/// The file `agent` keeps session `id` in under `home`, for turns run in `work`.
pub fn locate(home: &Path, agent: AgentKind, work: &Path, id: &str) -> Option<PathBuf> {
    relative(home, agent, work, id).map(|relative| home.join(relative))
}

/// Copies session `id`'s file from the home `from` into the home `to` under the same relative
/// path, readable only by its owner. A file already there is replaced whole, never partly.
pub fn carry(
    from: &Path,
    to: &Path,
    agent: AgentKind,
    work: &Path,
    id: &str,
) -> Result<Carried, Error> {
    let found = newest(from, agent, id).ok_or_else(|| Error::Missing {
        agent,
        id: id.to_string(),
    })?;
    let relative = match agent {
        AgentKind::Claude => claude(work, id),
        AgentKind::Codex => found.clone(),
    };
    let source = from.join(&found);
    let path = to.join(&relative);
    let parent = path.parent().unwrap_or(to);

    directories(to, relative.parent().unwrap_or(Path::new(""))).map_err(|source| {
        Error::Filesystem {
            path: parent.to_path_buf(),
            source,
        }
    })?;

    let staging = parent.join(format!("{STAGING}-{}", Uuid::new_v4().simple()));
    let bytes = copy(&source, &staging)
        .and_then(|bytes| fs::rename(&staging, &path).map(|()| bytes))
        .map_err(|error| {
            let _ = fs::remove_file(&staging);
            Error::Filesystem {
                path: path.clone(),
                source: error,
            }
        })?;

    Ok(Carried { path, bytes })
}

/// Moves session `id` within `home` to where `agent` looks for it when it runs in `work`. Claude
/// looks in a folder named for the directory it runs in, and a session stays with its folder when
/// a chat starts somewhere new, so a chat that changes directory would otherwise lose its
/// session. Codex finds a session by its id alone.
///
/// A handover copies a session and leaves the copy it left behind, so a home can hold an old one,
/// even in the folder `work` names. The newest is the session, and the rest are removed: resuming
/// an old one would lose every turn since without anything failing. The folder claude keeps beside
/// a transcript stays where it is, since the transcript cites what is in it by absolute path.
pub fn relocate(home: &Path, agent: AgentKind, work: &Path, id: &str) -> Result<(), Error> {
    if agent == AgentKind::Codex {
        return Ok(());
    }
    let found = newest(home, agent, id).ok_or_else(|| Error::Missing {
        agent,
        id: id.to_string(),
    })?;
    let relative = claude(work, id);
    let path = home.join(&relative);
    let source = home.join(&found);
    if !same_file(&source, &path) {
        directories(home, relative.parent().unwrap_or(Path::new("")))
            .and_then(|()| fs::rename(&source, &path))
            .map_err(|source| Error::Filesystem {
                path: path.clone(),
                source,
            })?;
    }
    for (copy, _) in copies(home, id) {
        let copy = home.join(copy);
        if !same_file(&copy, &path) {
            fs::remove_file(&copy).map_err(|source| Error::Filesystem { path: copy, source })?;
        }
    }
    Ok(())
}

/// Whether two paths are one file, which two spellings of a path are on a filesystem that ignores
/// case.
fn same_file(one: &Path, other: &Path) -> bool {
    match (fs::symlink_metadata(one), fs::symlink_metadata(other)) {
        (Ok(one), Ok(other)) => one.dev() == other.dev() && one.ino() == other.ino(),
        _ => false,
    }
}

/// Claude's project folder name for a working directory: every UTF-16 unit that is not an ASCII
/// letter or digit becomes `-`, and a name longer than [`FOLDER_LIMIT`] is cut there and suffixed
/// with `-` and the base-36 magnitude of the path's Java-style string hash.
pub fn sanitized(work: &Path) -> String {
    let units: Vec<u16> = work.to_string_lossy().encode_utf16().collect();
    let name: String = units
        .iter()
        .map(|&unit| match char::from_u32(u32::from(unit)) {
            Some(character) if character.is_ascii_alphanumeric() => character,
            _ => SEPARATOR,
        })
        .collect();

    if units.len() <= FOLDER_LIMIT {
        return name;
    }

    let hash = units.iter().fold(0_i32, |hash, &unit| {
        hash.wrapping_mul(31).wrapping_add(i32::from(unit))
    });
    format!(
        "{}{SEPARATOR}{}",
        &name[..FOLDER_LIMIT],
        base36(hash.unsigned_abs())
    )
}

fn relative(home: &Path, agent: AgentKind, work: &Path, id: &str) -> Option<PathBuf> {
    if !named(id) {
        return None;
    }

    let relative = match agent {
        AgentKind::Claude => {
            let relative = claude(work, id);
            regular(&home.join(&relative)).then_some(relative)
        }
        AgentKind::Codex => rollout(home, Path::new(SESSIONS), &format!("-{id}{EXTENSION}")),
    }?;
    relative
        .parent()
        .is_none_or(|parent| unlinked(home, parent))
        .then_some(relative)
}

/// Where claude keeps session `id` for turns run in `work`, relative to its home.
fn claude(work: &Path, id: &str) -> PathBuf {
    Path::new(PROJECTS)
        .join(sanitized(&physical(work)))
        .join(format!("{id}{EXTENSION}"))
}

/// Session `id` wherever in `home` `agent` left it, whichever directory it last ran in: the most
/// recently written copy, when an earlier handover left more than one.
fn newest(home: &Path, agent: AgentKind, id: &str) -> Option<PathBuf> {
    if !named(id) {
        return None;
    }
    match agent {
        AgentKind::Claude => copies(home, id)
            .into_iter()
            .max_by_key(|(_, modified)| *modified)
            .map(|(relative, _)| relative),
        AgentKind::Codex => rollout(home, Path::new(SESSIONS), &format!("-{id}{EXTENSION}"))
            .filter(|relative| {
                relative
                    .parent()
                    .is_none_or(|parent| unlinked(home, parent))
            }),
    }
}

/// Every copy of claude's session `id` under `home`, relative to it, and when each was written.
fn copies(home: &Path, id: &str) -> Vec<(PathBuf, SystemTime)> {
    if !named(id) {
        return Vec::new();
    }
    let Ok(folders) = fs::read_dir(home.join(PROJECTS)) else {
        return Vec::new();
    };
    folders
        .flatten()
        .filter_map(|folder| {
            let relative = Path::new(PROJECTS)
                .join(folder.file_name())
                .join(format!("{id}{EXTENSION}"));
            let metadata = fs::symlink_metadata(home.join(&relative)).ok()?;
            (metadata.file_type().is_file()
                && relative
                    .parent()
                    .is_none_or(|parent| unlinked(home, parent)))
            .then_some((relative, metadata.modified().ok()?))
        })
        .collect()
}

/// Whether every directory on `relative`'s way down from `home` is a directory itself, not a
/// link to one that could lie outside the home.
fn unlinked(home: &Path, relative: &Path) -> bool {
    let mut path = home.to_path_buf();
    relative.components().all(|component| {
        path.push(component);
        real(&path)
    })
}

/// Creates each directory on `relative`'s way down from `home` that is missing, refusing one that
/// is a link, so nothing is written outside the home.
fn directories(home: &Path, relative: &Path) -> io::Result<()> {
    let mut path = home.to_path_buf();
    for component in relative.components() {
        path.push(component);
        match DirBuilder::new().mode(DIRECTORY_MODE).create(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        if !real(&path) {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!("{} is a link or not a directory", path.display()),
            ));
        }
    }
    Ok(())
}

fn real(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

/// Session ids are UUIDs. Anything else could name a path outside the session folders.
fn named(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == SEPARATOR)
}

/// The directory as the CLI's `getcwd` reports it, which resolves every link on the way.
fn physical(work: &Path) -> PathBuf {
    fs::canonicalize(work).unwrap_or_else(|_| work.to_path_buf())
}

fn regular(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

/// The rollout under `home/directory` whose name ends in `suffix`, searched without following
/// links.
fn rollout(home: &Path, directory: &Path, suffix: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(home.join(directory)).ok()?;
    let mut directories = Vec::new();

    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let relative = directory.join(entry.file_name());
        if kind.is_dir() {
            directories.push(relative);
        } else if kind.is_file()
            && entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_prefix(ROLLOUT))
                .and_then(|rest| rest.strip_suffix(suffix))
                .is_some_and(|stamp| !stamp.is_empty())
        {
            return Some(relative);
        }
    }

    directories
        .iter()
        .find_map(|directory| rollout(home, directory, suffix))
}

fn copy(source: &Path, destination: &Path) -> io::Result<u64> {
    let mut reader = OpenOptions::new()
        .read(true)
        .custom_flags(OFlag::O_NOFOLLOW.bits())
        .open(source)?;
    let mut writer = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .open(destination)?;
    let bytes = io::copy(&mut reader, &mut writer)?;
    writer.sync_all()?;
    Ok(bytes)
}

fn base36(mut value: u32) -> String {
    let mut digits = Vec::new();
    loop {
        digits.push(char::from_digit(value % HASH_RADIX, HASH_RADIX).unwrap_or(SEPARATOR));
        value /= HASH_RADIX;
        if value == 0 {
            break;
        }
    }
    digits.iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use tempfile::TempDir;

    use super::*;

    const CLAUDE_ID: &str = "6f1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    const CODEX_ID: &str = "019b2c41-0000-7000-8000-000000000001";
    const CODEX_DAY: &str = "sessions/2026/09/23";
    const TRANSCRIPT: &[u8] = b"{\"type\":\"user\",\"message\":\"remember marigold\"}\n";

    /// Makes `path` an hour older than anything written now.
    fn age(path: &Path) {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
    }

    struct Homes {
        from: TempDir,
        to: TempDir,
        work: TempDir,
    }

    impl Homes {
        fn new() -> Self {
            Self {
                from: TempDir::new().unwrap(),
                to: TempDir::new().unwrap(),
                work: TempDir::new().unwrap(),
            }
        }

        fn claude(&self) -> PathBuf {
            Path::new(PROJECTS)
                .join(sanitized(&fs::canonicalize(self.work.path()).unwrap()))
                .join(format!("{CLAUDE_ID}{EXTENSION}"))
        }

        fn codex(&self) -> PathBuf {
            Path::new(CODEX_DAY).join(format!(
                "{ROLLOUT}2026-09-23T04-00-00-{CODEX_ID}{EXTENSION}"
            ))
        }

        fn write(&self, relative: &Path) {
            let path = self.from.path().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, TRANSCRIPT).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        }

        fn carry(&self, agent: AgentKind, id: &str) -> Result<Carried, Error> {
            carry(
                self.from.path(),
                self.to.path(),
                agent,
                self.work.path(),
                id,
            )
        }
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn empty(path: &Path) -> bool {
        fs::read_dir(path).unwrap().next().is_none()
    }

    #[test]
    fn a_session_file_is_carried_between_two_login_homes_under_the_same_path() {
        let homes = Homes::new();
        let layouts = [
            (AgentKind::Claude, CLAUDE_ID, homes.claude()),
            (AgentKind::Codex, CODEX_ID, homes.codex()),
        ];

        for (agent, id, relative) in layouts {
            homes.write(&relative);

            let carried = homes.carry(agent, id).unwrap();

            let expected = homes.to.path().join(&relative);
            assert_eq!(carried.path, expected, "{agent} keeps its relative path");
            assert_eq!(carried.bytes, TRANSCRIPT.len() as u64);
            assert_eq!(fs::read(&expected).unwrap(), TRANSCRIPT);
            assert_eq!(mode(&expected), FILE_MODE, "{agent}'s copy is private");
            assert_eq!(
                locate(homes.to.path(), agent, homes.work.path(), id),
                Some(expected),
                "{agent} finds the carried session where it looks for its own"
            );
        }
    }

    #[test]
    fn a_carried_session_replaces_an_older_copy_and_leaves_nothing_staged() {
        let homes = Homes::new();
        let relative = homes.claude();
        homes.write(&relative);
        let stale = homes.to.path().join(&relative);
        fs::create_dir_all(stale.parent().unwrap()).unwrap();
        fs::write(&stale, b"stale").unwrap();

        homes.carry(AgentKind::Claude, CLAUDE_ID).unwrap();

        assert_eq!(fs::read(&stale).unwrap(), TRANSCRIPT);
        let left: Vec<_> = fs::read_dir(stale.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, [stale.file_name().unwrap().to_owned()]);
    }

    #[test]
    fn carrying_a_session_that_does_not_exist_is_reported_not_faked() {
        let homes = Homes::new();
        homes.write(&homes.codex());

        for (agent, id) in [
            (AgentKind::Claude, CLAUDE_ID),
            (AgentKind::Codex, "019b2c41-0000-7000-8000-00000000ffff"),
        ] {
            let error = homes.carry(agent, id).unwrap_err();
            assert!(
                matches!(&error, Error::Missing { agent: missing, id: named } if *missing == agent && named == id),
                "{agent}: {error:?}"
            );
        }
        assert!(
            empty(homes.to.path()),
            "nothing is written for a missing session"
        );
    }

    #[test]
    fn a_claude_session_moves_to_the_folder_of_the_directory_its_chat_starts_in() {
        let homes = Homes::new();
        let left = homes.claude();
        homes.write(&left);
        let project = TempDir::new().unwrap();
        let expected = homes
            .from
            .path()
            .join(PROJECTS)
            .join(sanitized(&fs::canonicalize(project.path()).unwrap()))
            .join(format!("{CLAUDE_ID}{EXTENSION}"));

        relocate(
            homes.from.path(),
            AgentKind::Claude,
            project.path(),
            CLAUDE_ID,
        )
        .unwrap();

        assert_eq!(fs::read(&expected).unwrap(), TRANSCRIPT);
        assert!(
            !homes.from.path().join(&left).exists(),
            "one session stays in one place"
        );
        assert_eq!(
            locate(
                homes.from.path(),
                AgentKind::Claude,
                project.path(),
                CLAUDE_ID
            ),
            Some(expected.clone())
        );

        relocate(
            homes.from.path(),
            AgentKind::Claude,
            homes.work.path(),
            CLAUDE_ID,
        )
        .unwrap();
        assert_eq!(fs::read(homes.from.path().join(&left)).unwrap(), TRANSCRIPT);
        assert!(!expected.exists(), "and it can move back");
    }

    #[test]
    fn a_session_in_place_is_left_alone_and_a_codex_session_is_found_from_anywhere() {
        let homes = Homes::new();
        let claude = homes.claude();
        let codex = homes.codex();
        homes.write(&claude);
        homes.write(&codex);
        let elsewhere = TempDir::new().unwrap();

        relocate(
            homes.from.path(),
            AgentKind::Claude,
            homes.work.path(),
            CLAUDE_ID,
        )
        .unwrap();
        relocate(
            homes.from.path(),
            AgentKind::Codex,
            elsewhere.path(),
            CODEX_ID,
        )
        .unwrap();

        assert!(homes.from.path().join(&claude).exists());
        assert!(homes.from.path().join(&codex).exists());
        assert!(
            locate(
                homes.from.path(),
                AgentKind::Codex,
                elsewhere.path(),
                CODEX_ID
            )
            .is_some()
        );
    }

    #[test]
    fn moving_a_session_that_does_not_exist_is_reported() {
        let homes = Homes::new();
        assert!(matches!(
            relocate(
                homes.from.path(),
                AgentKind::Claude,
                homes.work.path(),
                CLAUDE_ID
            ),
            Err(Error::Missing { .. })
        ));
    }

    /// A handover copies a session and leaves the copy it left behind, so a home can hold an old
    /// one in the very folder the chat comes back to. Resuming that would lose every turn since.
    #[test]
    fn the_newest_copy_of_a_session_is_the_one_a_move_keeps() {
        let homes = Homes::new();
        let stale = homes.claude();
        homes.write(&stale);
        let elsewhere = TempDir::new().unwrap();
        let current = Path::new(PROJECTS)
            .join(sanitized(&fs::canonicalize(elsewhere.path()).unwrap()))
            .join(format!("{CLAUDE_ID}{EXTENSION}"));
        let path = homes.from.path().join(&current);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"current").unwrap();
        age(&homes.from.path().join(&stale));

        relocate(
            homes.from.path(),
            AgentKind::Claude,
            homes.work.path(),
            CLAUDE_ID,
        )
        .unwrap();

        assert_eq!(
            fs::read(homes.from.path().join(&stale)).unwrap(),
            b"current"
        );
        assert!(!path.exists(), "one session stays in one place");
    }

    #[test]
    fn a_stale_copy_elsewhere_does_not_outlive_the_move_or_take_its_place() {
        let homes = Homes::new();
        let current = homes.claude();
        homes.write(&current);
        let elsewhere = TempDir::new().unwrap();
        let stale = homes
            .from
            .path()
            .join(PROJECTS)
            .join(sanitized(&fs::canonicalize(elsewhere.path()).unwrap()))
            .join(format!("{CLAUDE_ID}{EXTENSION}"));
        fs::create_dir_all(stale.parent().unwrap()).unwrap();
        fs::write(&stale, b"stale").unwrap();
        age(&stale);

        relocate(
            homes.from.path(),
            AgentKind::Claude,
            homes.work.path(),
            CLAUDE_ID,
        )
        .unwrap();

        assert_eq!(
            fs::read(homes.from.path().join(&current)).unwrap(),
            TRANSCRIPT
        );
        assert!(!stale.exists(), "a stale copy could be resumed later");
    }

    #[test]
    fn the_newest_copy_of_a_session_is_the_one_carried() {
        let homes = Homes::new();
        let stale = homes.claude();
        homes.write(&stale);
        age(&homes.from.path().join(&stale));
        let elsewhere = TempDir::new().unwrap();
        let current = homes
            .from
            .path()
            .join(PROJECTS)
            .join(sanitized(&fs::canonicalize(elsewhere.path()).unwrap()))
            .join(format!("{CLAUDE_ID}{EXTENSION}"));
        fs::create_dir_all(current.parent().unwrap()).unwrap();
        fs::write(&current, b"current").unwrap();

        let carried = homes.carry(AgentKind::Claude, CLAUDE_ID).unwrap();

        assert_eq!(fs::read(&carried.path).unwrap(), b"current");
    }

    /// On a filesystem that ignores case, a folder renamed by case alone leaves the session in a
    /// folder that spells it another way. It is the same file, and must not be removed as a stale
    /// copy of itself.
    #[test]
    fn a_session_in_a_folder_spelled_another_way_is_not_removed_as_a_copy_of_itself() {
        let homes = Homes::new();
        let parent = TempDir::new().unwrap();
        let work = parent.path().join("code");
        fs::create_dir_all(&work).unwrap();
        let folder = sanitized(&fs::canonicalize(&work).unwrap());
        let respelled = folder.replacen("-code", "-Code", 1);
        assert_ne!(folder, respelled);
        let left = Path::new(PROJECTS)
            .join(&respelled)
            .join(format!("{CLAUDE_ID}{EXTENSION}"));
        homes.write(&left);

        relocate(homes.from.path(), AgentKind::Claude, &work, CLAUDE_ID).unwrap();

        assert_eq!(
            fs::read(
                homes
                    .from
                    .path()
                    .join(PROJECTS)
                    .join(&folder)
                    .join(format!("{CLAUDE_ID}{EXTENSION}"))
            )
            .unwrap(),
            TRANSCRIPT,
            "the only copy of the session is gone"
        );
    }

    #[test]
    fn a_session_id_that_names_a_path_is_never_moved() {
        let homes = Homes::new();
        let outside = homes.from.path().join("secret.jsonl");
        fs::write(&outside, TRANSCRIPT).unwrap();

        assert!(matches!(
            relocate(
                homes.from.path(),
                AgentKind::Claude,
                homes.work.path(),
                "../../secret"
            ),
            Err(Error::Missing { .. })
        ));
        assert!(outside.exists());
    }

    #[test]
    fn a_session_is_carried_to_another_home_from_the_directory_it_last_ran_in() {
        let homes = Homes::new();
        let previous = TempDir::new().unwrap();
        let left = Path::new(PROJECTS)
            .join(sanitized(&fs::canonicalize(previous.path()).unwrap()))
            .join(format!("{CLAUDE_ID}{EXTENSION}"));
        homes.write(&left);

        let carried = homes.carry(AgentKind::Claude, CLAUDE_ID).unwrap();

        assert_eq!(carried.path, homes.to.path().join(homes.claude()));
        assert_eq!(fs::read(&carried.path).unwrap(), TRANSCRIPT);
    }

    #[test]
    fn a_session_id_that_names_a_path_is_never_carried() {
        let homes = Homes::new();
        let outside = homes.from.path().join("secret.jsonl");
        fs::write(&outside, TRANSCRIPT).unwrap();

        for agent in AgentKind::ALL {
            for id in ["", "../../secret", "../secret", "a/b"] {
                assert!(
                    matches!(homes.carry(agent, id), Err(Error::Missing { .. })),
                    "{agent} refuses {id:?}"
                );
            }
        }
        assert!(empty(homes.to.path()));
    }

    #[test]
    fn a_session_linked_out_of_the_home_is_not_carried() {
        let homes = Homes::new();
        let outside = homes.work.path().join("elsewhere.jsonl");
        fs::write(&outside, TRANSCRIPT).unwrap();

        for (agent, id, relative) in [
            (AgentKind::Claude, CLAUDE_ID, homes.claude()),
            (AgentKind::Codex, CODEX_ID, homes.codex()),
        ] {
            let path = homes.from.path().join(&relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            symlink(&outside, &path).unwrap();

            assert!(
                matches!(homes.carry(agent, id), Err(Error::Missing { .. })),
                "{agent} does not follow the link"
            );
        }
        assert!(empty(homes.to.path()));
    }

    #[test]
    fn a_session_under_a_linked_directory_of_the_home_is_not_carried() {
        let homes = Homes::new();
        let outside = TempDir::new().unwrap();

        for (agent, id, relative, linked) in [
            (AgentKind::Claude, CLAUDE_ID, homes.claude(), 2),
            (AgentKind::Codex, CODEX_ID, homes.codex(), 1),
        ] {
            let directory: PathBuf = relative.components().take(linked).collect();
            let target = outside.path().join(agent.as_str());
            let file = target.join(relative.strip_prefix(&directory).unwrap());
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, TRANSCRIPT).unwrap();
            let link = homes.from.path().join(&directory);
            fs::create_dir_all(link.parent().unwrap()).unwrap();
            symlink(&target, &link).unwrap();

            assert_eq!(
                locate(homes.from.path(), agent, homes.work.path(), id),
                None,
                "{agent} finds a session through a linked directory"
            );
            assert!(
                matches!(homes.carry(agent, id), Err(Error::Missing { .. })),
                "{agent} carries a session through a linked directory"
            );
        }
        assert!(empty(homes.to.path()));
    }

    #[test]
    fn a_session_is_never_carried_through_a_linked_directory_of_the_receiving_home() {
        let homes = Homes::new();
        let outside = TempDir::new().unwrap();

        for (agent, id, relative, linked) in [
            (AgentKind::Claude, CLAUDE_ID, homes.claude(), 1),
            (AgentKind::Codex, CODEX_ID, homes.codex(), 2),
        ] {
            homes.write(&relative);
            let directory: PathBuf = relative.components().take(linked).collect();
            let target = outside.path().join(agent.as_str());
            fs::create_dir_all(&target).unwrap();
            let link = homes.to.path().join(&directory);
            fs::create_dir_all(link.parent().unwrap()).unwrap();
            symlink(&target, &link).unwrap();

            assert!(
                matches!(homes.carry(agent, id), Err(Error::Filesystem { .. })),
                "{agent} carries into a linked directory"
            );
            assert!(empty(&target), "{agent} wrote outside the receiving home");
        }
    }

    #[test]
    fn a_claude_session_is_found_under_the_real_path_of_a_linked_work_directory() {
        let homes = Homes::new();
        let relative = homes.claude();
        homes.write(&relative);
        let link = homes.to.path().join("work");
        symlink(homes.work.path(), &link).unwrap();

        assert_eq!(
            locate(homes.from.path(), AgentKind::Claude, &link, CLAUDE_ID),
            Some(homes.from.path().join(relative))
        );
    }

    #[test]
    fn a_work_directory_sanitises_like_claudes_project_folder() {
        let long = format!(
            "/srv/zone/state/{}claude/work",
            "3f2b9c1e-6d4a-4f0b-9c7e-1a2b3c4d5e6f/".repeat(6)
        );
        let astral = format!("/var/lib/zone/agents/{}/🦀/work", "a".repeat(190));
        let cases = [
            (
                "/srv/zone/state/3f2b9c1e-6d4a-4f0b-9c7e-1a2b3c4d5e6f/claude/work",
                "-srv-zone-state-3f2b9c1e-6d4a-4f0b-9c7e-1a2b3c4d5e6f-claude-work".to_string(),
            ),
            (
                "/Users/jake/My Projects/.claude/café_☕/work",
                "-Users-jake-My-Projects--claude-caf----work".to_string(),
            ),
            ("/srv/🦀/work", "-srv----work".to_string()),
            (
                long.as_str(),
                format!(
                    "-srv-zone-state-{}3f2b9c1e-6d4a-4f0b-9c7e-1a2b3c4d5e6f-3bttw8",
                    "3f2b9c1e-6d4a-4f0b-9c7e-1a2b3c4d5e6f-".repeat(4)
                ),
            ),
            (
                astral.as_str(),
                format!("-var-lib-zone-agents-{}-96imsu", "a".repeat(179)),
            ),
        ];

        for (work, expected) in cases {
            assert_eq!(sanitized(Path::new(work)), expected, "{work}");
        }
    }

    #[test]
    fn both_agents_resume_a_carried_session() {
        assert!(AgentKind::ALL.into_iter().all(portable));
    }
}
