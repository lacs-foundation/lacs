//! MCP server entry point for `sysknife mcp-server`.
//!
//! Exposes two tools:
//!
//! - `lacs_plan`    — turn a natural-language intent into a risk-labelled plan.
//! - `lacs_execute` — execute a plan returned by `lacs_plan`.
//!
//! Typical agentic loop:
//!
//! 1. Call `lacs_plan { intent }` — show the plan to the user, explain risk.
//! 2. User approves.
//! 3. Call `lacs_execute { steps, max_risk }` — daemon runs each step and
//!    streams output back as collected lines.
//!
//! The server uses stdio transport so any MCP client (Claude Desktop,
//! Cursor, …) can launch it as a local subprocess.
//!
//! Example `claude_desktop_config.json` entry:
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "sysknife": { "command": "sysknife", "args": ["mcp-server"] }
//!   }
//! }
//! ```

use rmcp::{
    handler::server::wrapper::{Json, Parameters},
    schemars, tool, tool_router,
    transport::stdio,
    ErrorData, ServiceExt,
};
use serde::{Deserialize, Serialize};
use sysknife_types::RiskLevel;

use sysknife_brain::config::BrainConfig;
use sysknife_brain::planner::LlmPlanner;

use crate::client::DaemonClient;
use crate::error::CliError;
use crate::runner::resolve_socket;

// ---------------------------------------------------------------------------
// lacs_plan — input / output types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PlanInput {
    /// Natural-language intent, e.g. "show disk usage" or "add vim to my system".
    pub intent: String,
}

/// One action step in the proposed plan.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanStepOutput {
    /// Canonical action name from the SysKnife catalogue.
    pub action_name: String,
    /// Human-readable description of what this step does.
    pub summary: String,
    /// Risk level: `"low"`, `"medium"`, or `"high"`.
    pub risk_level: String,
    /// Action-specific parameters.
    pub params: serde_json::Value,
}

/// The full plan returned by `lacs_plan`.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanOutput {
    /// The original natural-language intent.
    pub intent: String,
    /// One-line summary of the plan.
    pub summary: String,
    /// Longer explanation of why this plan was chosen.
    pub explanation: String,
    /// Ordered list of steps to execute.
    pub steps: Vec<PlanStepOutput>,
}

// ---------------------------------------------------------------------------
// lacs_execute — input / output types
// ---------------------------------------------------------------------------

/// A single step to execute, taken verbatim from `lacs_plan` output.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StepToExecute {
    /// Canonical action name from the SysKnife catalogue, e.g. `"GetDiskUsage"`.
    pub action_name: String,
    /// Action-specific parameters (pass through from the plan unchanged).
    pub params: serde_json::Value,
}

/// Input to `lacs_execute`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExecuteInput {
    /// Steps to execute — take the `steps` array from `lacs_plan` output.
    pub steps: Vec<StepToExecute>,
    /// Highest risk level you are willing to execute without further
    /// confirmation.  One of `"low"`, `"medium"`, `"high"`.
    /// Defaults to `"medium"` if omitted.
    ///
    /// Steps whose daemon-assessed risk exceeds this ceiling cause the
    /// tool to return an error before any execution occurs.
    pub max_risk: Option<String>,
}

/// Execution result for a single step.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct StepResult {
    /// Action that was executed.
    pub action_name: String,
    /// Final status: `"succeeded"`, `"failed"`, `"needs_reboot"`, etc.
    pub status: String,
    /// Human-readable summary from the daemon.
    pub summary: String,
    /// Progress lines collected during execution (ANSI stripped).
    pub output: Vec<String>,
    /// Warnings emitted by the daemon for this step.
    pub warnings: Vec<String>,
    /// Whether this step requires a reboot to take effect.
    pub needs_reboot: bool,
    /// Daemon transaction ID for audit purposes.
    pub transaction_id: String,
}

/// Output of `lacs_execute`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ExecuteOutput {
    /// Results for each executed step, in order.
    pub steps: Vec<StepResult>,
    /// True if any step requires a reboot to take effect.
    pub needs_reboot: bool,
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LacsMcpServer;

