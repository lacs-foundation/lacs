use lacs_daemon::actions::containers;
use lacs_daemon::actions::deployment;
use lacs_daemon::actions::flatpak;
use lacs_daemon::actions::layering;
use lacs_daemon::actions::package_repos;
use lacs_daemon::actions::toolbox;
use lacs_daemon::actions::ActionMechanism;
use lacs_types::RiskLevel;

#[test]
fn deployment_family_covers_primary_silverblue_workflows() {
    let names = deployment::specs()
        .into_iter()
        .map(|spec| spec.action_name)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "GetSystemState",
            "CollectDiagnostics",
            "GetDeploymentHistory",
            "ListDeployments",
            "UpdateSystem",
            "PinDeployment",
            "UnpinDeployment",
            "RebaseSystem",
            "CleanupDeployments",
            "RebootSystem",
            "RollbackDeployment",
            "GetKernelArguments",
            "SetKernelArguments",
        ]
    );
}

#[test]
fn update_system_is_planned_as_a_high_risk_rpm_ostree_upgrade() {
    let spec = deployment::update_system();

    assert_eq!(spec.action_name, "UpdateSystem");
    assert_eq!(spec.risk_level, RiskLevel::High);
    assert!(spec.reboot_required);
    assert!(spec.rollback_available);
    assert_eq!(
        spec.mechanism,
        ActionMechanism::Command {
            program: "rpm-ostree",
            args: vec!["upgrade".to_string()],
        }
    );
}

#[test]
fn flatpak_family_covers_install_remove_and_query_actions() {
    let names = flatpak::specs()
        .into_iter()
        .map(|spec| spec.action_name)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "InstallFlatpak",
            "RemoveFlatpak",
            "SearchFlatpakApps",
            "ListFlatpakRemotes",
            "AddFlatpakRemote",
            "RemoveFlatpakRemote",
            "GetFlatpakAppInfo",
        ]
    );
}

#[test]
fn flatpak_install_uses_flatpak_cli_without_shell() {
    let spec = flatpak::install_flatpak("org.mozilla.firefox", "flathub");

    assert_eq!(spec.action_name, "InstallFlatpak");
    assert_eq!(spec.risk_level, RiskLevel::Medium);
    assert_eq!(
        spec.mechanism,
        ActionMechanism::Command {
            program: "flatpak",
            args: vec![
                "install".to_string(),
                "-y".to_string(),
                "flathub".to_string(),
                "org.mozilla.firefox".to_string(),
            ],
        }
    );
}

#[test]
fn toolbox_family_covers_create_enter_list_and_remove() {
    let names = toolbox::specs()
        .into_iter()
        .map(|spec| spec.action_name)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "ListToolboxes",
            "CreateToolbox",
            "EnterToolbox",
            "RemoveToolbox"
        ]
    );
}

#[test]
fn toolbox_create_uses_toolbox_cli() {
    let spec = toolbox::create_toolbox("lacs-dev", Some("41"), None);

    assert_eq!(spec.action_name, "CreateToolbox");
    assert_eq!(spec.risk_level, RiskLevel::Medium);
    assert_eq!(
        spec.mechanism,
        ActionMechanism::Command {
            program: "toolbox",
            args: vec![
                "create".to_string(),
                "--container".to_string(),
                "lacs-dev".to_string(),
                "--release".to_string(),
                "41".to_string()
            ],
        }
    );
}

#[test]
fn layering_family_covers_package_lifecycle() {
    let names = layering::specs()
        .into_iter()
        .map(|spec| spec.action_name)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "InstallPackages",
            "RemovePackages",
            "GetLayeredPackages",
            "AddLayeredPackage",
            "RemoveLayeredPackage",
            "ReplaceLayeredPackage",
            "ResetLayeredPackageOverride",
        ]
    );
}

#[test]
fn layered_package_install_is_high_risk_and_uses_rpm_ostree() {
    let spec = layering::add_layered_package("podman");

    assert_eq!(spec.action_name, "AddLayeredPackage");
    assert_eq!(spec.risk_level, RiskLevel::High);
    assert!(spec.reboot_required);
    assert!(spec.rollback_available);
    assert_eq!(
        spec.mechanism,
        ActionMechanism::Command {
            program: "rpm-ostree",
            args: vec!["install".to_string(), "podman".to_string()],
        }
    );
}

#[test]
fn package_repo_family_covers_repo_file_management() {
    let names = package_repos::specs()
        .into_iter()
        .map(|spec| spec.action_name)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "ListPackageRepositories",
            "AddPackageRepository",
            "RemovePackageRepository",
            "EnablePackageRepository",
            "DisablePackageRepository",
        ]
    );
}

#[test]
fn package_repo_add_is_planned_as_a_repo_file_write() {
    let spec = package_repos::add_package_repository("example", "https://example.invalid/repo");

    assert_eq!(spec.action_name, "AddPackageRepository");
    assert_eq!(
        spec.mechanism,
        ActionMechanism::FileWrite {
            path: "/etc/yum.repos.d/example.repo".to_string(),
            content: "[example]\nbaseurl=https://example.invalid/repo\nenabled=1\n".to_string(),
        }
    );
}

#[test]
fn package_repo_enable_uses_a_targeted_file_patch() {
    let spec = package_repos::enable_package_repository("example");

    assert_eq!(spec.action_name, "EnablePackageRepository");
    assert_eq!(
        spec.mechanism,
        ActionMechanism::FilePatch {
            path: "/etc/yum.repos.d/example.repo".to_string(),
            search: "enabled=0".to_string(),
            replace: "enabled=1".to_string(),
        }
    );
}

#[test]
fn package_repo_disable_uses_a_targeted_file_patch() {
    let spec = package_repos::disable_package_repository("example");

    assert_eq!(spec.action_name, "DisablePackageRepository");
    assert_eq!(
        spec.mechanism,
        ActionMechanism::FilePatch {
            path: "/etc/yum.repos.d/example.repo".to_string(),
            search: "enabled=1".to_string(),
            replace: "enabled=0".to_string(),
        }
    );
}

#[test]
fn container_family_covers_runtime_lifecycle() {
    let names = containers::specs()
        .into_iter()
        .map(|spec| spec.action_name)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "ListContainers",
            "CreateContainer",
            "StartContainer",
            "StopContainer",
            "RemoveContainer",
            "GetContainerInfo",
        ]
    );
}

#[test]
fn container_create_uses_podman_without_shell() {
    let spec =
        containers::create_container("lacs-dev", "registry.fedoraproject.org/fedora-toolbox:41");

    assert_eq!(spec.action_name, "CreateContainer");
    assert_eq!(
        spec.mechanism,
        ActionMechanism::Command {
            program: "podman",
            args: vec![
                "create".to_string(),
                "--name".to_string(),
                "lacs-dev".to_string(),
                "registry.fedoraproject.org/fedora-toolbox:41".to_string(),
            ],
        }
    );
}
