//! Whether this process is on the VPN tunnel.
//!
//! Public web fetches stay offline until `ZONE_VPN` is on: SearXNG, page
//! fetch, knowledge URLs, web sources, and `curl`/`wget` from `run_command`.

use std::env;
use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Why a public fetch refused to leave.
pub const OFFLINE: &str = "Public web access is offline until the VPN is enabled.";

/// `ZONE_VPN` is truthy (`1`, `true`, `yes`, `on`).
pub fn enabled() -> bool {
    match env::var("ZONE_VPN") {
        Ok(value) => matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

static ENVIRONMENT: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Holds `ZONE_VPN` in a known state for the guard's lifetime.
pub struct Hold {
    _lock: MutexGuard<'static, ()>,
    previous: Option<OsString>,
}

impl Hold {
    /// `ZONE_VPN=1` until dropped.
    pub fn on() -> Self {
        Self::set(true)
    }

    /// `ZONE_VPN` unset until dropped.
    pub fn off() -> Self {
        Self::set(false)
    }

    fn set(on: bool) -> Self {
        let held = lock();
        let previous = env::var_os("ZONE_VPN");
        // SAFETY: Hold owns ENVIRONMENT for the mutation and the restore.
        unsafe {
            if on {
                env::set_var("ZONE_VPN", "1");
            } else {
                env::remove_var("ZONE_VPN");
            }
        }
        Self {
            _lock: held,
            previous,
        }
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        // SAFETY: Hold still owns ENVIRONMENT while restoring.
        unsafe {
            match &self.previous {
                Some(value) => env::set_var("ZONE_VPN", value),
                None => env::remove_var("ZONE_VPN"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_off_when_unset_or_false() {
        let _vpn = Hold::off();
        assert!(!enabled());
        unsafe { env::set_var("ZONE_VPN", "0") };
        assert!(!enabled());
        unsafe { env::set_var("ZONE_VPN", "") };
        assert!(!enabled());
    }

    #[test]
    fn turns_on_for_truthy_values() {
        let _vpn = Hold::on();
        assert!(enabled());
        for value in ["1", "true", "TRUE", "yes", "on"] {
            unsafe { env::set_var("ZONE_VPN", value) };
            assert!(enabled(), "{value}");
        }
    }
}
