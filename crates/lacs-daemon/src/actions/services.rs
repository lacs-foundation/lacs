use super::{command_mechanism, ActionSpec};
use lacs_types::RiskLevel;

pub fn specs() -> Vec<ActionSpec> {
    vec![
        list_services(),
        start_service("NetworkManager.service"),
        stop_service("NetworkManager.service"),
        restart_service("NetworkManager.service"),
        set_service_enabled("sshd.service", true),
        mask_service("cups.service"),
        unmask_service("cups.service"),
        get_service_logs("NetworkManager.service"),
    ]
}

pub fn list_services() -> ActionSpec {
    ActionSpec {
        action_name: "ListServices",
        mechanism: command_mechanism(
            "systemctl",
            [
                "list-units",
                "--type=service",
                "--all",
                "--no-legend",
                "--no-pager",
            ],
        ),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn start_service(unit: &str) -> ActionSpec {
    ActionSpec {
        action_name: "StartService",
        mechanism: command_mechanism("systemctl", ["start", unit]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn stop_service(unit: &str) -> ActionSpec {
    ActionSpec {
        action_name: "StopService",
        mechanism: command_mechanism("systemctl", ["stop", unit]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn restart_service(unit: &str) -> ActionSpec {
    ActionSpec {
        action_name: "RestartService",
        mechanism: command_mechanism("systemctl", ["restart", unit]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn set_service_enabled(unit: &str, enabled: bool) -> ActionSpec {
    let verb = if enabled { "enable" } else { "disable" };

    ActionSpec {
        action_name: "SetServiceEnabled",
        mechanism: command_mechanism("systemctl", [verb, unit]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: true,
    }
}

pub fn mask_service(unit: &str) -> ActionSpec {
    ActionSpec {
        action_name: "MaskService",
        mechanism: command_mechanism("systemctl", ["mask", unit]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: true,
    }
}

pub fn unmask_service(unit: &str) -> ActionSpec {
    ActionSpec {
        action_name: "UnmaskService",
        mechanism: command_mechanism("systemctl", ["unmask", unit]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: true,
    }
}

pub fn get_service_logs(unit: &str) -> ActionSpec {
    ActionSpec {
        action_name: "GetServiceLogs",
        mechanism: command_mechanism("journalctl", ["-u", unit, "-n", "200", "--no-pager"]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}
