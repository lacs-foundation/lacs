use crate::events::{DaemonJobOutcome, TimelineEvent};
use lacs_brain::planner::{Plan, Planner};
use lacs_brain::state_client::{CuratedState, StateClient};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanResponse {
    pub summary: String,
    pub preview: ShellPreview,
    pub approval_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellPreview {
    pub summary: String,
}

// NOTE(task-8): DemoStateClient returns a hardcoded Silverblue fixture.
// Replace with a real StateClient that queries the daemon over the Unix socket
// before this shell is used in a production context.
#[derive(Clone, Debug, Default)]
pub struct DemoStateClient;

impl StateClient for DemoStateClient {
    fn curated_state(&self) -> Result<CuratedState, lacs_brain::planner::PlanningError> {
        Ok(CuratedState {
            host_name: "silverblue".to_string(),
            deployment: "fedora/41".to_string(),
            services: vec!["NetworkManager.service".to_string()],
            flatpaks: vec!["org.mozilla.firefox".to_string()],
            toolboxes: vec!["lacs-dev".to_string()],
        })
    }
}

pub struct ShellCommandState {
    planner: Planner<DemoStateClient>,
}

impl ShellCommandState {
    pub fn new() -> Self {
        Self {
            planner: Planner::new(DemoStateClient),
        }
    }
}

impl Default for ShellCommandState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn plan_intent(
    state: tauri::State<ShellCommandState>,
    intent: String,
) -> Result<PlanResponse, String> {
    execute_plan_intent(&state, &intent)
}

#[tauri::command]
pub fn approve_preview(app: AppHandle, request_hash: String) -> Result<(), String> {
    // NOTE(task-8): This currently emits a frontend event only.
    // Wire to the daemon Unix socket to forward approval before production use.
    app.emit("lacs:approval-granted", request_hash)
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn publish_timeline_event(app: AppHandle, text: String) -> Result<(), String> {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| err.to_string())?
        .as_nanos()
        .to_string();

    app.emit("lacs:timeline-entry", TimelineEvent { id, text })
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn publish_job_outcome(app: AppHandle, outcome: DaemonJobOutcome) -> Result<(), String> {
    app.emit("lacs:job-completed", outcome)
        .map_err(|err| err.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers (extracted so they are testable without a Tauri runtime)
// ---------------------------------------------------------------------------

pub(crate) fn execute_plan_intent(
    state: &ShellCommandState,
    intent: &str,
) -> Result<PlanResponse, String> {
    let plan = state
        .planner
        .plan_intent(intent)
        .map_err(|err| err.to_string())?;
    Ok(plan_to_response(plan))
}

fn plan_to_response(plan: Plan) -> PlanResponse {
    PlanResponse {
        summary: plan.summary().to_string(),
        preview: ShellPreview {
            summary: format!("Preview for {}", plan.intent()),
        },
        approval_required: plan.steps().iter().any(|step| step.approval_required()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_to_response_sets_approval_required_for_mutating_intent() {
        let state = ShellCommandState::new();
        let response = execute_plan_intent(&state, "update the system").unwrap();
        assert!(
            response.approval_required,
            "a mutating intent must produce approval_required = true"
        );
    }

    #[test]
    fn plan_to_response_clears_approval_required_for_read_only_intent() {
        let state = ShellCommandState::new();
        let response = execute_plan_intent(&state, "show me the state").unwrap();
        assert!(
            !response.approval_required,
            "a read-only intent must produce approval_required = false"
        );
    }

    #[test]
    fn execute_plan_intent_rejects_empty_intent_with_descriptive_error() {
        let state = ShellCommandState::new();
        let err = execute_plan_intent(&state, "").unwrap_err();
        assert!(
            err.contains("empty"),
            "error message should mention 'empty', got: {err}"
        );
    }
}
