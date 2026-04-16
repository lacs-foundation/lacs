//! Ubuntu package management actions (apt-based).
//!
//! These are the Ubuntu equivalents of the rpm-ostree layering actions in
//! [`layering`]. Action *names* are identical — the executor dispatches to
//! this module when [`distro::current()`] returns [`Distro::Ubuntu`].
//!
//! Key differences from Fedora Atomic layering:
//! - Changes take effect **immediately** — no reboot required (`reboot_required: false`).
//! - No deployment staging or rollback — apt writes directly to the live system.
//! - `AddLayeredPackage` / `RemoveLayeredPackage` map to `apt-get install/remove`.
//! - `UpdateSystem` maps to `apt-get dist-upgrade`.
//! - rpm-ostree-specific actions (`RebaseSystem`, `PinDeployment`, rollback ops,
//!   etc.) have no Ubuntu equivalent and are handled in the executor by returning
//!   an unsupported-on-distro error.
//!
//! [`layering`]: super::layering
//! [`distro::current()`]: crate::distro::current
//! [`Distro::Ubuntu`]: crate::distro::Distro::Ubuntu

use super::{command_mechanism, ActionMechanism, ActionSpec};
use sysknife_types::RiskLevel;

// ---------------------------------------------------------------------------
// Package install / remove
// ---------------------------------------------------------------------------

/// Install a single package immediately (`apt-get install -y`).
///
/// Ubuntu equivalent of [`layering::add_layered_package`].
///
/// [`layering::add_layered_package`]: super::layering::add_layered_package
pub fn install_package(package: &str) -> ActionSpec {
    ActionSpec {
        action_name: "AddLayeredPackage",
        mechanism: command_mechanism("sudo", ["apt-get", "install", "-y", package]),
        risk_level: RiskLevel::High,
        reboot_required: false,
        rollback_available: false,
    }
}

/// Remove a single package (`apt-get remove -y`).
///
/// Ubuntu equivalent of [`layering::remove_layered_package`].
///
/// [`layering::remove_layered_package`]: super::layering::remove_layered_package
pub fn remove_package(package: &str) -> ActionSpec {
    ActionSpec {
        action_name: "RemoveLayeredPackage",
        mechanism: command_mechanism("sudo", ["apt-get", "remove", "-y", package]),
        risk_level: RiskLevel::High,
        reboot_required: false,
        rollback_available: false,
    }
}

/// Install multiple packages in a single `apt-get install -y` call.
///
/// Ubuntu equivalent of [`layering::install_packages`].
///
/// [`layering::install_packages`]: super::layering::install_packages
pub fn install_packages(packages: &[&str]) -> ActionSpec {
    let mut args = vec![
        "apt-get".to_string(),
        "install".to_string(),
        "-y".to_string(),
    ];
    args.extend(packages.iter().map(|s| s.to_string()));

    ActionSpec {
        action_name: "InstallPackages",
        mechanism: ActionMechanism::Command {
            program: "sudo",
            args,
        },
        risk_level: RiskLevel::High,
        reboot_required: false,
        rollback_available: false,
    }
}

/// Remove multiple packages in a single `apt-get remove -y` call.
///
/// Ubuntu equivalent of [`layering::remove_packages`].
///
/// [`layering::remove_packages`]: super::layering::remove_packages
pub fn remove_packages(packages: &[&str]) -> ActionSpec {
    let mut args = vec![
        "apt-get".to_string(),
        "remove".to_string(),
        "-y".to_string(),
    ];
    args.extend(packages.iter().map(|s| s.to_string()));

    ActionSpec {
        action_name: "RemovePackages",
        mechanism: ActionMechanism::Command {
            program: "sudo",
            args,
        },
        risk_level: RiskLevel::High,
        reboot_required: false,
        rollback_available: false,
    }
}

// ---------------------------------------------------------------------------
// System upgrade
// ---------------------------------------------------------------------------

/// Upgrade all installed packages (`apt-get dist-upgrade -y`).
///
/// Ubuntu equivalent of [`deployment::update_system`].
/// Unlike rpm-ostree, apt changes take effect immediately without a reboot
/// (though kernel or libc updates may require one by convention).
///
/// [`deployment::update_system`]: super::deployment::update_system
pub fn upgrade_system() -> ActionSpec {
    ActionSpec {
        action_name: "UpdateSystem",
        mechanism: command_mechanism("sudo", ["apt-get", "dist-upgrade", "-y"]),
        risk_level: RiskLevel::High,
        reboot_required: false,
        rollback_available: false,
    }
}

// ---------------------------------------------------------------------------
// Package queries
// ---------------------------------------------------------------------------

/// List explicitly installed packages via `dpkg --get-selections`.
///
/// Ubuntu equivalent of [`layering::get_layered_packages`].
///
/// [`layering::get_layered_packages`]: super::layering::get_layered_packages
pub fn list_installed_packages() -> ActionSpec {
    // `dpkg --get-selections` lists all packages with their install status.
    // Pipe through `grep -v deinstall` to hide removed-but-not-purged packages.
    ActionSpec {
        action_name: "GetLayeredPackages",
        mechanism: command_mechanism("dpkg", ["--get-selections"]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

/// List packages with available upgrades (`apt list --upgradable`).
///
/// Ubuntu equivalent of [`layering::get_pending_updates`].
///
/// [`layering::get_pending_updates`]: super::layering::get_pending_updates
pub fn list_upgradable() -> ActionSpec {
    ActionSpec {
        action_name: "GetPendingUpdates",
        mechanism: command_mechanism("apt", ["list", "--upgradable"]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}
