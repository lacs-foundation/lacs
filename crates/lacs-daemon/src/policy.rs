//! Authorization policy for the daemon.
//!
//! Combines two checks:
//! 1. **Per-action allowlist** — each known action name maps to a minimum
//!    `CallerRole`. This is a compile-time constant so the daemon never
//!    executes an action whose policy was not reviewed at build time.
//! 2. **Approval freshness** — the approval hash from the shell must match
//!    the request hash computed during preview.

use lacs_types::CallerRole;

use crate::auth::role_rank;

// ---------------------------------------------------------------------------
// Per-action minimum role
// ---------------------------------------------------------------------------

/// Return the minimum `CallerRole` required to call `action_name`, or `None`
/// if the action is not recognised.
///
/// The mapping is intentionally exhaustive over every action known to the
/// executor. Unknown actions return `None` so the caller can emit a
/// validation-failure error rather than silently denying or allowing.
pub fn min_role_for_action(action_name: &str) -> Option<CallerRole> {
    let role = match action_name {
        // ── Read-only / informational (Observer) ─────────────────────────
        "GetSystemState"
        | "CollectDiagnostics"
        | "GetDeploymentHistory"
        | "ListDeployments"
        | "GetKernelArguments"
        | "ListFlatpakRemotes"
        | "SearchFlatpakApps"
        | "GetFlatpakAppInfo"
        | "ListContainers"
        | "GetContainerInfo"
        | "GetLayeredPackages"
        | "ListPackageRepositories"
        | "ListServices"
        | "GetServiceLogs"
        | "ListToolboxes"
        | "GetFirewallState"
        | "ListUsers"
        | "ListGroups" => CallerRole::Observer,

        // ── Medium-risk mutations (Dev) ──────────────────────────────────
        //
        // Flatpak install/remove, container lifecycle, service control,
        // toolbox ops, identity changes, network config, package repos,
        // and user create/delete.
        "InstallFlatpak"
        | "RemoveFlatpak"
        | "AddFlatpakRemote"
        | "RemoveFlatpakRemote"
        | "CreateContainer"
        | "StartContainer"
        | "StopContainer"
        | "RemoveContainer"
        | "StartService"
        | "StopService"
        | "RestartService"
        | "SetServiceEnabled"
        | "MaskService"
        | "UnmaskService"
        | "CreateToolbox"
        | "RemoveToolbox"
        | "SetHostname"
        | "SetTimezone"
        | "SetLocale"
        | "SetNtp"
        | "ConfigureWifi"
        | "SetDnsServers"
        | "ConfigureFirewall"
        | "AddPackageRepository"
        | "RemovePackageRepository"
        | "EnablePackageRepository"
        | "DisablePackageRepository"
        | "CreateUser"
        | "DeleteUser" => CallerRole::Dev,

        // ── High-risk system mutations (Admin) ───────────────────────────
        //
        // Deployment lifecycle, layering, kernel arguments, privilege-
        // escalation-sensitive user-group operations.
        "UpdateSystem"
        | "PinDeployment"
        | "UnpinDeployment"
        | "RebaseSystem"
        | "CleanupDeployments"
        | "RebootSystem"
        | "RollbackDeployment"
        | "SetKernelArguments"
        | "InstallPackages"
        | "RemovePackages"
        | "AddLayeredPackage"
        | "RemoveLayeredPackage"
        | "ReplaceLayeredPackage"
        | "ResetLayeredPackageOverride"
        | "AddUserToGroup"
        | "RemoveUserFromGroup" => CallerRole::Admin,

        _ => return None,
    };
    Some(role)
}

