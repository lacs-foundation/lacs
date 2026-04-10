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

#[tauri::command]
pub fn plan_intent(intent: String) -> Result<PlanResponse, String> {
    let state = ShellCommandState::new();
    let plan = state
        .planner
        .plan_intent(&intent)
        .map_err(|err| err.to_string())?;
    Ok(plan_to_response(plan))
}

#[tauri::command]
pub fn approve_preview(app: AppHandle, request_hash: String) -> Result<(), String> {
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

fn plan_to_response(plan: Plan) -> PlanResponse {
    PlanResponse {
        summary: plan.summary,
        preview: ShellPreview {
            summary: format!("Preview for {}", plan.intent),
        },
        approval_required: plan.steps.iter().any(|step| step.approval_required),
    }
}
