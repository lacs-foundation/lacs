use super::{command_mechanism, ActionSpec};
use lacs_types::RiskLevel;

pub fn specs() -> Vec<ActionSpec> {
    vec![
        install_packages(&["podman"]),
        remove_packages(&["podman"]),
        get_layered_packages(),
        add_layered_package("podman"),
        remove_layered_package("podman"),
        replace_layered_package("old", "new"),
        reset_layered_package_override(),
    ]
}

pub fn install_packages(packages: &[&str]) -> ActionSpec {
    ActionSpec {
        action_name: "InstallPackages",
        mechanism: command_mechanism(
            "rpm-ostree",
            std::iter::once("install").chain(packages.iter().copied()),
        ),
        risk_level: RiskLevel::High,
        reboot_required: true,
        rollback_available: true,
    }
}

pub fn remove_packages(packages: &[&str]) -> ActionSpec {
    ActionSpec {
        action_name: "RemovePackages",
        mechanism: command_mechanism(
            "rpm-ostree",
            std::iter::once("uninstall").chain(packages.iter().copied()),
        ),
        risk_level: RiskLevel::High,
        reboot_required: true,
        rollback_available: true,
    }
}

pub fn get_layered_packages() -> ActionSpec {
    ActionSpec {
        action_name: "GetLayeredPackages",
        mechanism: command_mechanism("rpm-ostree", ["status", "--json"]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

pub fn add_layered_package(package: &str) -> ActionSpec {
    ActionSpec {
        action_name: "AddLayeredPackage",
        mechanism: command_mechanism("rpm-ostree", ["install", package]),
        risk_level: RiskLevel::High,
        reboot_required: true,
        rollback_available: true,
    }
}

pub fn remove_layered_package(package: &str) -> ActionSpec {
    ActionSpec {
        action_name: "RemoveLayeredPackage",
        mechanism: command_mechanism("rpm-ostree", ["uninstall", package]),
        risk_level: RiskLevel::High,
        reboot_required: true,
        rollback_available: true,
    }
}

pub fn replace_layered_package(old: &str, new: &str) -> ActionSpec {
    ActionSpec {
        action_name: "ReplaceLayeredPackage",
        mechanism: command_mechanism("rpm-ostree", ["override", "replace", old, new]),
        risk_level: RiskLevel::High,
        reboot_required: true,
        rollback_available: true,
    }
}

pub fn reset_layered_package_override() -> ActionSpec {
    ActionSpec {
        action_name: "ResetLayeredPackageOverride",
        mechanism: command_mechanism("rpm-ostree", ["override", "reset"]),
        risk_level: RiskLevel::High,
        reboot_required: true,
        rollback_available: true,
    }
}