/// Check whether `caller` is authorized to invoke `action_name`.
///
/// Returns `true` if the action is known and the caller's role meets or
/// exceeds the minimum role required by the per-action allowlist. Returns
/// `false` for unknown actions (the caller should surface a validation error
/// separately).
pub fn action_allowed(caller: &CallerRole, action_name: &str) -> bool {
    match min_role_for_action(action_name) {
        Some(required) => role_rank(caller) >= role_rank(&required),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Approval freshness
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("stale approval")]
pub enum ApprovalError {
    StaleApproval {
        request_hash: String,
        approval_hash: String,
    },
}

pub fn approval_matches_request(request_hash: &str, approval_hash: &str) -> bool {
    !request_hash.is_empty() && request_hash == approval_hash
}

pub fn require_fresh_approval(
    request_hash: &str,
    approval_hash: &str,
) -> Result<(), ApprovalError> {
    if approval_matches_request(request_hash, approval_hash) {
        Ok(())
    } else {
        Err(ApprovalError::StaleApproval {
            request_hash: request_hash.to_string(),
            approval_hash: approval_hash.to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Observer — can call read-only actions
    // ------------------------------------------------------------------

    #[test]
    fn observer_can_call_read_only_actions() {
        let role = CallerRole::Observer;
        assert!(action_allowed(&role, "GetSystemState"));
        assert!(action_allowed(&role, "CollectDiagnostics"));
        assert!(action_allowed(&role, "GetDeploymentHistory"));
        assert!(action_allowed(&role, "ListDeployments"));
        assert!(action_allowed(&role, "GetKernelArguments"));
        assert!(action_allowed(&role, "ListFlatpakRemotes"));
        assert!(action_allowed(&role, "SearchFlatpakApps"));
        assert!(action_allowed(&role, "GetFlatpakAppInfo"));
        assert!(action_allowed(&role, "ListContainers"));
        assert!(action_allowed(&role, "GetContainerInfo"));
        assert!(action_allowed(&role, "GetLayeredPackages"));
        assert!(action_allowed(&role, "ListPackageRepositories"));
        assert!(action_allowed(&role, "ListServices"));
        assert!(action_allowed(&role, "GetServiceLogs"));
        assert!(action_allowed(&role, "ListToolboxes"));
        assert!(action_allowed(&role, "GetFirewallState"));
        assert!(action_allowed(&role, "ListUsers"));
        assert!(action_allowed(&role, "ListGroups"));
    }

    // ------------------------------------------------------------------
    // Observer — cannot call medium or high risk actions
    // ------------------------------------------------------------------

    #[test]
    fn observer_cannot_call_medium_or_high_risk_actions() {
        let role = CallerRole::Observer;
        // Medium-risk
        assert!(!action_allowed(&role, "InstallFlatpak"));
        assert!(!action_allowed(&role, "RemoveFlatpak"));
        assert!(!action_allowed(&role, "CreateContainer"));
        assert!(!action_allowed(&role, "StartService"));
        assert!(!action_allowed(&role, "CreateToolbox"));
        assert!(!action_allowed(&role, "SetHostname"));
        assert!(!action_allowed(&role, "ConfigureWifi"));
        assert!(!action_allowed(&role, "AddPackageRepository"));
        // High-risk
        assert!(!action_allowed(&role, "UpdateSystem"));
        assert!(!action_allowed(&role, "RebaseSystem"));
        assert!(!action_allowed(&role, "InstallPackages"));
        assert!(!action_allowed(&role, "AddUserToGroup"));
        assert!(!action_allowed(&role, "RebootSystem"));
        assert!(!action_allowed(&role, "SetKernelArguments"));
    }

    // ------------------------------------------------------------------
    // Dev — can call medium risk actions (and all observer actions)
    // ------------------------------------------------------------------

    #[test]
    fn dev_can_call_medium_risk_actions() {
        let role = CallerRole::Dev;
        // Medium-risk
        assert!(action_allowed(&role, "InstallFlatpak"));
        assert!(action_allowed(&role, "RemoveFlatpak"));
        assert!(action_allowed(&role, "AddFlatpakRemote"));
        assert!(action_allowed(&role, "RemoveFlatpakRemote"));
        assert!(action_allowed(&role, "CreateContainer"));
        assert!(action_allowed(&role, "StartContainer"));
        assert!(action_allowed(&role, "StopContainer"));
        assert!(action_allowed(&role, "RemoveContainer"));
        assert!(action_allowed(&role, "StartService"));
        assert!(action_allowed(&role, "StopService"));
        assert!(action_allowed(&role, "RestartService"));
        assert!(action_allowed(&role, "SetServiceEnabled"));
        assert!(action_allowed(&role, "MaskService"));
        assert!(action_allowed(&role, "UnmaskService"));
        assert!(action_allowed(&role, "CreateToolbox"));
        assert!(action_allowed(&role, "RemoveToolbox"));
        assert!(action_allowed(&role, "SetHostname"));
        assert!(action_allowed(&role, "SetTimezone"));
        assert!(action_allowed(&role, "SetLocale"));
        assert!(action_allowed(&role, "SetNtp"));
        assert!(action_allowed(&role, "ConfigureWifi"));
        assert!(action_allowed(&role, "SetDnsServers"));
        assert!(action_allowed(&role, "ConfigureFirewall"));
        assert!(action_allowed(&role, "AddPackageRepository"));
        assert!(action_allowed(&role, "RemovePackageRepository"));
        assert!(action_allowed(&role, "EnablePackageRepository"));
        assert!(action_allowed(&role, "DisablePackageRepository"));
        assert!(action_allowed(&role, "CreateUser"));
        assert!(action_allowed(&role, "DeleteUser"));
        // Observer-level actions still allowed
        assert!(action_allowed(&role, "GetSystemState"));
        assert!(action_allowed(&role, "ListServices"));
        assert!(action_allowed(&role, "ListContainers"));
    }

    // ------------------------------------------------------------------
    // Dev — cannot call high risk actions
    // ------------------------------------------------------------------

    #[test]
    fn dev_cannot_call_high_risk_actions() {
        let role = CallerRole::Dev;
        assert!(!action_allowed(&role, "UpdateSystem"));
        assert!(!action_allowed(&role, "PinDeployment"));
        assert!(!action_allowed(&role, "UnpinDeployment"));
        assert!(!action_allowed(&role, "RebaseSystem"));
        assert!(!action_allowed(&role, "CleanupDeployments"));
        assert!(!action_allowed(&role, "RebootSystem"));
        assert!(!action_allowed(&role, "RollbackDeployment"));
        assert!(!action_allowed(&role, "SetKernelArguments"));
        assert!(!action_allowed(&role, "InstallPackages"));
        assert!(!action_allowed(&role, "RemovePackages"));
        assert!(!action_allowed(&role, "AddLayeredPackage"));
        assert!(!action_allowed(&role, "RemoveLayeredPackage"));
        assert!(!action_allowed(&role, "ReplaceLayeredPackage"));
        assert!(!action_allowed(&role, "ResetLayeredPackageOverride"));
        assert!(!action_allowed(&role, "AddUserToGroup"));
        assert!(!action_allowed(&role, "RemoveUserFromGroup"));
    }

    // ------------------------------------------------------------------
    // Admin — can call high risk actions (and all lower)
    // ------------------------------------------------------------------

    #[test]
    fn admin_can_call_high_risk_actions() {
        let role = CallerRole::Admin;
        // High-risk
        assert!(action_allowed(&role, "UpdateSystem"));
        assert!(action_allowed(&role, "PinDeployment"));
        assert!(action_allowed(&role, "UnpinDeployment"));
        assert!(action_allowed(&role, "RebaseSystem"));
        assert!(action_allowed(&role, "CleanupDeployments"));
        assert!(action_allowed(&role, "RebootSystem"));
        assert!(action_allowed(&role, "RollbackDeployment"));
        assert!(action_allowed(&role, "SetKernelArguments"));
        assert!(action_allowed(&role, "InstallPackages"));
        assert!(action_allowed(&role, "RemovePackages"));
        assert!(action_allowed(&role, "AddLayeredPackage"));
        assert!(action_allowed(&role, "RemoveLayeredPackage"));
        assert!(action_allowed(&role, "ReplaceLayeredPackage"));
        assert!(action_allowed(&role, "ResetLayeredPackageOverride"));
        assert!(action_allowed(&role, "AddUserToGroup"));
        assert!(action_allowed(&role, "RemoveUserFromGroup"));
        // Medium-risk still allowed
        assert!(action_allowed(&role, "InstallFlatpak"));
        assert!(action_allowed(&role, "CreateToolbox"));
        assert!(action_allowed(&role, "StartService"));
        // Observer-level still allowed
        assert!(action_allowed(&role, "GetSystemState"));
        assert!(action_allowed(&role, "ListUsers"));
    }

    // ------------------------------------------------------------------
    // Boot — can call everything
    // ------------------------------------------------------------------

    #[test]
    fn boot_can_call_everything() {
        let role = CallerRole::Boot;
        // Sample from each tier
        assert!(action_allowed(&role, "GetSystemState"));
        assert!(action_allowed(&role, "ListDeployments"));
        assert!(action_allowed(&role, "ListContainers"));
        assert!(action_allowed(&role, "GetFirewallState"));
        assert!(action_allowed(&role, "InstallFlatpak"));
        assert!(action_allowed(&role, "CreateToolbox"));
        assert!(action_allowed(&role, "StartService"));
        assert!(action_allowed(&role, "SetHostname"));
        assert!(action_allowed(&role, "ConfigureWifi"));
        assert!(action_allowed(&role, "CreateUser"));
        assert!(action_allowed(&role, "UpdateSystem"));
        assert!(action_allowed(&role, "RebaseSystem"));
        assert!(action_allowed(&role, "RebootSystem"));
        assert!(action_allowed(&role, "InstallPackages"));
        assert!(action_allowed(&role, "AddUserToGroup"));
        assert!(action_allowed(&role, "RemoveUserFromGroup"));
    }

    // ------------------------------------------------------------------
    // Unknown actions are denied
    // ------------------------------------------------------------------

    #[test]
    fn unknown_action_denied_for_all_roles() {
        assert!(!action_allowed(&CallerRole::Observer, "NonExistent"));
        assert!(!action_allowed(&CallerRole::Dev, "NonExistent"));
        assert!(!action_allowed(&CallerRole::Admin, "NonExistent"));
        assert!(!action_allowed(&CallerRole::Boot, "NonExistent"));
    }
}
