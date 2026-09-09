//! Deciding whether a tool's program is actually installed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub trait ProgramLookup: Send + Sync {
    fn is_available(&self, program: &str) -> bool;
}

/// Resolves a program against `PATH`, the way a shell would.
#[derive(Debug, Clone, Copy, Default)]
pub struct PathLookup;

impl ProgramLookup for PathLookup {
    fn is_available(&self, program: &str) -> bool {
        if program.is_empty() {
            return false;
        }
        if program.contains('/') {
            return is_executable(Path::new(program));
        }
        let Some(path) = std::env::var_os("PATH") else {
            return false;
        };
        std::env::split_paths(&path).any(|directory| {
            let candidate: PathBuf = directory.join(program);
            is_executable(&candidate)
        })
    }
}

/// Treats every program as installed, so detection can be tested on its own.
#[derive(Debug, Clone, Copy, Default)]
pub struct EveryProgram;

impl ProgramLookup for EveryProgram {
    fn is_available(&self, _program: &str) -> bool {
        true
    }
}

/// Treats exactly the named programs as installed.
#[derive(Debug, Clone, Default)]
pub struct NamedPrograms(HashSet<String>);

impl NamedPrograms {
    pub fn new<I, S>(programs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self(programs.into_iter().map(Into::into).collect())
    }
}

impl ProgramLookup for NamedPrograms {
    fn is_available(&self, program: &str) -> bool {
        self.0.contains(program)
    }
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_program_that_is_on_the_path() {
        assert!(
            PathLookup.is_available("sh"),
            "a POSIX shell is always installed on the hosts this runs on"
        );
    }

    #[test]
    fn rejects_a_program_that_is_not_installed() {
        assert!(!PathLookup.is_available("zone-definitely-not-a-real-program"));
        assert!(!PathLookup.is_available(""));
    }

    #[test]
    fn resolves_an_absolute_program_path_without_the_path_variable() {
        assert!(PathLookup.is_available("/bin/sh"));
        assert!(!PathLookup.is_available("/bin/zone-not-real"));
    }

    #[test]
    fn rejects_a_directory_that_shares_a_program_name() {
        assert!(!PathLookup.is_available("/tmp"));
    }

    #[test]
    fn named_programs_only_matches_what_it_was_given() {
        let lookup = NamedPrograms::new(["cargo", "bun"]);
        assert!(lookup.is_available("cargo"));
        assert!(lookup.is_available("bun"));
        assert!(!lookup.is_available("npm"));
    }

    #[test]
    fn every_program_is_available_to_the_test_double() {
        assert!(EveryProgram.is_available("anything-at-all"));
    }
}
