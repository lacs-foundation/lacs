//! Netplan network configuration actions (Ubuntu Server).
//!
//! Netplan is the default network configuration tool on Ubuntu Server. It
//! generates backend configuration for either `systemd-networkd` or
//! `NetworkManager` from YAML files in `/etc/netplan/`.
//!
//! ## When to use netplan vs NetworkManager
//!
//! - Ubuntu **Desktop**: `nmcli` / `NetworkManager` (Phase 2a actions)
//! - Ubuntu **Server** / headless: netplan → detected via `which netplan`
//!
//! The executor routes `NetplanApply` and `NetplanGetConfig` here when the
//! distro is Ubuntu. On Fedora the actions return an unsupported-on-distro
//! error (netplan is not installed by default on Fedora).
//!
//! ## SSH disconnect risk
//!
//! `NetplanApply` reloads network interfaces immediately. On a remote session,
//! a misconfigured netplan YAML can drop the SSH connection with no path to
//! recovery other than console or OOB access. The preview profile carries an
//! explicit warning about this risk.

use super::{command_mechanism, ActionSpec};
use sysknife_types::RiskLevel;

// ---------------------------------------------------------------------------
// specs() — for action_consistency tests
// ---------------------------------------------------------------------------

/// Return one representative `ActionSpec` per netplan action name.
pub fn specs() -> Vec<ActionSpec> {
    vec![
        netplan_get_config(),
        netplan_apply(),
        netplan_set("ethernets.eth0.dhcp4", "true"),
        netplan_generate(),
    ]
}

// ---------------------------------------------------------------------------
// Action constructors
// ---------------------------------------------------------------------------

