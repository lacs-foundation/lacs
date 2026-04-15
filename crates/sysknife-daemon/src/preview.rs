use sysknife_types::{PreviewEnvelope, RequestEnvelope, RiskLevel};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
struct PreviewProfile {
    risk_level: RiskLevel,
    expected_side_effects: Vec<String>,
    reboot_required: bool,
    rollback_available: bool,
    warnings: Vec<String>,
}

pub fn preview_action(
    request: &RequestEnvelope,
    current_state: Value,
    proposed_change: Value,
) -> PreviewEnvelope {
    let profile = preview_profile(&request.action_name);

    PreviewEnvelope {
        summary: preview_summary(&request.action_name, &profile.risk_level),
        risk_level: profile.risk_level,
        current_state,
        proposed_change,
        expected_side_effects: profile.expected_side_effects,
        reboot_required: profile.reboot_required,
        rollback_available: profile.rollback_available,
        warnings: profile.warnings,
        request_hash: request.request_hash.clone(),
    }
}

fn preview_profile(action_name: &str) -> PreviewProfile {
    match action_name {
        "GetSystemState"
        | "CollectDiagnostics"
        | "GetDeploymentHistory"
        | "ListDeployments"
        | "GetKernelArguments"
        | "GetPendingUpdates"
        | "SearchFlatpakApps"
        | "ListFlatpakRemotes"
        | "ListInstalledFlatpaks"
        | "GetFlatpakAppInfo"
        | "ListToolboxes"
        | "ListServices"
        | "GetServiceLogs"
        | "GetServiceStatus"
        | "ListTimers"
        | "GetFirewallState"
        | "ListUsers"
        | "ListGroups"
        | "ListPackageRepositories"
        | "ListContainers"
        | "GetContainerInfo"
        | "GetLayeredPackages"
        | "GetDiskUsage"
        | "ListProcesses"
        | "GetMemoryInfo"
        | "GetNetworkStatus"
        | "GetAuthorizedKeys"
        | "ListJobHistory" => PreviewProfile {
            risk_level: RiskLevel::Low,
            expected_side_effects: Vec::new(),
            reboot_required: false,
            rollback_available: false,
            warnings: Vec::new(),
        },
        "ReloadService" => PreviewProfile {
            risk_level: RiskLevel::Medium,
            expected_side_effects: vec!["service config will be reloaded".to_string()],
            reboot_required: false,
            rollback_available: false,
            warnings: vec![
                "approval required".to_string(),
                "requires ExecReload= to be defined in the unit file; \
                 if not defined, use RestartService instead"
                    .to_string(),
            ],
        },
        "RestartService"
        | "ReloadDaemon"
        | "SetServiceEnabled"
        | "StartService"
        | "StopService"
        | "ConfigureWifi"
        | "SetDnsServers"
        | "ConfigureFirewall"
        | "CreateToolbox"
        | "RemoveToolbox"
        | "InstallFlatpak"
        | "RemoveFlatpak"
        | "UpdateFlatpak"
        | "AddFlatpakRemote"
        | "RemoveFlatpakRemote"
        | "MaskService"
        | "UnmaskService"
        | "SetHostname"
        | "SetTimezone"
        | "SetLocale"
        | "SetNtp"
        | "CreateUser" => PreviewProfile {
            risk_level: RiskLevel::Medium,
            expected_side_effects: vec!["service interruption".to_string()],
            reboot_required: false,
            rollback_available: false,
            warnings: vec!["approval required".to_string()],
        },
        "AddUserToGroup"
        | "RemoveUserFromGroup"
        | "DeleteUser"
        | "AddAuthorizedKey"
        | "RemoveAuthorizedKey" => PreviewProfile {
            // High risk: access-control changes — group membership, account
            // deletion, and SSH key modifications require Admin authorization.
            risk_level: RiskLevel::High,
            expected_side_effects: vec!["access control will change".to_string()],
            reboot_required: false,
            rollback_available: false,
            warnings: vec![
                "privilege change".to_string(),
                "exact approval required".to_string(),
            ],
        },
        "AddPackageRepository"
        | "RemovePackageRepository"
        | "EnablePackageRepository"
        | "DisablePackageRepository" => PreviewProfile {
            risk_level: RiskLevel::Medium,
            expected_side_effects: vec!["package repository configuration will change".to_string()],
            reboot_required: false,
            rollback_available: false,
            warnings: vec!["approval required".to_string()],
        },
        "CreateContainer" | "StartContainer" | "StopContainer" | "RemoveContainer" => {
            PreviewProfile {
                risk_level: RiskLevel::Medium,
                expected_side_effects: vec!["container lifecycle will change".to_string()],
                reboot_required: false,
                rollback_available: false,
                warnings: vec!["approval required".to_string()],
            }
        }
        "UpdateSystem"
        | "InstallPackages"
        | "RemovePackages"
        | "RebaseSystem"
        | "RollbackDeployment"
        | "AddLayeredPackage"
        | "RemoveLayeredPackage"
        | "ReplaceLayeredPackage"
        | "ResetLayeredPackageOverride"
        | "RemoveBasePackage" => PreviewProfile {
            risk_level: RiskLevel::High,
            expected_side_effects: vec![
                "system deployment will change".to_string(),
                "reboot may be required".to_string(),
            ],
            reboot_required: true,
            rollback_available: true,
            warnings: vec![
                "reboot required".to_string(),
                "exact approval required".to_string(),
            ],
        },
        "SetKernelArguments" => PreviewProfile {
            risk_level: RiskLevel::High,
            expected_side_effects: vec!["boot arguments will change".to_string()],
            reboot_required: true,
            rollback_available: true,
            warnings: vec![
                "reboot required".to_string(),
                "exact approval required".to_string(),
            ],
        },
        "RebootSystem" => PreviewProfile {
            risk_level: RiskLevel::High,
            expected_side_effects: vec!["system reboot will interrupt running work".to_string()],
            reboot_required: true,
            rollback_available: false,
            warnings: vec![
                "reboot required".to_string(),
                "exact approval required".to_string(),
            ],
        },
        "CleanupDeployments" => PreviewProfile {
            risk_level: RiskLevel::High,
            expected_side_effects: vec!["old deployments may be removed".to_string()],
            reboot_required: false,
            rollback_available: false,
            warnings: vec!["exact approval required".to_string()],
        },
        "PinDeployment" | "UnpinDeployment" => PreviewProfile {
            risk_level: RiskLevel::High,
            expected_side_effects: vec!["deployment pin state will change".to_string()],
            reboot_required: false,
            rollback_available: false,
            warnings: vec!["exact approval required".to_string()],
        },
        _ => PreviewProfile {
            risk_level: RiskLevel::High,
            expected_side_effects: vec!["unclassified action".to_string()],
            reboot_required: false,
            rollback_available: false,
            warnings: vec!["action profile not recognized".to_string()],
        },
    }
}

fn preview_summary(action_name: &str, risk_level: &RiskLevel) -> String {
    let risk = match risk_level {
        RiskLevel::Low => "low-risk",
        RiskLevel::Medium => "medium-risk",
        RiskLevel::High => "high-risk",
    };

    format!("{action_name} preview ({risk})")
}
