//! Whether this process is on the VPN tunnel, and whether public web may leave.
//!
//! Public fetches (SearXNG, page fetch, knowledge URLs, web sources, and
//! `curl`/`wget` from `run_command`) stay offline when the VPN is configured
//! and the tunnel is down (`ZONE_VPN` off). Configured means a WireGuard
//! private key or OpenVPN user is present, or `ZONE_VPN_REQUIRED` is on.
//! `ZONE_VPN_REQUIRED=0` allows the public internet even with credentials.

use std::env;
use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Why a public fetch refused to leave.
pub const OFFLINE: &str = "Public web access is offline until the VPN is enabled.";

const VPN: &str = "ZONE_VPN";
const REQUIRED: &str = "ZONE_VPN_REQUIRED";
const WIREGUARD: &str = "VPN_WIREGUARD_PRIVATE_KEY";
const OPENVPN: &str = "VPN_OPENVPN_USER";
const NAMES: [&str; 4] = [VPN, REQUIRED, WIREGUARD, OPENVPN];

fn truthy(name: &str) -> bool {
    match env::var(name) {
        Ok(value) => matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

fn present(name: &str) -> bool {
    match env::var(name) {
        Ok(value) => !value.trim().is_empty(),
        Err(_) => false,
    }
}

fn required_flag() -> Option<bool> {
    match env::var(REQUIRED) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        },
        Err(_) => None,
    }
}

/// `ZONE_VPN` is truthy (`1`, `true`, `yes`, `on`).
pub fn enabled() -> bool {
    truthy(VPN)
}

/// A WireGuard private key or OpenVPN user is present.
pub fn configured() -> bool {
    present(WIREGUARD) || present(OPENVPN)
}

/// Public web must use the tunnel: `ZONE_VPN_REQUIRED` when set, otherwise
/// credentials in the environment.
pub fn required() -> bool {
    required_flag().unwrap_or_else(configured)
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
    previous: [Option<OsString>; 4],
}

impl Hold {
    /// Tunnel on (`ZONE_VPN=1`).
    pub fn on() -> Self {
        Self::apply(Some("1"), Some("1"), None, None)
    }

    /// Tunnel off and not configured: public web is allowed.
    pub fn off() -> Self {
        Self::apply(None, None, None, None)
    }

    /// VPN is required and the tunnel is down: public web stays offline.
    pub fn required_off() -> Self {
        Self::apply(None, Some("1"), None, None)
    }

    /// Credentials present, required unset, tunnel down: public web stays offline.
    pub fn configured_off() -> Self {
        Self::apply(None, None, Some("1"), None)
    }

    fn apply(
        vpn: Option<&str>,
        required: Option<&str>,
        wireguard: Option<&str>,
        openvpn: Option<&str>,
    ) -> Self {
        let held = lock();
        let previous = [
            env::var_os(VPN),
            env::var_os(REQUIRED),
            env::var_os(WIREGUARD),
            env::var_os(OPENVPN),
        ];
        // SAFETY: Hold owns ENVIRONMENT for the mutation and the restore.
        unsafe {
            for (name, value) in [
                (VPN, vpn),
                (REQUIRED, required),
                (WIREGUARD, wireguard),
                (OPENVPN, openvpn),
            ] {
                match value {
                    Some(value) => env::set_var(name, value),
                    None => env::remove_var(name),
                }
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
            for (name, previous) in NAMES.iter().zip(self.previous.iter()) {
                match previous {
                    Some(value) => env::set_var(name, value),
                    None => env::remove_var(name),
                }
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
        assert!(!configured());
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
    fn public_web_is_allowed_when_the_vpn_is_not_configured() {
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

    #[test]
    fn public_web_stays_offline_when_credentials_are_present_and_the_tunnel_is_down() {
        let _vpn = Hold::configured_off();
        assert!(configured());
        assert!(!enabled());
        assert!(required());
        assert!(!allows_public());
        unsafe { env::remove_var(WIREGUARD) };
        unsafe { env::set_var(OPENVPN, "user") };
        assert!(configured());
        assert!(!allows_public());
    }

    #[test]
    fn public_web_is_allowed_when_required_is_off_even_with_credentials() {
        let _vpn = Hold::configured_off();
        unsafe { env::set_var(REQUIRED, "0") };
        assert!(configured());
        assert!(!required());
        assert!(allows_public());
    }
}