/// Read current netplan configuration files (`cat /etc/netplan/*.yaml`).
///
/// Risk: Low / Observer. Reads YAML config from `/etc/netplan/`. Does not
/// apply or change anything.
pub fn netplan_get_config() -> ActionSpec {
    ActionSpec {
        action_name: "NetplanGetConfig",
        mechanism: command_mechanism(
            "bash",
            [
                "-c",
                "cat /etc/netplan/*.yaml 2>/dev/null || echo 'no netplan files found'",
            ],
        ),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

/// Set a single netplan key to a value (`sudo netplan set <key>=<value>`).
///
/// Risk: High / Admin. Modifies the active netplan configuration in-memory.
/// Run `NetplanApply` afterward to apply the change to the live network stack.
///
/// `value` is quoted with shell single-quotes when it contains spaces to
/// prevent word splitting by the shell.
pub fn netplan_set(key: &str, value: &str) -> ActionSpec {
    // Quote the value if it contains whitespace to avoid shell word-splitting.
    let kv = if value.contains(' ') {
        format!("{}='{}'", key, value)
    } else {
        format!("{}={}", key, value)
    };
    ActionSpec {
        action_name: "NetplanSet",
        mechanism: command_mechanism("sudo", ["netplan", "set", &kv]),
        risk_level: RiskLevel::High,
        reboot_required: false,
        rollback_available: true,
    }
}

/// Regenerate backend configuration from netplan YAML without applying it
/// (`sudo netplan generate`).
///
/// Risk: Medium. Regenerates the systemd-networkd / NetworkManager config
/// files from the current netplan YAML but does NOT reload interfaces.
/// Safe to use as a dry-run check before `NetplanApply`.
pub fn netplan_generate() -> ActionSpec {
    ActionSpec {
        action_name: "NetplanGenerate",
        mechanism: command_mechanism("sudo", ["netplan", "generate"]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

/// Apply the current netplan configuration (`netplan apply`).
///
/// Risk: High / Admin. Re-configures network interfaces immediately.
/// Can disconnect an SSH session if the configuration is wrong or if the
/// interface IP changes.
///
/// **Warning:** run `netplan try` (with a rollback timeout) in preference
/// to `netplan apply` when testing new configurations. `netplan try` is not
/// exposed as a daemon action because it requires an interactive terminal to
/// accept or reject the change.
pub fn netplan_apply() -> ActionSpec {
    ActionSpec {
        action_name: "NetplanApply",
        mechanism: command_mechanism("sudo", ["netplan", "apply"]),
        risk_level: RiskLevel::High,
        reboot_required: false,
        rollback_available: false,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::ActionMechanism;

    fn extract_cmd(spec: &ActionSpec) -> (&'static str, Vec<&str>) {
        match &spec.mechanism {
            ActionMechanism::Command { program, args } => {
                (*program, args.iter().map(String::as_str).collect())
            }
            _ => panic!("expected Command mechanism"),
        }
    }

    // ── netplan_get_config ───────────────────────────────────────────────────

    #[test]
    fn netplan_get_config_action_name() {
        assert_eq!(netplan_get_config().action_name, "NetplanGetConfig");
    }

    #[test]
    fn netplan_get_config_risk_low() {
        assert_eq!(netplan_get_config().risk_level, RiskLevel::Low);
    }

    #[test]
    fn netplan_get_config_no_reboot() {
        assert!(!netplan_get_config().reboot_required);
    }

    #[test]
    fn netplan_get_config_reads_yaml_files() {
        let spec = netplan_get_config();
        let (prog, args) = extract_cmd(&spec);
        // Uses bash -c to glob /etc/netplan/*.yaml.
        assert_eq!(prog, "bash");
        let cmd = args.join(" ");
        assert!(
            cmd.contains("netplan"),
            "should reference /etc/netplan: {cmd}"
        );
    }

    // ── netplan_apply ────────────────────────────────────────────────────────

    #[test]
    fn netplan_apply_action_name() {
        assert_eq!(netplan_apply().action_name, "NetplanApply");
    }

    #[test]
    fn netplan_apply_risk_high() {
        assert_eq!(netplan_apply().risk_level, RiskLevel::High);
    }

    #[test]
    fn netplan_apply_no_reboot() {
        // netplan apply takes effect immediately; no reboot is required.
        assert!(!netplan_apply().reboot_required);
    }

    #[test]
    fn netplan_apply_argv() {
        let spec = netplan_apply();
        let (prog, args) = extract_cmd(&spec);
        assert_eq!(prog, "sudo");
        assert!(args.contains(&"netplan"));
        assert!(args.contains(&"apply"));
    }

    // ── netplan_set ──────────────────────────────────────────────────────────

    #[test]
    fn netplan_set_action_name() {
        assert_eq!(
            netplan_set("ethernets.eth0.dhcp4", "true").action_name,
            "NetplanSet"
        );
    }

    #[test]
    fn netplan_set_risk_high() {
        assert_eq!(
            netplan_set("ethernets.eth0.dhcp4", "true").risk_level,
            RiskLevel::High
        );
    }

    #[test]
    fn netplan_set_rollback_available() {
        assert!(netplan_set("ethernets.eth0.dhcp4", "true").rollback_available);
    }

    #[test]
    fn netplan_set_no_reboot() {
        assert!(!netplan_set("ethernets.eth0.dhcp4", "true").reboot_required);
    }

    #[test]
    fn netplan_set_argv_simple_value() {
        let spec = netplan_set("ethernets.eth0.dhcp4", "true");
        let (prog, args) = extract_cmd(&spec);
        assert_eq!(prog, "sudo");
        assert!(args.contains(&"netplan"));
        assert!(args.contains(&"set"));
        // key=value appears as a single argument
        assert!(
            args.contains(&"ethernets.eth0.dhcp4=true"),
            "expected key=value arg, got: {:?}",
            args
        );
    }

    #[test]
    fn netplan_set_argv_value_with_spaces_is_quoted() {
        let spec = netplan_set("renderer", "NetworkManager with fallback");
        let (_, args) = extract_cmd(&spec);
        // value contains spaces — must be wrapped in single-quotes.
        assert!(
            args.iter()
                .any(|a| a.contains("'NetworkManager with fallback'")),
            "value with spaces must be single-quoted: {:?}",
            args
        );
    }

    // ── netplan_generate ─────────────────────────────────────────────────────

    #[test]
    fn netplan_generate_action_name() {
        assert_eq!(netplan_generate().action_name, "NetplanGenerate");
    }

    #[test]
    fn netplan_generate_risk_medium() {
        assert_eq!(netplan_generate().risk_level, RiskLevel::Medium);
    }

    #[test]
    fn netplan_generate_no_reboot() {
        assert!(!netplan_generate().reboot_required);
    }

    #[test]
    fn netplan_generate_no_rollback() {
        assert!(!netplan_generate().rollback_available);
    }

    #[test]
    fn netplan_generate_argv() {
        let spec = netplan_generate();
        let (prog, args) = extract_cmd(&spec);
        assert_eq!(prog, "sudo");
        assert!(args.contains(&"netplan"));
        assert!(args.contains(&"generate"));
    }

    // ── specs() completeness ─────────────────────────────────────────────────

    #[test]
    fn specs_covers_all_action_names() {
        let expected = [
            "NetplanGetConfig",
            "NetplanApply",
            "NetplanSet",
            "NetplanGenerate",
        ];
        let spec_names: Vec<&str> = specs().iter().map(|s| s.action_name).collect();
        for name in &expected {
            assert!(spec_names.contains(name), "specs() missing {name}");
        }
    }
}
