use super::{command_mechanism, ActionSpec};
use sysknife_types::RiskLevel;

pub fn specs() -> Vec<ActionSpec> {
    vec![
        configure_wifi("CafeHotspot"),
        set_dns_servers("wlp1s0", &["1.1.1.1", "8.8.8.8"]),
        configure_firewall("public", "ssh", true),
        get_firewall_state(),
        get_network_status(),
    ]
}

pub fn configure_wifi(ssid: &str) -> ActionSpec {
    ActionSpec {
        action_name: "ConfigureWifi",
        mechanism: command_mechanism("nmcli", ["device", "wifi", "connect", ssid, "--ask"]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn set_dns_servers(interface: &str, servers: &[&str]) -> ActionSpec {
    let args = std::iter::once("dns")
        .chain(std::iter::once(interface))
        .chain(servers.iter().copied());

    ActionSpec {
        action_name: "SetDnsServers",
        mechanism: command_mechanism("resolvectl", args),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn configure_firewall(zone: &str, service: &str, enabled: bool) -> ActionSpec {
    let mut args = vec!["--zone".to_string(), zone.to_string()];
    if enabled {
        args.push("--add-service".to_string());
    } else {
        args.push("--remove-service".to_string());
    }
    args.push(service.to_string());

    ActionSpec {
        action_name: "ConfigureFirewall",
        mechanism: super::ActionMechanism::Command {
            program: "firewall-cmd",
            args,
        },
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn get_firewall_state() -> ActionSpec {
    // `--list-all` shows the active zone, interfaces, services, ports, and
    // rich rules — the full picture. `--state` only returns "running"/"not
    // running" which is useless for actual configuration inspection.
    ActionSpec {
        action_name: "GetFirewallState",
        mechanism: command_mechanism("firewall-cmd", ["--list-all"]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn get_network_status() -> ActionSpec {
    ActionSpec {
        action_name: "GetNetworkStatus",
        mechanism: command_mechanism("ip", ["-brief", "addr"]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}
