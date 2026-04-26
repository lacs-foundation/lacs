//! fail2ban intrusion-prevention actions (Ubuntu / Debian).
//!
//! fail2ban is an intrusion-prevention tool that monitors log files and bans
//! IP addresses that exhibit suspicious behaviour (e.g. repeated failed SSH
//! logins). These actions use the `fail2ban-client` CLI.
//!
//! ## Jail terminology
//!
//! A **jail** is a named set of rules (filter + action) that monitors a
//! specific service. For example, `sshd` watches `/var/log/auth.log` for SSH
//! authentication failures.
//!
//! ## IP validation
//!
//! `Fail2banBanIp` and `Fail2banUnbanIp` validate the supplied IP address with
//! `std::net::IpAddr::from_str` before constructing the `ActionSpec`. An
//! invalid address returns `Err(InvalidIpAddress)` immediately so a bad value
//! cannot reach the daemon.

use std::net::IpAddr;
use std::str::FromStr;

use super::{command_mechanism, ActionSpec};
use sysknife_types::RiskLevel;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Returned when the supplied IP string is not a valid IPv4 or IPv6 address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidIpAddress(pub String);

impl std::fmt::Display for InvalidIpAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid IP address: '{}'", self.0)
    }
}

impl std::error::Error for InvalidIpAddress {}

// ---------------------------------------------------------------------------
// specs() — for action_consistency tests
// ---------------------------------------------------------------------------

/// Return one representative `ActionSpec` per fail2ban action name.
pub fn specs() -> Vec<ActionSpec> {
    vec![
        fail2ban_status(None),
        fail2ban_ban_ip("sshd", "192.0.2.1").expect("valid IP in specs()"),
        fail2ban_unban_ip("sshd", "192.0.2.1").expect("valid IP in specs()"),
    ]
}

// ---------------------------------------------------------------------------
// Action constructors
// ---------------------------------------------------------------------------

