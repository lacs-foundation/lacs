use super::{command_mechanism, ActionSpec};
use lacs_types::RiskLevel;

pub fn specs() -> Vec<ActionSpec> {
    vec![
        list_containers(),
        create_container("lacs-dev", "registry.fedoraproject.org/fedora-toolbox:41"),
        start_container("lacs-dev"),
        stop_container("lacs-dev"),
        remove_container("lacs-dev"),
        get_container_info("lacs-dev"),
    ]
}

pub fn list_containers() -> ActionSpec {
    ActionSpec {
        action_name: "ListContainers",
        mechanism: command_mechanism("podman", ["ps", "--all", "--format", "json"]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn create_container(name: &str, image: &str) -> ActionSpec {
    ActionSpec {
        action_name: "CreateContainer",
        mechanism: command_mechanism("podman", ["create", "--name", name, image]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: true,
    }
}

pub fn start_container(name: &str) -> ActionSpec {
    ActionSpec {
        action_name: "StartContainer",
        mechanism: command_mechanism("podman", ["start", name]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn stop_container(name: &str) -> ActionSpec {
    ActionSpec {
        action_name: "StopContainer",
        mechanism: command_mechanism("podman", ["stop", name]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn remove_container(name: &str) -> ActionSpec {
    ActionSpec {
        action_name: "RemoveContainer",
        mechanism: command_mechanism("podman", ["rm", name]),
        risk_level: RiskLevel::Medium,
        reboot_required: false,
        rollback_available: true,
    }
}

pub fn get_container_info(name: &str) -> ActionSpec {
    ActionSpec {
        action_name: "GetContainerInfo",
        mechanism: command_mechanism("podman", ["inspect", name]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}
