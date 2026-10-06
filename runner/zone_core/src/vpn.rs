//! Whether this process is on the VPN tunnel, and whether public web may leave.
//!
//! Public fetches (SearXNG, page fetch, knowledge URLs, web sources, and
//! `curl`/`wget` from `run_command`) stay offline when the VPN is configured
//! and the tunnel is down (`ZONE_VPN` off). Configured means a WireGuard
//! private key or OpenVPN user is present, or `ZONE_VPN_REQUIRED` is on.
//! `ZONE_VPN_REQUIRED=0` allows the public internet even with credentials.

use std::env;

#[cfg(any(test, feature = "test-support"))]
use crate::variables::Variables;

/// Why a public fetch refused to leave.
pub const OFFLINE: &str = "Public web access is offline until the VPN is enabled.";

/// Why a public fetch refused to leave this chat.
pub const CHAT_OFFLINE: &str = "This chat is offline and does not use the public web.";

/// Why public web must not leave, if it must not.
pub fn refusal(chat_offline: bool) -> Option<&'static str> {
    if chat_offline {
        Some(CHAT_OFFLINE)
    } else if !allows_public() {
        Some(OFFLINE)
    } else {
        None
    }
}

const VPN: &str = "ZONE_VPN";
const REQUIRED: &str = "ZONE_VPN_REQUIRED";
const WIREGUARD: &str = "VPN_WIREGUARD_PRIVATE_KEY";
const OPENVPN: &str = "VPN_OPENVPN_USER";

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

/// Holds VPN env vars in a known state for the guard's lifetime, under the
/// lock every [`Variables`] takes.
#[cfg(any(test, feature = "test-support"))]
pub struct Hold(Variables);

#[cfg(any(test, feature = "test-support"))]
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

    /// The environment this hold has locked, for changing more variables
    /// under the same lock.
    pub fn variables(&mut self) -> &mut Variables {
        &mut self.0
    }

    fn apply(
        vpn: Option<&str>,
        required: Option<&str>,
        wireguard: Option<&str>,
        openvpn: Option<&str>,
    ) -> Self {
        let mut variables = Variables::lock();
        for (name, value) in [
            (VPN, vpn),
            (REQUIRED, required),
            (WIREGUARD, wireguard),
            (OPENVPN, openvpn),
        ] {
            match value {
                Some(value) => variables.set(name, value),
                None => variables.remove(name),
            }
        }
        Self(variables)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_off_when_unset_or_false() {
        let mut vpn = Hold::off();
        assert!(!enabled());
        assert!(!required());
        assert!(!configured());
        vpn.variables().set(VPN, "0");
        assert!(!enabled());
        vpn.variables().set(VPN, "");
        assert!(!enabled());
    }

    #[test]
    fn turns_on_for_truthy_values() {
        let mut vpn = Hold::on();
        assert!(enabled());
        for value in ["1", "true", "TRUE", "yes", "on"] {
            vpn.variables().set(VPN, value);
            assert!(enabled(), "{value}");
        }
    }

    #[test]
    fn public_web_is_allowed_when_the_vpn_is_not_configured() {
        let mut vpn = Hold::off();
        assert!(allows_public());
        vpn.variables().set(REQUIRED, "0");
        assert!(allows_public());
    }

    #[test]
    fn public_web_is_allowed_when_the_tunnel_is_on() {
        let mut vpn = Hold::required_off();
        assert!(!allows_public());
        vpn.variables().set(VPN, "1");
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
        let mut vpn = Hold::configured_off();
        assert!(configured());
        assert!(!enabled());
        assert!(required());
        assert!(!allows_public());
        vpn.variables().remove(WIREGUARD);
        vpn.variables().set(OPENVPN, "user");
        assert!(configured());
        assert!(!allows_public());
    }

    #[test]
    fn public_web_is_allowed_when_required_is_off_even_with_credentials() {
        let mut vpn = Hold::configured_off();
        vpn.variables().set(REQUIRED, "0");
        assert!(configured());
        assert!(!required());
        assert!(allows_public());
    }

    #[test]
    fn an_offline_chat_refuses_public_web_even_when_the_tunnel_is_on() {
        let _vpn = Hold::on();
        assert_eq!(refusal(true), Some(CHAT_OFFLINE));
        assert_eq!(refusal(false), None);
    }

    #[test]
    fn a_live_chat_still_follows_the_vpn_gate() {
        let _vpn = Hold::required_off();
        assert_eq!(refusal(false), Some(OFFLINE));
        assert_eq!(refusal(true), Some(CHAT_OFFLINE));
    }

    #[test]
    fn a_hold_takes_the_lock_every_variables_guard_takes() {
        let _vpn = Hold::off();
        assert!(crate::variables::held());
    }
}
