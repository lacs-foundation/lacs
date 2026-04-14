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

/// The approved list of LACS action names. This is the safety fence: the LLM
/// is shown this enum and can only produce names from it. Any name outside
/// this set is rejected by [`ActionName::parse`].
///
/// Must be kept in sync with the action catalogue in `lacs-daemon`. The
/// cross-module consistency test in `lacs-daemon/tests/action_consistency.rs`
/// verifies this at test time.
pub const KNOWN_ACTIONS: &[&str] = &[
    // Deployment and boot
    "GetSystemState",
    "CollectDiagnostics",
    "GetDeploymentHistory",
    "ListDeployments",
    "PinDeployment",
    "UnpinDeployment",
    "UpdateSystem",
    "RebaseSystem",
    "CleanupDeployments",
    "RebootSystem",
    "RollbackDeployment",
    "GetKernelArguments",
    "SetKernelArguments",
    // Flatpak
    "InstallFlatpak",
    "RemoveFlatpak",
    "SearchFlatpakApps",
    "ListFlatpakRemotes",
    "AddFlatpakRemote",
    "RemoveFlatpakRemote",
    "GetFlatpakAppInfo",
    // Toolbox
    "ListToolboxes",
    "CreateToolbox",
    "RemoveToolbox",
    // Layering
    "InstallPackages",
    "RemovePackages",
    "GetLayeredPackages",
    "AddLayeredPackage",
    "RemoveLayeredPackage",
    "ReplaceLayeredPackage",
    "ResetLayeredPackageOverride",
    // Services
    "ListServices",
    "StartService",
    "StopService",
    "RestartService",
    "SetServiceEnabled",
    "MaskService",
    "UnmaskService",
    "GetServiceLogs",
    // Network
    "ConfigureWifi",
    "SetDnsServers",
    "ConfigureFirewall",
    "GetFirewallState",
    "GetNetworkStatus",
    // Filesystem
    "GetDiskUsage",
    // Processes
    "ListProcesses",
    // System info
    "GetMemoryInfo",
    // Identity / time / locale
    "SetHostname",
    "SetTimezone",
    "SetLocale",
    "SetNtp",
    // Package repositories
    "ListPackageRepositories",
    "AddPackageRepository",
    "RemovePackageRepository",
    "EnablePackageRepository",
    "DisablePackageRepository",
    // Containers
    "ListContainers",
    "CreateContainer",
    "StartContainer",
    "StopContainer",
    "RemoveContainer",
    "GetContainerInfo",
    // Users and groups
    "ListUsers",
    "ListGroups",
    "CreateUser",
    "DeleteUser",
    "AddUserToGroup",
    "RemoveUserFromGroup",
    // SSH
    "GetAuthorizedKeys",
    "AddAuthorizedKey",
    "RemoveAuthorizedKey",
];

pub fn propose_plan_tool_def() -> ToolDefinition {
    let action_enum: Vec<serde_json::Value> = KNOWN_ACTIONS
        .iter()
        .map(|&s| serde_json::Value::String(s.into()))
        .collect();

    ToolDefinition {
        name: "propose_plan".into(),
        description: "Emit the final typed LACS action plan. Call this exactly once after \
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
                    "description": "Ordered list of LACS actions. Steps execute in order; a failure stops the plan.",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "action_name": {
                                "type": "string",
                                "enum": action_enum,
                                "description": "LACS action name from the approved list."
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
                                "description": "Action parameters encoded as a JSON string. Use \"{}\" for read-only actions (GetDiskUsage, GetMemoryInfo, ListServices, etc.). For parameterized actions encode as JSON: e.g. \"{\\\"unit\\\":\\\"sshd.service\\\"}\" for GetServiceLogs, \"{\\\"package\\\":\\\"vim\\\"}\" for AddLayeredPackage."
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
        for action in KNOWN_ACTIONS {
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
}