#[tool_router(server_handler)]
impl LacsMcpServer {
    /// Plan a Linux system administration intent.
    ///
    /// Returns a JSON object with the proposed steps, each carrying an
    /// `action_name`, `summary`, `risk_level` ("low" | "medium" | "high"),
    /// and `params`. No action is executed — call `lacs_execute` with the
    /// returned steps after the user approves the plan.
    #[tool(
        description = "Plan a Linux system administration intent. Returns typed steps with risk levels (low/medium/high). No action is executed — call lacs_execute with the returned steps after the user approves."
    )]
    async fn lacs_plan(
        &self,
        Parameters(PlanInput { intent }): Parameters<PlanInput>,
    ) -> Result<Json<PlanOutput>, ErrorData> {
        let value = plan_intent_inner(&intent)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        let output: PlanOutput = serde_json::from_value(value).map_err(|e| {
            ErrorData::internal_error(format!("output deserialization error: {e}"), None)
        })?;
        Ok(Json(output))
    }

    /// Execute a plan produced by `lacs_plan`.
    ///
    /// Pass the `steps` array from `lacs_plan` output unchanged.  Set
    /// `max_risk` to the highest risk level you are willing to execute
    /// without further confirmation (`"low"` | `"medium"` | `"high"`;
    /// defaults to `"medium"`).
    ///
    /// Steps whose daemon-assessed risk exceeds `max_risk` cause an error
    /// before any execution occurs.  On failure mid-plan execution stops
    /// immediately and the error is returned.
    ///
    /// Returns per-step results including output lines, warnings, and
    /// whether a reboot is required.
    #[tool(
        description = "Execute a plan produced by lacs_plan. Pass the steps array unchanged. Set max_risk to the highest risk you will execute without confirmation (low/medium/high, default medium). Returns per-step output, warnings, and reboot requirements."
    )]
    async fn lacs_execute(
        &self,
        Parameters(ExecuteInput { steps, max_risk }): Parameters<ExecuteInput>,
    ) -> Result<Json<ExecuteOutput>, ErrorData> {
        execute_steps_inner(steps, max_risk.as_deref())
            .await
            .map(Json)
            .map_err(|e| ErrorData::internal_error(e, None))
    }
}

// ---------------------------------------------------------------------------
// lacs_plan helper
// ---------------------------------------------------------------------------

async fn plan_intent_inner(intent: &str) -> Result<serde_json::Value, String> {
    let config = BrainConfig::from_env().map_err(|e| format!("config error: {e}"))?;

    let socket = resolve_socket();
    let state_client = DaemonClient::new(socket);

    let planner = LlmPlanner::from_config(config, Box::new(state_client))
        .map_err(|e| format!("planner init error: {e}"))?;

    // `plan_intent` may call `StateClient::curated_state()` (a blocking sync
    // Unix socket call) on the current async thread.  This is tolerable on
    // the multi-threaded runtime: the call is bounded by SOCKET_TIMEOUT (10 s)
    // and ties up one worker thread for at most that duration.  MCP sessions
    // are LLM-driven and sequential in practice, so concurrent saturation of
    // the thread pool is not a realistic concern here.
    let plan = planner
        .plan_intent(intent)
        .await
        .map_err(|e| format!("planning error: {e}"))?;

    serde_json::to_value(&plan).map_err(|e| format!("serialization error: {e}"))
}

// ---------------------------------------------------------------------------
// lacs_execute helper
// ---------------------------------------------------------------------------

async fn execute_steps_inner(
    steps: Vec<StepToExecute>,
    max_risk: Option<&str>,
) -> Result<ExecuteOutput, String> {
    let ceiling = parse_max_risk(max_risk)?;
    let socket = resolve_socket();
    let client = DaemonClient::new(socket);

    let mut results: Vec<StepResult> = Vec::new();
    let mut plan_needs_reboot = false;

    for step in steps {
        // Preview: get daemon's authoritative risk assessment + request_hash.
        let preview = client
            .preview(&step.action_name, &step.params)
            .await
            .map_err(|e| format!("preview error for {}: {e}", step.action_name))?;

        // Risk gate: check daemon-assessed risk against the ceiling.
        check_risk_ceiling(&preview.risk_level, ceiling).map_err(|_| {
            format!(
                "step '{}' has risk '{:?}' which exceeds max_risk ceiling '{}'",
                step.action_name,
                preview.risk_level,
                max_risk.unwrap_or("medium"),
            )
        })?;

        // Execute and collect progress lines.
        let mut output_lines: Vec<String> = Vec::new();
        let result = client
            .execute(
                &step.action_name,
                &step.params,
                &preview.request_hash,
                |line| output_lines.push(line.to_owned()),
            )
            .await
            .map_err(|e| format!("execute error for {}: {e}", step.action_name))?;

        let needs_reboot = result.needs_reboot;
        if needs_reboot {
            plan_needs_reboot = true;
        }

        let status = serde_json::to_value(&result.status)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| "unknown".into());

        let succeeded = matches!(result.status, sysknife_types::JobState::Succeeded);

        results.push(StepResult {
            action_name: step.action_name,
            status,
            summary: result.summary,
            output: output_lines,
            warnings: result.warnings,
            needs_reboot,
            transaction_id: result.transaction_id,
        });

        // Halt on first failure — do not continue executing subsequent steps.
        if !succeeded {
            break;
        }
    }

    Ok(ExecuteOutput {
        steps: results,
        needs_reboot: plan_needs_reboot,
    })
}

