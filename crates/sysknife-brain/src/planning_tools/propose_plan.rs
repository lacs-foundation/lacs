//! The `propose_plan` planning tool definition and output parser.
//!
//! This tool is the only mechanism by which the planner loop terminates
//! successfully. When the LLM calls it, the planner parses the structured
//! input into a validated `Plan` and returns it.

use crate::action_name::ActionName;
use crate::planner::{Plan, PlanRiskLevel, PlanStep, PlanningError};
use crate::provider::ToolDefinition;

// ---------------------------------------------------------------------------
// Tool definition
// ---------------------------------------------------------------------------

/// The approved list of SysKnife action names. This is the safety fence: the LLM
/// is shown this enum and can only produce names from it. Any name outside
/// this set is rejected by [`ActionName::parse`].
///
/// Must be kept in sync with the action catalogue in `sysknife-daemon`. The
/// cross-module consistency test in `sysknife-daemon/tests/action_consistency.rs`
/// verifies this at test time.
/// Each entry is `(action_name, one-line description)`.
///
/// The description is surfaced in the `action_name` field of the `propose_plan`
/// tool schema so the model can reason about which action to pick from its
/// *purpose* rather than just its name.
pub const KNOWN_ACTIONS: &[(&str, &str)] = &[
    // Deployment and boot
    ("GetSystemState",
     "full rpm-ostree deployment snapshot: layered packages, pinned/staged deployments, booted/pending OSTree refs"),
    ("CollectDiagnostics",
     "recent system journal log (last 500 lines) for error diagnosis and troubleshooting"),
    ("GetDeploymentHistory",
     "rpm-ostree deployment history: past and current OSTree commits with timestamps"),
    ("ListDeployments",
     "list all currently staged, pending, and booted deployments"),
    ("PinDeployment",
     "pin a specific deployment so it is not garbage-collected during cleanup"),
    ("UnpinDeployment",
     "unpin a previously pinned deployment, allowing it to be removed"),
    ("UpdateSystem",
     "download and stage the latest OSTree update (does not reboot)"),
    ("RebaseSystem",
     "switch the system to a different OSTree ref/remote (rebase)"),
    ("CleanupDeployments",
     "remove old staged deployments to free disk space"),
    ("RebootSystem",
     "reboot the machine into the current or staged deployment"),
    ("RollbackDeployment",
     "roll back to the previous booted deployment"),
    ("GetKernelArguments",
     "list current kernel command-line arguments (kargs)"),
    ("SetKernelArguments",
     "add, remove, or replace kernel command-line arguments"),
    // Flatpak
    ("InstallFlatpak",
     "install a Flatpak application from a configured remote"),
    ("RemoveFlatpak",
     "uninstall an installed Flatpak application"),
    ("UpdateFlatpak",
     "update one or all installed Flatpak applications"),
    ("SearchFlatpakApps",
     "search Flatpak remotes for applications matching a query"),
    ("ListFlatpakRemotes",
     "list configured Flatpak remotes (e.g. flathub)"),
    ("ListInstalledFlatpaks",
     "list all Flatpak applications currently installed on the system"),
    ("AddFlatpakRemote",
     "add a new Flatpak remote repository"),
    ("RemoveFlatpakRemote",
     "remove a configured Flatpak remote repository"),
    ("GetFlatpakAppInfo",
     "show metadata and runtime info for a specific installed Flatpak"),
    // Toolbox
    ("ListToolboxes",
     "list all toolbox containers (distrobox/podman toolbox)"),
    ("CreateToolbox",
     "create a new toolbox container with a specified image"),
    ("RemoveToolbox",
     "remove an existing toolbox container"),
    // Layering
    ("InstallPackages",
     "layer one or more RPM packages onto the immutable OS via rpm-ostree"),
    ("RemovePackages",
     "remove previously layered RPM packages from the OS"),
    ("GetLayeredPackages",
     "list all RPM packages currently layered on top of the base OS image"),
    ("AddLayeredPackage",
     "layer a single RPM package onto the OS (requires reboot to take effect)"),
    ("RemoveLayeredPackage",
     "remove a single layered RPM package (requires reboot)"),
    ("ReplaceLayeredPackage",
     "replace a base OS package with a different version via rpm-ostree override"),
    ("ResetLayeredPackageOverride",
     "reset a package override, restoring the base OS version"),
    ("RemoveBasePackage",
     "exclude a base OS package from the deployment"),
    ("GetPendingUpdates",
     "check whether a staged update is pending and show its diff"),
    // Services
    ("ListServices",
     "list all systemd units and their current active/enabled state"),
    ("StartService",
     "start a stopped systemd service unit"),
    ("StopService",
     "stop a running systemd service unit"),
    ("RestartService",
     "restart a systemd service unit (stop then start)"),
    ("ReloadService",
     "send SIGHUP to a service to reload its configuration without restarting"),
    ("ReloadDaemon",
     "run systemctl daemon-reload to pick up new or changed unit files"),
    ("SetServiceEnabled",
     "enable or disable a systemd service unit at boot"),
    ("MaskService",
     "mask a systemd unit so it cannot be started manually or automatically"),
    ("UnmaskService",
     "unmask a previously masked systemd unit"),
    ("GetServiceLogs",
     "fetch recent journald log lines for a specific systemd service"),
    ("GetServiceStatus",
     "show detailed status of a specific systemd service unit"),
    ("ListTimers",
     "list all active and loaded systemd timer units with their next trigger time"),
    // Network
    ("ConfigureWifi",
     "connect to a Wi-Fi network using an SSID and password via NetworkManager"),
    ("SetDnsServers",
     "set DNS resolver addresses for a network interface via NetworkManager"),
    ("ConfigureFirewall",
     "add or remove a service or port in a firewalld zone"),
    ("GetFirewallState",
     "show current firewalld zones, open services, and port rules"),
    ("GetNetworkStatus",
     "show network interfaces, IP addresses, and connection state (ip addr / nmcli)"),
    // Filesystem
    ("GetDiskUsage",
     "show disk space usage for all mounted filesystems (df -h)"),
    // Processes
    ("ListProcesses",
     "list running processes with CPU and memory usage (ps / top snapshot)"),
    // System info
    ("GetMemoryInfo",
     "show RAM and swap usage totals and availability (free -h)"),
    // Identity / time / locale
    ("GetDateTime",
     "current date, time, timezone, and NTP synchronisation status (timedatectl)"),
    ("SetHostname",
     "change the system hostname permanently via hostnamectl"),
    ("SetTimezone",
     "change the system timezone via timedatectl"),
    ("SetLocale",
     "change the system locale (language/region) via localectl"),
    ("SetNtp",
     "enable or disable NTP time synchronisation via timedatectl"),
    // Package repositories
    ("ListPackageRepositories",
     "list configured DNF/rpm-ostree package repositories and their enabled state"),
    ("AddPackageRepository",
     "add a new DNF package repository (repo file)"),
    ("RemovePackageRepository",
     "remove a configured DNF package repository"),
    ("EnablePackageRepository",
     "enable a disabled DNF package repository"),
    ("DisablePackageRepository",
     "disable an enabled DNF package repository without removing it"),
    // Containers
    ("ListContainers",
     "list all Podman containers (running and stopped) with their status"),
    ("CreateContainer",
     "create a new Podman container from an image"),
    ("StartContainer",
     "start a stopped Podman container"),
    ("StopContainer",
     "stop a running Podman container"),
    ("RemoveContainer",
     "remove a Podman container (must be stopped first)"),
    ("GetContainerInfo",
     "inspect a specific Podman container and show its detailed configuration"),
    // Users and groups
    ("ListUsers",
     "list all local user accounts on the system"),
    ("ListGroups",
     "list all local groups on the system"),
    ("CreateUser",
     "create a new local user account with optional shell and home directory"),
    ("DeleteUser",
     "delete a local user account (optionally remove home directory)"),
    ("AddUserToGroup",
     "add an existing user to a supplementary group"),
    ("RemoveUserFromGroup",
     "remove a user from a supplementary group"),
    // SSH
    ("GetAuthorizedKeys",
     "list SSH authorized_keys entries for a user"),
    ("AddAuthorizedKey",
     "append an SSH public key to a user's authorized_keys file"),
    ("RemoveAuthorizedKey",
     "remove a specific SSH public key from a user's authorized_keys file"),
    // Job history
    ("ListJobHistory",
     "show SysKnife's own past job execution log: what actions ran, when, and whether they succeeded"),
];

