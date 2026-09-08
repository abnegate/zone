#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "zone-dev-migrations-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("manager")).unwrap();
        fs::write(path.join("package.json"), "{}").unwrap();
        fs::create_dir_all(path.join("runner/zone_server/migrations")).unwrap();
        fs::write(
            path.join("runner/zone_server/migrations/001_initial_schema.sql"),
            "SELECT 1;",
        )
        .unwrap();
        fs::create_dir(path.join("bin")).unwrap();
        for name in ["make", "docker"] {
            let command = path.join("bin").join(name);
            fs::write(&command, "#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >> \"$ZONE_TEST_CALLS\"\nexit \"$ZONE_TEST_EXIT\"\n").unwrap();
            fs::set_permissions(command, fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(path)
    }

    fn verify(&self, action: &str, exit: i32) {
        let log = self.0.join("commands");
        let mut paths = vec![self.0.join("bin")];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let output = Command::new(env!("CARGO_BIN_EXE_zone-dev"))
            .arg("--directory")
            .arg(&self.0)
            .args(["db", action])
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("ZONE_TEST_CALLS", &log)
            .env("ZONE_TEST_EXIT", exit.to_string())
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            exit == 0,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let calls = fs::read_to_string(log).unwrap();
        assert_eq!(
            calls.lines().count(),
            1,
            "unexpected database commands: {calls}"
        );
        assert_eq!(
            calls.trim_end(),
            format!("{} db-{action}", self.0.join("bin/make").display())
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn migrate_uses_shared_migration_target() {
    Fixture::new().verify("migrate", 0);
}

#[test]
fn reset_uses_shared_migration_target() {
    Fixture::new().verify("reset", 0);
}

#[test]
fn migrate_propagates_shared_target_failure() {
    Fixture::new().verify("migrate", 19);
}

#[test]
fn reset_propagates_shared_target_failure() {
    Fixture::new().verify("reset", 19);
}