// ---------------------------------------------------------------------------
// Pure helpers (also tested below)
// ---------------------------------------------------------------------------

/// Parse a `max_risk` string into an ordinal `u8` (0=low, 1=medium, 2=high).
/// `None` defaults to medium (1).
fn parse_max_risk(s: Option<&str>) -> Result<u8, String> {
    match s.unwrap_or("medium") {
        "low" => Ok(0),
        "medium" => Ok(1),
        "high" => Ok(2),
        other => Err(format!(
            "invalid max_risk {other:?}: expected \"low\", \"medium\", or \"high\""
        )),
    }
}

/// Convert a daemon `RiskLevel` to an ordinal comparable against `parse_max_risk`.
fn risk_level_ord(r: &RiskLevel) -> u8 {
    match r {
        RiskLevel::Low => 0,
        RiskLevel::Medium => 1,
        RiskLevel::High => 2,
    }
}

/// Return `Err(())` if `risk` exceeds `ceiling`.
fn check_risk_ceiling(risk: &RiskLevel, ceiling: u8) -> Result<(), ()> {
    if risk_level_ord(risk) > ceiling {
        Err(())
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub async fn run_mcp_server() -> Result<(), CliError> {
    let service = LacsMcpServer
        .serve(stdio())
        .await
        .map_err(|e| CliError::ExecutionFailed(format!("MCP server error: {e}")))?;

    service
        .waiting()
        .await
        .map_err(|e| CliError::ExecutionFailed(format!("MCP server wait error: {e}")))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // parse_max_risk
    // -----------------------------------------------------------------------

    #[test]
    fn parse_max_risk_none_defaults_to_medium() {
        assert_eq!(parse_max_risk(None), Ok(1));
    }

    #[test]
    fn parse_max_risk_low() {
        assert_eq!(parse_max_risk(Some("low")), Ok(0));
    }

    #[test]
    fn parse_max_risk_medium() {
        assert_eq!(parse_max_risk(Some("medium")), Ok(1));
    }

    #[test]
    fn parse_max_risk_high() {
        assert_eq!(parse_max_risk(Some("high")), Ok(2));
    }

    #[test]
    fn parse_max_risk_unknown_returns_err() {
        assert!(parse_max_risk(Some("extreme")).is_err());
        assert!(parse_max_risk(Some("")).is_err());
        assert!(parse_max_risk(Some("HIGH")).is_err()); // case-sensitive
    }

    // -----------------------------------------------------------------------
    // risk_level_ord
    // -----------------------------------------------------------------------

    #[test]
    fn risk_level_ord_ordering() {
        assert!(risk_level_ord(&RiskLevel::Low) < risk_level_ord(&RiskLevel::Medium));
        assert!(risk_level_ord(&RiskLevel::Medium) < risk_level_ord(&RiskLevel::High));
    }

    // -----------------------------------------------------------------------
    // check_risk_ceiling
    // -----------------------------------------------------------------------

    #[test]
    fn check_risk_ceiling_within_ceiling_is_ok() {
        // low step, ceiling=medium
        assert!(check_risk_ceiling(&RiskLevel::Low, 1).is_ok());
        // medium step, ceiling=medium
        assert!(check_risk_ceiling(&RiskLevel::Medium, 1).is_ok());
        // high step, ceiling=high
        assert!(check_risk_ceiling(&RiskLevel::High, 2).is_ok());
        // exact match at every level
        assert!(check_risk_ceiling(&RiskLevel::Low, 0).is_ok());
    }

    #[test]
    fn check_risk_ceiling_exceeds_ceiling_is_err() {
        // medium step, ceiling=low
        assert!(check_risk_ceiling(&RiskLevel::Medium, 0).is_err());
        // high step, ceiling=low
        assert!(check_risk_ceiling(&RiskLevel::High, 0).is_err());
        // high step, ceiling=medium
        assert!(check_risk_ceiling(&RiskLevel::High, 1).is_err());
    }
}