pub fn propose_plan_tool_def() -> ToolDefinition {
    let action_enum: Vec<serde_json::Value> = KNOWN_ACTIONS
        .iter()
        .map(|(name, _)| serde_json::Value::String((*name).into()))
        .collect();

    // Build a compact catalogue so the model can match intent to action by
    // purpose.  Format: "Name — description" on its own line.
    let action_catalogue: String = KNOWN_ACTIONS
        .iter()
        .map(|(name, desc)| format!("{name} — {desc}"))
        .collect::<Vec<_>>()
        .join("\n");

    let action_name_description = format!(
        "SysKnife action name from the approved list. \
         Choose by PURPOSE, not by name similarity. \
         Catalogue (name — what it does):\n{action_catalogue}"
    );

    ToolDefinition {
        name: "propose_plan".into(),
        description: "Emit the final typed SysKnife action plan. Call this exactly once after \
                       you have gathered enough information to make a confident plan. \
                       Each step must use an action_name from the approved list."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "One-sentence plain-language summary of the full plan."
                },
                "explanation": {
                    "type": "string",
                    "description": "2–4 sentence explanation for the user: what will happen, why, and what to watch for."
                },
                "steps": {
                    "type": "array",
                    "description": "Ordered list of SysKnife actions. Steps execute in order; a failure stops the plan.",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "action_name": {
                                "type": "string",
                                "enum": action_enum,
                                "description": action_name_description
                            },
                            "summary": {
                                "type": "string",
                                "description": "One sentence describing what this step does and why."
                            },
                            "risk_level": {
                                "type": "string",
                                "enum": ["low", "medium", "high"],
                                "description": "Risk classification for this step."
                            },
                            "params": {
                                "type": "string",
                                "description": "Action parameters encoded as a JSON string. Use \"{}\" for read-only actions (GetDateTime, GetDiskUsage, GetMemoryInfo, ListServices, GetAuthorizedKeys, etc.). For parameterized actions encode all required fields as JSON — examples by category:\n• Service: {\"unit\":\"sshd.service\"} for StartService/StopService/RestartService/GetServiceLogs/GetServiceStatus\n• Package: {\"package\":\"vim\"} for AddLayeredPackage/RemoveLayeredPackage\n• SSH keys: {\"username\":\"alice\",\"public_key\":\"ssh-ed25519 AAAA... comment\"} for AddAuthorizedKey/RemoveAuthorizedKey — copy the FULL key string verbatim from the intent\n• User mgmt: {\"username\":\"alice\",\"shell\":\"/bin/bash\",\"home\":\"/home/alice\"} for CreateUser; {\"username\":\"alice\"} for DeleteUser\n• Group mgmt: {\"username\":\"alice\",\"group\":\"wheel\"} for AddUserToGroup/RemoveUserFromGroup\n• Identity: {\"hostname\":\"myhost\"} for SetHostname; {\"timezone\":\"America/Chicago\"} for SetTimezone; {\"locale\":\"en_US.UTF-8\"} for SetLocale; {\"enabled\":true} for SetNtp\n• Firewall: {\"zone\":\"public\",\"service\":\"ssh\",\"enabled\":true} for ConfigureFirewall\n• Network: {\"ssid\":\"MyWifi\",\"password\":\"secret\"} for ConfigureWifi; {\"interface\":\"wlp1s0\",\"servers\":[\"1.1.1.1\"]} for SetDnsServers\nIMPORTANT: Extract parameter values verbatim from the user's intent. Never omit required fields."
                            }
                        },
                        "required": ["action_name", "summary", "risk_level", "params"]
                    }
                }
            },
            "required": ["summary", "explanation", "steps"]
        }),
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse and validate the `propose_plan` tool call input into a [`Plan`].
///
/// Validates:
/// - `summary` and `explanation` are present and non-empty
/// - `steps` is a non-empty array
/// - each step has a valid `action_name` (from [`KNOWN_ACTIONS`])
/// - each step has a valid `risk_level` ("low", "medium", "high")
/// - derives `approval_required` from risk level
pub fn parse_proposed_plan(intent: &str, input: &serde_json::Value) -> Result<Plan, PlanningError> {
    let summary = input
        .get("summary")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| PlanningError::InvalidPlanOutput("missing or empty 'summary'".into()))?;

    let explanation = input
        .get("explanation")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| PlanningError::InvalidPlanOutput("missing or empty 'explanation'".into()))?;

    let steps_value = input
        .get("steps")
        .and_then(|v| v.as_array())
        .ok_or_else(|| PlanningError::InvalidPlanOutput("'steps' must be an array".into()))?;

    if steps_value.is_empty() {
        return Err(PlanningError::InvalidPlanOutput(
            "'steps' must not be empty".into(),
        ));
    }

    let mut steps = Vec::with_capacity(steps_value.len());

    for (i, step_val) in steps_value.iter().enumerate() {
        let action_name_str = step_val
            .get("action_name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                PlanningError::InvalidPlanOutput(format!("step {i}: missing 'action_name'"))
            })?;

        let action_name = ActionName::parse(action_name_str).map_err(|_| {
            PlanningError::InvalidPlanOutput(format!(
                "step {i}: unknown action_name '{action_name_str}'"
            ))
        })?;

        let step_summary = step_val
            .get("summary")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                PlanningError::InvalidPlanOutput(format!("step {i}: missing 'summary'"))
            })?;

        let risk_str = step_val
            .get("risk_level")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                PlanningError::InvalidPlanOutput(format!("step {i}: missing 'risk_level'"))
            })?;

        let risk_level = match risk_str {
            "low" => PlanRiskLevel::Low,
            "medium" => PlanRiskLevel::Medium,
            "high" => PlanRiskLevel::High,
            other => {
                return Err(PlanningError::InvalidPlanOutput(format!(
                    "step {i}: invalid risk_level '{other}'"
                )));
            }
        };

        // `params` may arrive as a JSON-encoded string (OpenAI Responses API
        // strict-mode providers) or as a plain object (Ollama and others).
        // Both are normalised to a JSON object here. Empty strings are treated
        // as `{}`. Non-empty strings that are not valid JSON are rejected so
        // that malformed params (e.g. the LLM passing a bare word like `"vim"`)
        // are caught at parse time rather than silently becoming `{}`.
        let params = match step_val.get("params") {
            Some(serde_json::Value::String(s)) if s.is_empty() => {
                serde_json::Value::Object(serde_json::Map::new())
            }
            Some(serde_json::Value::String(s)) => serde_json::from_str(s).map_err(|_| {
                PlanningError::InvalidPlanOutput(format!(
                    "step {i}: 'params' is not valid JSON: {s:?}"
                ))
            })?,
            Some(v) => v.clone(),
            None => serde_json::Value::Object(serde_json::Map::new()),
        };

        steps.push(PlanStep::new(
            action_name,
            step_summary.to_string(),
            risk_level,
            params,
        )?);
    }

    Ok(Plan::new(
        intent.to_string(),
        summary.to_string(),
        explanation.to_string(),
        steps,
    )?)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_input(risk: &str) -> serde_json::Value {
        serde_json::json!({
            "summary": "do the thing",
            "explanation": "this does the thing for the reason",
            "steps": [{
                "action_name": "GetSystemState",
                "summary": "read state",
                "risk_level": risk,
                "params": {}
            }]
        })
    }

    #[test]
    fn valid_low_risk_plan_parses() {
        let plan = parse_proposed_plan("intent", &valid_input("low")).unwrap();
        assert_eq!(plan.steps().len(), 1);
        assert!(!plan.steps()[0].approval_required());
    }

    #[test]
    fn medium_risk_requires_approval() {
        let input = serde_json::json!({
            "summary": "configure wifi",
            "explanation": "connects to wifi",
            "steps": [{
                "action_name": "ConfigureWifi",
                "summary": "connect",
                "risk_level": "medium",
                "params": {}
            }]
        });
        let plan = parse_proposed_plan("wifi", &input).unwrap();
        assert!(plan.steps()[0].approval_required());
    }

    #[test]
    fn high_risk_requires_approval() {
        let input = serde_json::json!({
            "summary": "rebase",
            "explanation": "rebases system",
            "steps": [{
                "action_name": "RebaseSystem",
                "summary": "rebase to f42",
                "risk_level": "high",
                "params": {}
            }]
        });
        let plan = parse_proposed_plan("rebase", &input).unwrap();
        assert!(plan.steps()[0].approval_required());
    }

    #[test]
    fn unknown_action_name_is_rejected() {
        let input = serde_json::json!({
            "summary": "bad",
            "explanation": "bad plan",
            "steps": [{
                "action_name": "RunShellCommand",
                "summary": "run stuff",
                "risk_level": "low",
                "params": {}
            }]
        });
        let err = parse_proposed_plan("intent", &input).unwrap_err();
        assert!(matches!(err, PlanningError::InvalidPlanOutput(_)));
    }

    #[test]
    fn empty_steps_rejected() {
        let input = serde_json::json!({
            "summary": "nothing",
            "explanation": "no steps",
            "steps": []
        });
        assert!(matches!(
            parse_proposed_plan("intent", &input).unwrap_err(),
            PlanningError::InvalidPlanOutput(_)
        ));
    }

    #[test]
    fn missing_summary_rejected() {
        let input = serde_json::json!({
            "explanation": "test",
            "steps": [{ "action_name": "GetSystemState", "summary": "s", "risk_level": "low", "params": {} }]
        });
        assert!(matches!(
            parse_proposed_plan("intent", &input).unwrap_err(),
            PlanningError::InvalidPlanOutput(_)
        ));
    }

    #[test]
    fn params_passthrough() {
        let input = serde_json::json!({
            "summary": "install vim",
            "explanation": "layers vim",
            "steps": [{
                "action_name": "InstallPackages",
                "summary": "layer vim",
                "risk_level": "high",
                "params": { "packages": ["vim"] }
            }]
        });
        let plan = parse_proposed_plan("vim", &input).unwrap();
        assert_eq!(plan.steps()[0].params()["packages"][0], "vim");
    }

    #[test]
    fn params_string_invalid_json_is_rejected() {
        // A bare word like "vim" is not valid JSON and must not silently become {}.
        let input = serde_json::json!({
            "summary": "install vim",
            "explanation": "layers vim",
            "steps": [{
                "action_name": "AddLayeredPackage",
                "summary": "layer vim",
                "risk_level": "high",
                "params": "vim"
            }]
        });
        assert!(matches!(
            parse_proposed_plan("vim", &input).unwrap_err(),
            PlanningError::InvalidPlanOutput(_)
        ));
    }

    #[test]
    fn params_string_empty_normalises_to_object() {
        let input = serde_json::json!({
            "summary": "read state",
            "explanation": "reads state",
            "steps": [{
                "action_name": "GetSystemState",
                "summary": "read",
                "risk_level": "low",
                "params": ""
            }]
        });
        let plan = parse_proposed_plan("read", &input).unwrap();
        assert_eq!(plan.steps()[0].params(), &serde_json::json!({}));
    }

    #[test]
    fn all_known_actions_are_accepted() {
        for &(action, _) in KNOWN_ACTIONS {
            let input = serde_json::json!({
                "summary": "test",
                "explanation": "test",
                "steps": [{ "action_name": action, "summary": "s", "risk_level": "low", "params": {} }]
            });
            parse_proposed_plan("test", &input)
                .unwrap_or_else(|e| panic!("action '{action}' rejected: {e}"));
        }
    }

    // -- Explanation validation -----------------------------------------------

    #[test]
    fn missing_explanation_rejected() {
        let input = serde_json::json!({
            "summary": "test",
            "steps": [{ "action_name": "GetSystemState", "summary": "s", "risk_level": "low", "params": {} }]
        });
        assert!(matches!(
            parse_proposed_plan("intent", &input).unwrap_err(),
            PlanningError::InvalidPlanOutput(_)
        ));
    }

    #[test]
    fn empty_explanation_rejected() {
        let input = serde_json::json!({
            "summary": "test",
            "explanation": "",
            "steps": [{ "action_name": "GetSystemState", "summary": "s", "risk_level": "low", "params": {} }]
        });
        assert!(matches!(
            parse_proposed_plan("intent", &input).unwrap_err(),
            PlanningError::InvalidPlanOutput(_)
        ));
    }

    // -- Steps array validation -----------------------------------------------

    #[test]
    fn steps_not_an_array_is_rejected() {
        let input = serde_json::json!({
            "summary": "bad",
            "explanation": "bad plan",
            "steps": "GetSystemState"
        });
        assert!(matches!(
            parse_proposed_plan("intent", &input).unwrap_err(),
            PlanningError::InvalidPlanOutput(_)
        ));
    }

    // -- Step field validation -------------------------------------------------

    #[test]
    fn step_missing_risk_level_rejected() {
        let input = serde_json::json!({
            "summary": "test",
            "explanation": "test",
            "steps": [{
                "action_name": "RebaseSystem",
                "summary": "rebase",
                "params": {}
            }]
        });
        assert!(matches!(
            parse_proposed_plan("intent", &input).unwrap_err(),
            PlanningError::InvalidPlanOutput(_)
        ));
    }

    #[test]
    fn invalid_risk_level_strings_rejected() {
        for bad in &["critical", "none", "HIGH", "LOW", "0", "unknown"] {
            let input = serde_json::json!({
                "summary": "test",
                "explanation": "test",
                "steps": [{ "action_name": "GetSystemState", "summary": "s",
                            "risk_level": bad, "params": {} }]
            });
            assert!(
                matches!(
                    parse_proposed_plan("intent", &input).unwrap_err(),
                    PlanningError::InvalidPlanOutput(_)
                ),
                "risk_level '{bad}' should be rejected"
            );
        }
    }

    #[test]
    fn list_job_history_is_accepted() {
        let input = serde_json::json!({
            "summary": "show history",
            "explanation": "shows recent SysKnife actions",
            "steps": [{ "action_name": "ListJobHistory", "summary": "show recent activity", "risk_level": "low", "params": {} }]
        });
        parse_proposed_plan("show history", &input).unwrap();
    }
}