/// Show fail2ban jail status (`sudo fail2ban-client status [<jail>]`).
///
/// Risk: Low. Read-only; shows active jails, banned IPs, and hit counts.
/// When `jail` is `None` the global status (list of all jails) is returned.
/// When `jail` is `Some(name)` the detailed status for that jail is returned.
pub fn fail2ban_status(jail: Option<&str>) -> ActionSpec {
    let args: Vec<&str> = match jail {
        Some(j) => vec!["fail2ban-client", "status", j],
        None => vec!["fail2ban-client", "status"],
    };
    ActionSpec {
        action_name: "Fail2banStatus",
        mechanism: command_mechanism("sudo", args),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

/// Ban an IP address in a fail2ban jail
/// (`sudo fail2ban-client set <jail> banip <ip>`).
///
/// Risk: High. Immediately blocks all traffic from the IP for all services
/// protected by the named jail. Banning a legitimate address can cause an
/// outage (e.g. banning the admin's own IP on the `sshd` jail).
///
/// Returns `Err(InvalidIpAddress)` when `ip` is not a valid IPv4 or IPv6
/// address.
pub fn fail2ban_ban_ip(jail: &str, ip: &str) -> Result<ActionSpec, InvalidIpAddress> {
    // Validate before constructing the spec so a bad value never reaches the daemon.
    IpAddr::from_str(ip).map_err(|_| InvalidIpAddress(ip.to_string()))?;
    Ok(ActionSpec {
        action_name: "Fail2banBanIp",
        mechanism: command_mechanism("sudo", ["fail2ban-client", "set", jail, "banip", ip]),
        risk_level: RiskLevel::High,
        reboot_required: false,
        rollback_available: false,
    })
}

/// Unban an IP address from a fail2ban jail
/// (`sudo fail2ban-client set <jail> unbanip <ip>`).
///
/// Risk: Medium. Removes a ban, potentially re-admitting a previously blocked
/// address. Reversible — the address can be banned again with `Fail2banBanIp`.
///
/// Returns `Err(InvalidIpAddress)` when `ip` is not a valid IPv4 or IPv6
/// address.
pub fn fail2ban_unban_ip(jail: &str, ip: &str) -> Result<ActionSpec, InvalidIpAddress> {
    IpAddr::from_str(ip).map_err(|_| InvalidIpAddress(ip.to_string()))?;
    Ok(ActionSpec {
        action_name: "Fail2banUnbanIp",
        mechanism: command_mechanism("sudo", ["fail2ban-client", "set", jail, "unbanip", ip]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::ActionMechanism;

    fn extract_args(spec: &ActionSpec) -> (&'static str, Vec<String>) {
        match &spec.mechanism {
            ActionMechanism::Command { program, args } => (*program, args.clone()),
            _ => panic!("expected Command mechanism"),
        }
    }

    // ── fail2ban_status (global) ─────────────────────────────────────────────

    #[test]
    fn fail2ban_status_action_name() {
        assert_eq!(fail2ban_status(None).action_name, "Fail2banStatus");
    }

    #[test]
    fn fail2ban_status_risk_is_low() {
        assert_eq!(fail2ban_status(None).risk_level, RiskLevel::Low);
    }

    #[test]
    fn fail2ban_status_global_argv() {
        let spec = fail2ban_status(None);
        let (prog, args) = extract_args(&spec);
        assert_eq!(prog, "sudo");
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        assert_eq!(a, vec!["fail2ban-client", "status"]);
    }

    // ── fail2ban_status (with jail) ──────────────────────────────────────────

    #[test]
    fn fail2ban_status_jail_argv() {
        let spec = fail2ban_status(Some("sshd"));
        let (prog, args) = extract_args(&spec);
        assert_eq!(prog, "sudo");
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        assert_eq!(a, vec!["fail2ban-client", "status", "sshd"]);
    }

    // ── fail2ban_ban_ip ──────────────────────────────────────────────────────

    #[test]
    fn fail2ban_ban_ip_action_name() {
        assert_eq!(
            fail2ban_ban_ip("sshd", "10.0.0.1").unwrap().action_name,
            "Fail2banBanIp"
        );
    }

    #[test]
    fn fail2ban_ban_ip_risk_is_high() {
        assert_eq!(
            fail2ban_ban_ip("sshd", "10.0.0.1").unwrap().risk_level,
            RiskLevel::High
        );
    }

    #[test]
    fn fail2ban_ban_ip_argv_ordering() {
        // argv must be: sudo fail2ban-client set <jail> banip <ip>
        let spec = fail2ban_ban_ip("sshd", "198.51.100.42").unwrap();
        let (prog, args) = extract_args(&spec);
        assert_eq!(prog, "sudo");
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        assert_eq!(a[0], "fail2ban-client");
        assert_eq!(a[1], "set");
        assert_eq!(a[2], "sshd");
        assert_eq!(a[3], "banip");
        assert_eq!(a[4], "198.51.100.42");
    }

    #[test]
    fn fail2ban_ban_ip_rejects_invalid_ip() {
        let err = fail2ban_ban_ip("sshd", "not-an-ip").unwrap_err();
        assert_eq!(err, InvalidIpAddress("not-an-ip".to_string()));
    }

    #[test]
    fn fail2ban_ban_ip_accepts_ipv6() {
        let spec = fail2ban_ban_ip("sshd", "::1").unwrap();
        let (_, args) = extract_args(&spec);
        assert!(args.contains(&"::1".to_string()));
    }

    // ── fail2ban_unban_ip ────────────────────────────────────────────────────

    #[test]
    fn fail2ban_unban_ip_action_name() {
        assert_eq!(
            fail2ban_unban_ip("sshd", "10.0.0.1").unwrap().action_name,
            "Fail2banUnbanIp"
        );
    }

    #[test]
    fn fail2ban_unban_ip_risk_is_medium() {
        assert_eq!(
            fail2ban_unban_ip("sshd", "10.0.0.1").unwrap().risk_level,
            RiskLevel::Medium
        );
    }

    #[test]
    fn fail2ban_unban_ip_argv_ordering() {
        // argv must be: sudo fail2ban-client set <jail> unbanip <ip>
        let spec = fail2ban_unban_ip("nginx-http-auth", "203.0.113.7").unwrap();
        let (prog, args) = extract_args(&spec);
        assert_eq!(prog, "sudo");
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        assert_eq!(a[0], "fail2ban-client");
        assert_eq!(a[1], "set");
        assert_eq!(a[2], "nginx-http-auth");
        assert_eq!(a[3], "unbanip");
        assert_eq!(a[4], "203.0.113.7");
    }

    #[test]
    fn fail2ban_unban_ip_rejects_invalid_ip() {
        let err = fail2ban_unban_ip("sshd", "256.0.0.1").unwrap_err();
        assert_eq!(err, InvalidIpAddress("256.0.0.1".to_string()));
    }

    // ── specs() completeness ─────────────────────────────────────────────────

    #[test]
    fn specs_covers_all_action_names() {
        let expected = ["Fail2banStatus", "Fail2banBanIp", "Fail2banUnbanIp"];
        let spec_names: Vec<&str> = specs().iter().map(|s| s.action_name).collect();
        for name in &expected {
            assert!(spec_names.contains(name), "specs() missing {name}");
        }
    }
}
