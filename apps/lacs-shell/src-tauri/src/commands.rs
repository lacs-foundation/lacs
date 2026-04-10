use crate::events::{DaemonJobOutcome, TimelineEvent};
use lacs_brain::config::{BrainConfig, ProviderConfig};
use lacs_brain::planner::{LlmPlanner, Plan, PlanningError};
use lacs_brain::provider::LlmProvider;
use lacs_brain::providers::anthropic::AnthropicProvider;
use lacs_brain::providers::ollama::OllamaProvider;
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

// NOTE(task-8): DemoStateClient returns a hardcoded Silverblue fixture.
// Replace with a real StateClient that queries the daemon over the Unix socket
// before this shell is used in a production context.
#[derive(Clone, Debug, Default)]
pub struct DemoStateClient;

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

pub struct ShellCommandState {
    planner: LlmPlanner,
}

impl ShellCommandState {
    /// Create from environment-derived config.
    /// Falls back to Ollama defaults when no provider is configured.
    pub fn new() -> Self {
        let config = BrainConfig::from_env().unwrap_or_else(|_| BrainConfig::ollama_defaults());
        let provider = make_provider(&config);
        Self {
            planner: LlmPlanner::new(provider, Box::new(DemoStateClient), config.max_turns),
        }
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

fn make_provider(config: &BrainConfig) -> Box<dyn LlmProvider> {
    match &config.provider {
        ProviderConfig::Anthropic {
            api_key,
            model,
            base_url,
        } => Box::new(
            AnthropicProvider::new(api_key, model, base_url)
                .expect("failed to build Anthropic HTTP client"),
        ),
        ProviderConfig::Ollama { base_url, model } => Box::new(
            OllamaProvider::new(base_url, model).expect("failed to build Ollama HTTP client"),
        ),
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
        Completion, ContentBlock, Message, ProviderError, StopReason, ToolDefinition,
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
}
