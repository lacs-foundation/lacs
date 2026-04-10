use crate::events::{DaemonJobOutcome, TimelineEvent};
use lacs_brain::config::BrainConfig;
#[cfg(any(test, feature = "demo"))]
use lacs_brain::planner::PlanningError;
use lacs_brain::planner::{LlmPlanner, Plan};
#[cfg(any(test, feature = "demo"))]
use lacs_brain::state_client::{CuratedState, StateClient};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

// ---------------------------------------------------------------------------
// Response types (serialised to the frontend)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStepResponse {
    pub action_name: String,
    pub summary: String,
    pub risk_level: String,
    pub approval_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanResponse {
    pub summary: String,
    pub explanation: String,
    pub preview: ShellPreview,
    pub approval_required: bool,
    pub steps: Vec<PlanStepResponse>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellPreview {
    pub summary: String,
}

// ---------------------------------------------------------------------------
// Demo state client (hardcoded Silverblue fixture)
// ---------------------------------------------------------------------------

// TODO(daemon-ipc): DemoStateClient returns a hardcoded Silverblue fixture.
// Replace with a real StateClient that queries the daemon over the Unix socket
// (gRPC or Unix-domain IPC) before this shell is used in a production context.
#[cfg(any(test, feature = "demo"))]
#[derive(Clone, Debug, Default)]
pub struct DemoStateClient;

#[cfg(any(test, feature = "demo"))]
impl StateClient for DemoStateClient {
    fn curated_state(&self) -> Result<CuratedState, PlanningError> {
        Ok(CuratedState {
            host_name: "silverblue".to_string(),
            deployment: "fedora/41".to_string(),
            services: vec!["NetworkManager.service".to_string()],
            flatpaks: vec!["org.mozilla.firefox".to_string()],
            toolboxes: vec!["lacs-dev".to_string()],
        })
    }
}

// ---------------------------------------------------------------------------
// Shell command state
// ---------------------------------------------------------------------------

/// Returns the `StateClient` for the current build.
///
/// When the `demo` feature is disabled, this panics — replace with a
/// real daemon IPC client first.
#[cfg(any(test, feature = "demo"))]
fn build_state_client() -> Box<dyn StateClient> {
    Box::new(DemoStateClient)
}

#[cfg(not(any(test, feature = "demo")))]
fn build_state_client() -> Box<dyn StateClient> {
    panic!(
        "No StateClient available: enable the 'demo' feature or implement a real daemon IPC client"
    )
}

pub struct ShellCommandState {
    planner: LlmPlanner,
}

impl ShellCommandState {
    /// Create from environment-derived config.
    ///
    /// Logs a warning and falls back to Ollama defaults when `LACS_LLM_PROVIDER`
    /// is not set or the config is invalid, so the shell starts even without
    /// API credentials configured.
    ///
    /// Note: requires the `demo` feature (enabled by default) via
    /// `build_state_client()`. Replace `DemoStateClient` with a real daemon
    /// IPC client before disabling that feature.
    pub fn new() -> Self {
        let config = BrainConfig::from_env().unwrap_or_else(|err| {
            eprintln!("[LACS WARNING] Brain config error: {err}. Falling back to Ollama defaults.");
            BrainConfig::ollama_defaults()
        });
        let planner = LlmPlanner::from_config(config, build_state_client()).unwrap_or_else(|err| {
            eprintln!(
                "[LACS WARNING] Failed to build LLM provider: {err}. \
                     Check LACS_LLM_PROVIDER and related env vars."
            );
            LlmPlanner::from_config(BrainConfig::ollama_defaults(), build_state_client())
                .expect("Ollama defaults must always produce a valid planner")
        });
        Self { planner }
    }

    /// Inject a pre-built planner — used in unit tests.
    #[cfg(test)]
    pub fn with_planner(planner: LlmPlanner) -> Self {
        Self { planner }
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
pub async fn plan_intent(
    state: tauri::State<'_, ShellCommandState>,
    intent: String,
) -> Result<PlanResponse, String> {
    execute_plan_intent(&state, &intent).await
}

#[tauri::command]
pub fn approve_preview(app: AppHandle, request_hash: String) -> Result<(), String> {
    // TODO(daemon-ipc): This currently emits a frontend event only.
    // Wire to the daemon over gRPC / Unix-domain socket to forward approval
    // before production use so the daemon can execute the plan step.
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

pub(crate) async fn execute_plan_intent(
    state: &ShellCommandState,
    intent: &str,
) -> Result<PlanResponse, String> {
    let plan = state
        .planner
        .plan_intent(intent)
        .await
        .map_err(|err| err.to_string())?;
    Ok(plan_to_response(plan))
}

fn plan_to_response(plan: Plan) -> PlanResponse {
    let approval_required = plan.steps().iter().any(|step| step.approval_required());
    let steps = plan
        .steps()
        .iter()
        .map(|step| PlanStepResponse {
            action_name: step.action_name().to_string(),
            summary: step.summary().to_string(),
            risk_level: step.risk_level().as_str().to_string(),
            approval_required: step.approval_required(),
        })
        .collect();

    PlanResponse {
        summary: plan.summary().to_string(),
        explanation: plan.explanation().to_string(),
        preview: ShellPreview {
            summary: format!("Preview for {}", plan.intent()),
        },
        approval_required,
        steps,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use lacs_brain::planner::PlanRiskLevel;
    use lacs_brain::provider::{
        Completion, ContentBlock, LlmProvider, Message, ProviderError, StopReason, ToolDefinition,
    };
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct MockProvider {
        turns: Mutex<VecDeque<Result<Completion, ProviderError>>>,
    }

    impl MockProvider {
        fn once(turn: Result<Completion, ProviderError>) -> Self {
            Self {
                turns: Mutex::new(std::iter::once(turn).collect()),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn complete(
            &self,
            _system: &str,
            _messages: &[Message],
            _tools: &[ToolDefinition],
            _max_tokens: u32,
        ) -> Result<Completion, ProviderError> {
            self.turns
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(ProviderError::Parse("mock exhausted".into())))
        }
    }

    fn propose_plan_completion(
        summary: &str,
        explanation: &str,
        steps: &[(&str, &str, &str)],
    ) -> Result<Completion, ProviderError> {
        let steps_json: Vec<serde_json::Value> = steps
            .iter()
            .map(|(name, s, risk)| {
                serde_json::json!({
                    "action_name": name,
                    "summary": s,
                    "risk_level": risk,
                    "params": {}
                })
            })
            .collect();

        Ok(Completion {
            content: vec![ContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "propose_plan".into(),
                input: serde_json::json!({
                    "summary": summary,
                    "explanation": explanation,
                    "steps": steps_json
                }),
            }],
            stop_reason: StopReason::ToolUse,
        })
    }

    #[tokio::test]
    async fn empty_intent_returns_error_with_message_about_empty() {
        // ShellCommandState::new() would call BrainConfig::from_env() which
        // may configure a real provider. Use with_planner() to inject a mock.
        let planner = LlmPlanner::new(
            Box::new(MockProvider::once(Err(ProviderError::Parse(
                "unused".into(),
            )))),
            Box::new(DemoStateClient),
            5,
        );
        let state = ShellCommandState::with_planner(planner);
        let err = execute_plan_intent(&state, "").await.unwrap_err();
        assert!(
            err.contains("empty"),
            "expected 'empty' in error, got: {err}"
        );
    }

    #[tokio::test]
    async fn plan_to_response_serialises_approval_required_correctly() {
        let planner = LlmPlanner::new(
            Box::new(MockProvider::once(propose_plan_completion(
                "Inspect system state",
                "This plan reads the current system state.",
                &[("GetSystemState", "Read state", "low")],
            ))),
            Box::new(DemoStateClient),
            5,
        );
        let state = ShellCommandState::with_planner(planner);
        let response = execute_plan_intent(&state, "show me the system")
            .await
            .unwrap();
        assert!(!response.approval_required);
        assert_eq!(response.steps.len(), 1);
        assert_eq!(response.steps[0].action_name, "GetSystemState");
        assert_eq!(response.steps[0].risk_level, "low");
    }

    #[tokio::test]
    async fn plan_to_response_sets_approval_required_for_mutating_step() {
        let planner = LlmPlanner::new(
            Box::new(MockProvider::once(propose_plan_completion(
                "Install vim",
                "Layers vim via rpm-ostree.",
                &[("InstallPackages", "Layer vim", "high")],
            ))),
            Box::new(DemoStateClient),
            5,
        );
        let state = ShellCommandState::with_planner(planner);
        let response = execute_plan_intent(&state, "install vim").await.unwrap();
        assert!(response.approval_required);
    }

    #[test]
    fn plan_to_response_maps_all_fields() {
        use lacs_brain::planner::{Plan, PlanStep};

        let step = PlanStep::new(
            "RebaseSystem".into(),
            "Rebase to f42".into(),
            PlanRiskLevel::High,
            serde_json::json!({}),
        );
        let plan = Plan::new(
            "rebase intent".into(),
            "Rebase the system".into(),
            "This rebases Fedora Silverblue to f42 and requires a reboot.".into(),
            vec![step],
        );
        let resp = plan_to_response(plan);

        assert_eq!(resp.summary, "Rebase the system");
        assert_eq!(
            resp.explanation,
            "This rebases Fedora Silverblue to f42 and requires a reboot."
        );
        assert!(resp.approval_required);
        assert_eq!(resp.steps[0].risk_level, "high");
        assert_eq!(resp.preview.summary, "Preview for rebase intent");
    }

    #[tokio::test]
    async fn provider_error_surfaces_as_err_string() {
        // Verify that a ProviderError from plan_intent arrives at the frontend
        // as a non-empty Err(String) containing recognisable content. This pins
        // the execute_plan_intent → plan_intent → map_err chain.
        let planner = LlmPlanner::new(
            Box::new(MockProvider::once(Err(ProviderError::Http {
                status: 500,
                body: "internal server error".into(),
            }))),
            Box::new(DemoStateClient),
            5,
        );
        let state = ShellCommandState::with_planner(planner);
        let err = execute_plan_intent(&state, "install vim")
            .await
            .unwrap_err();
        assert!(
            err.contains("500") || err.contains("http"),
            "provider HTTP error must appear in err string, got: {err}"
        );
    }

    #[test]
    fn plan_to_response_approval_required_when_any_step_is_high_risk() {
        // Mixed plan: first step is low, second is high. The aggregated
        // approval_required must be true (any() semantics, not all()).
        use lacs_brain::planner::{Plan, PlanStep};

        let steps = vec![
            PlanStep::new(
                "GetSystemState".into(),
                "Read current state".into(),
                PlanRiskLevel::Low,
                serde_json::json!({}),
            ),
            PlanStep::new(
                "InstallPackages".into(),
                "Layer vim via rpm-ostree".into(),
                PlanRiskLevel::High,
                serde_json::json!({}),
            ),
        ];
        let plan = Plan::new(
            "install vim intent".into(),
            "Install vim on the system".into(),
            "Reads state then layers vim. Requires reboot.".into(),
            steps,
        );
        let resp = plan_to_response(plan);

        assert!(
            resp.approval_required,
            "approval_required must be true when any step is high-risk"
        );
        assert_eq!(resp.steps.len(), 2);
        assert!(
            !resp.steps[0].approval_required,
            "low step should not require approval"
        );
        assert!(
            resp.steps[1].approval_required,
            "high step must require approval"
        );
    }
}
