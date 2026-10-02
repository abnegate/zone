//! Whether this process is on the VPN tunnel, and whether public web may leave.
//!
//! Public fetches (SearXNG, page fetch, knowledge URLs, web sources, and
//! `curl`/`wget` from `run_command`) stay offline only when the VPN is
//! configured as required (`ZONE_VPN_REQUIRED`) and the tunnel is down
//! (`ZONE_VPN` off). An install that never required the VPN uses the public
//! internet directly.

use std::env;
use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Why a public fetch refused to leave.
pub const OFFLINE: &str = "Public web access is offline until the VPN is enabled.";

const VPN: &str = "ZONE_VPN";
const REQUIRED: &str = "ZONE_VPN_REQUIRED";

fn truthy(name: &str) -> bool {
    match env::var(name) {
        Ok(value) => matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

/// `ZONE_VPN` is truthy (`1`, `true`, `yes`, `on`).
pub fn enabled() -> bool {
    truthy(VPN)
}

/// `ZONE_VPN_REQUIRED` is truthy: public web must use the tunnel.
pub fn required() -> bool {
    truthy(REQUIRED)
}

/// Public web may leave this process.
pub fn allows_public() -> bool {
    enabled() || !required()
}

static ENVIRONMENT: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Holds VPN env vars in a known state for the guard's lifetime.
pub struct Hold {
    _lock: MutexGuard<'static, ()>,
    previous_vpn: Option<OsString>,
    previous_required: Option<OsString>,
}

impl Hold {
    /// Tunnel on (`ZONE_VPN=1`).
    pub fn on() -> Self {
        Self::set(true, true)
    }

    /// Tunnel off and not required: public web is allowed.
    pub fn off() -> Self {
        Self::set(false, false)
    }

    /// VPN is required and the tunnel is down: public web stays offline.
    pub fn required_off() -> Self {
        Self::set(false, true)
    }

    fn set(on: bool, required: bool) -> Self {
        let held = lock();
        let previous_vpn = env::var_os(VPN);
        let previous_required = env::var_os(REQUIRED);
        // SAFETY: Hold owns ENVIRONMENT for the mutation and the restore.
        unsafe {
            if on {
                env::set_var(VPN, "1");
            } else {
                env::remove_var(VPN);
            }
            if required {
                env::set_var(REQUIRED, "1");
            } else {
                env::remove_var(REQUIRED);
            }
        }
        Self {
            _lock: held,
            previous_vpn,
            previous_required,
        }
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        // SAFETY: Hold still owns ENVIRONMENT while restoring.
        unsafe {
            match &self.previous_vpn {
                Some(value) => env::set_var(VPN, value),
                None => env::remove_var(VPN),
            }
            match &self.previous_required {
                Some(value) => env::set_var(REQUIRED, value),
                None => env::remove_var(REQUIRED),
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
        assert!(!required());
        unsafe { env::set_var(VPN, "0") };
        assert!(!enabled());
        unsafe { env::set_var(VPN, "") };
        assert!(!enabled());
    }

    #[test]
    fn turns_on_for_truthy_values() {
        let _vpn = Hold::on();
        assert!(enabled());
        for value in ["1", "true", "TRUE", "yes", "on"] {
            unsafe { env::set_var(VPN, value) };
            assert!(enabled(), "{value}");
        }
    }

    #[test]
    fn public_web_is_allowed_when_the_vpn_is_not_required() {
        let _vpn = Hold::off();
        assert!(allows_public());
        unsafe { env::set_var(REQUIRED, "0") };
        assert!(allows_public());
    }

    #[test]
    fn public_web_is_allowed_when_the_tunnel_is_on() {
        let _vpn = Hold::required_off();
        assert!(!allows_public());
        unsafe { env::set_var(VPN, "1") };
        assert!(allows_public());
    }

    #[test]
    fn public_web_stays_offline_when_required_and_the_tunnel_is_down() {
        let _vpn = Hold::required_off();
        assert!(required());
        assert!(!enabled());
        assert!(!allows_public());
    }
}
