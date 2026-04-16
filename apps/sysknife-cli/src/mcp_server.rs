//! MCP server entry point for `sysknife mcp-server`.
//!
//! Exposes a single `lacs_plan` tool that takes a natural-language intent
//! and returns the typed plan JSON produced by `sysknife-brain`.
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
use serde::Deserialize;

use sysknife_brain::config::BrainConfig;
use sysknife_brain::planner::LlmPlanner;

use crate::client::DaemonClient;
use crate::error::CliError;
use crate::runner::resolve_socket;

// ---------------------------------------------------------------------------
// Input schema
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PlanInput {
    /// Natural-language intent, e.g. "show disk usage" or "add vim to my system".
    pub intent: String,
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LacsMcpServer;

#[tool_router(server_handler)]
impl LacsMcpServer {
    /// Plan a system administration intent.
    ///
    /// Returns a JSON object with the proposed steps, each carrying an
    /// `action_name`, `summary`, `risk_level` ("low" | "medium" | "high"),
    /// and `params`. No action is executed — this is plan-only.
    #[tool(
        description = "Plan a Linux system administration intent. Returns typed steps with risk levels (low/medium/high). No action is executed — LACS always requires explicit approval before touching the system."
    )]
    async fn lacs_plan(
        &self,
        Parameters(PlanInput { intent }): Parameters<PlanInput>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        plan_intent_inner(&intent)
            .await
            .map(Json)
            .map_err(|e| ErrorData::internal_error(e, None))
    }
}

// ---------------------------------------------------------------------------
// Planner helper
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
