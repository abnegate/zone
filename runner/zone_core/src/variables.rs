//! Test-only changes to the process environment, made under one process-wide
//! lock and undone before it is released. Compiled only for tests and the
//! `test-support` feature, never into a server or CLI build.
//!
//! `std::env::set_var` and `remove_var` are unsound while another thread reads
//! the environment, and tests run on threads. Every test in the workspace that
//! changes a variable goes through this module (a [`Variables`], a
//! [`crate::vpn::Hold`], which holds one, or [`Variables::install`]), so:
//!
//! - no two changes interleave, whichever module or crate the tests live in;
//! - a test holding a guard reads the values it set, because no other test can
//!   change a variable while it holds the lock;
//! - reads through `std::env` on other threads are already serialised against
//!   these changes by the standard library's own lock.
//!
//! It cannot cover a read that bypasses `std::env`, such as libc's `getenv`
//! inside `getaddrinfo` or `localtime` on a thread the test did not start.

use std::env;
use std::ffi::{OsStr, OsString};
use std::sync::{Mutex, MutexGuard, PoisonError};

static LOCK: Mutex<()> = Mutex::new(());

/// The process environment, held for this guard's lifetime. Every variable it
/// changes is restored when it drops, while it still holds the lock, so a test
/// that panics leaves nothing behind.
pub struct Variables {
    saved: Vec<(&'static str, Option<OsString>)>,
    _lock: MutexGuard<'static, ()>,
}

impl Variables {
    /// Waits for the lock; nothing is changed yet.
    pub fn lock() -> Self {
        Self {
            saved: Vec::new(),
            _lock: LOCK.lock().unwrap_or_else(PoisonError::into_inner),
        }
    }

    /// Waits for the lock and unsets `names`.
    pub fn isolated(names: &[&'static str]) -> Self {
        let mut variables = Self::lock();
        variables.clear(names);
        variables
    }

    /// Unsets `names` until the guard drops.
    pub fn clear(&mut self, names: &[&'static str]) {
        for name in names {
            self.remove(name);
        }
    }

    /// Sets `name` until the guard drops.
    pub fn set(&mut self, name: &'static str, value: impl AsRef<OsStr>) {
        self.save(name);
        // SAFETY: this guard holds LOCK, and every test change to the
        // environment takes it (see the module documentation for the limits).
        unsafe { env::set_var(name, value) };
    }

    /// Unsets `name` until the guard drops.
    pub fn remove(&mut self, name: &'static str) {
        self.save(name);
        // SAFETY: this guard holds LOCK, and every test change to the
        // environment takes it (see the module documentation for the limits).
        unsafe { env::remove_var(name) };
    }

    /// Sets `name` for the rest of the process, under the lock, without
    /// restoring it: for a value a whole test binary shares, set once before
    /// the tests that read it.
    pub fn install(name: &'static str, value: impl AsRef<OsStr>) {
        let _variables = Self::lock();
        // SAFETY: `_variables` holds LOCK, and every test change to the
        // environment takes it (see the module documentation for the limits).
        unsafe { env::set_var(name, value) };
    }

    fn save(&mut self, name: &'static str) {
        if self.saved.iter().all(|(saved, _)| *saved != name) {
            self.saved.push((name, env::var_os(name)));
        }
    }
}

impl Drop for Variables {
    fn drop(&mut self) {
        for (name, value) in self.saved.drain(..).rev() {
            // SAFETY: the lock is released only after this restore, when the
            // guard's fields drop.
            unsafe {
                match value {
                    Some(value) => env::set_var(name, value),
                    None => env::remove_var(name),
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn held() -> bool {
    LOCK.try_lock().is_err()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHANGED: &str = "ZONE_VARIABLES_TEST_CHANGED";
    const ADDED: &str = "ZONE_VARIABLES_TEST_ADDED";

    #[test]
    fn every_change_is_undone_to_the_value_before_the_first() {
        let mut variables = Variables::isolated(&[ADDED]);
        // SAFETY: `variables` holds LOCK.
        unsafe { env::set_var(CHANGED, "before") };
        variables.set(CHANGED, "first");
        variables.set(CHANGED, "second");
        variables.remove(CHANGED);
        variables.set(ADDED, "added");
        assert!(held());
        drop(variables);

        let variables = Variables::lock();
        assert_eq!(env::var(CHANGED).as_deref(), Ok("before"));
        assert_eq!(env::var_os(ADDED), None);
        // SAFETY: `variables` holds LOCK.
        unsafe { env::remove_var(CHANGED) };
        drop(variables);
    }
}
